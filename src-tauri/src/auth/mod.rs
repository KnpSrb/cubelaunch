//! Аккаунты: Microsoft (OAuth2 device code → Xbox Live → XSTS → Minecraft
//! Services) и офлайн (явно нелицензионные), выбор аккаунта по умолчанию.
//!
//! Файл `<data>/accounts.json` хранит только несекретные данные (ник, UUID,
//! ссылка на скин). Refresh-токен Microsoft и токен Minecraft лежат в системном
//! keyring (см. [`secrets`]).

pub mod microsoft;
pub mod secrets;
pub mod skin;

use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use self::microsoft::{minecraft_chain, refresh_msa, Cape, Endpoints, McProfile, McToken, MsaTokens, Skin};
use self::secrets::{minecraft_key, refresh_key, SecretStore};
use crate::error::{AppError, AppResult, AuthError};
use crate::util::{read_json_opt, write_json_atomic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Offline,
    Microsoft,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub kind: AccountKind,
    pub username: String,
    /// UUID игрока без дефисов.
    pub uuid: String,
    pub added_at: DateTime<Utc>,
    /// Активный скин (только Microsoft).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<Skin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cape: Option<Cape>,
}

impl Account {
    pub fn from_profile(p: &McProfile, added_at: DateTime<Utc>) -> Self {
        Self {
            id: format!("msa-{}", p.id),
            kind: AccountKind::Microsoft,
            username: p.name.clone(),
            uuid: p.id.clone(),
            added_at,
            skin: p.active_skin().cloned(),
            cape: p.active_cape().cloned(),
        }
    }
}

/// Всё, что нужно для операций с Microsoft-аккаунтами.
pub struct MsaContext<'a> {
    pub client: &'a reqwest::Client,
    pub ep: &'a Endpoints,
    pub client_id: &'a str,
    pub secrets: &'a dyn SecretStore,
}

fn store_mc_token(secrets: &dyn SecretStore, account_id: &str, mc: &McToken) {
    // Кэш токена Minecraft необязателен: если keyring его не принял (лимит
    // размера), просто получим токен заново при следующем запуске.
    let json = serde_json::to_string(mc).unwrap_or_default();
    if let Err(e) = secrets.set(&minecraft_key(account_id), &json) {
        tracing::warn!("токен Minecraft не закэширован: {e}");
    }
}

/// Сохраняет результат успешного входа.
pub fn finish_login(
    secrets: &dyn SecretStore,
    store: &AccountStore,
    tokens: &MsaTokens,
    mc: &McToken,
    prof: &McProfile,
) -> AppResult<Account> {
    let existing = store.load()?.accounts.into_iter().find(|a| a.id == format!("msa-{}", prof.id));
    let acc = Account::from_profile(prof, existing.map(|a| a.added_at).unwrap_or_else(Utc::now));
    secrets.set(&refresh_key(&acc.id), &tokens.refresh_token)?;
    store_mc_token(secrets, &acc.id, mc);
    store.upsert(acc)
}

/// Действующий токен Minecraft для запуска: из кэша, иначе через refresh-токен
/// (с обновлением ника и скина). Возвращает токен и актуальный аккаунт.
pub async fn ensure_token(ctx: &MsaContext<'_>, store: &AccountStore, account: &Account, force: bool) -> AppResult<(String, Account)> {
    if !force {
        let cached = ctx
            .secrets
            .get(&minecraft_key(&account.id))?
            .and_then(|s| serde_json::from_str::<McToken>(&s).ok());
        if let Some(t) = cached.filter(McToken::is_fresh) {
            return Ok((t.token, account.clone()));
        }
    }
    let refresh = ctx.secrets.get(&refresh_key(&account.id))?.ok_or(AuthError::ReloginRequired)?;
    let msa = refresh_msa(ctx.client, ctx.ep, ctx.client_id, &refresh).await?;
    // Microsoft выдаёт новый refresh-токен при каждом обновлении.
    ctx.secrets.set(&refresh_key(&account.id), &msa.refresh_token)?;
    let (mc, prof) = minecraft_chain(ctx.client, ctx.ep, &msa.access_token, &|_: &str| {}).await?;
    store_mc_token(ctx.secrets, &account.id, &mc);
    let updated = store.upsert(Account::from_profile(&prof, account.added_at))?;
    Ok((mc.token, updated))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountsFile {
    pub accounts: Vec<Account>,
    pub default_id: Option<String>,
}

/// Офлайн-UUID как у ванильного сервера: `UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`
/// (MD5, версия 3).
pub fn offline_uuid(name: &str) -> String {
    let mut h = md5::compute(format!("OfflinePlayer:{name}")).0;
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    h.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn validate_offline_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    let ok = (3..=16).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Err(AppError::InvalidInput(
            "ник должен содержать 3–16 символов: латинские буквы, цифры и «_»".into(),
        ));
    }
    Ok(n.to_string())
}

pub struct AccountStore {
    file: PathBuf,
    lock: Mutex<()>,
}

impl AccountStore {
    pub fn new(file: PathBuf) -> Self {
        Self { file, lock: Mutex::new(()) }
    }

    pub fn load(&self) -> AppResult<AccountsFile> {
        Ok(read_json_opt(&self.file)?.unwrap_or_default())
    }

    fn modify<T>(&self, f: impl FnOnce(&mut AccountsFile) -> AppResult<T>) -> AppResult<T> {
        let _g = self.lock.lock().unwrap();
        let mut data = self.load()?;
        let r = f(&mut data)?;
        // Аккаунт по умолчанию всегда указывает на существующий.
        if data.default_id.as_ref().is_none_or(|d| !data.accounts.iter().any(|a| &a.id == d)) {
            data.default_id = data.accounts.first().map(|a| a.id.clone());
        }
        write_json_atomic(&self.file, &data)?;
        Ok(r)
    }

    pub fn add_offline(&self, name: &str) -> AppResult<Account> {
        let username = validate_offline_name(name)?;
        self.modify(|d| {
            if let Some(a) = d
                .accounts
                .iter()
                .find(|a| a.kind == AccountKind::Offline && a.username.eq_ignore_ascii_case(&username))
            {
                return Err(AppError::InvalidInput(format!("офлайн-аккаунт {} уже есть", a.username)));
            }
            let acc = Account {
                id: uuid::Uuid::new_v4().simple().to_string(),
                kind: AccountKind::Offline,
                uuid: offline_uuid(&username),
                username,
                added_at: Utc::now(),
                skin: None,
                cape: None,
            };
            d.accounts.push(acc.clone());
            Ok(acc)
        })
    }

    /// Добавляет аккаунт или обновляет существующий с тем же id.
    pub fn upsert(&self, acc: Account) -> AppResult<Account> {
        self.modify(|d| {
            match d.accounts.iter_mut().find(|a| a.id == acc.id) {
                Some(a) => *a = acc.clone(),
                None => d.accounts.push(acc.clone()),
            }
            Ok(acc)
        })
    }

    pub fn get(&self, id: &str) -> AppResult<Account> {
        self.load()?
            .accounts
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| AppError::NotFound(format!("аккаунт {id}")))
    }

    pub fn remove(&self, id: &str) -> AppResult<()> {
        self.modify(|d| {
            let before = d.accounts.len();
            d.accounts.retain(|a| a.id != id);
            if d.accounts.len() == before {
                return Err(AppError::NotFound(format!("аккаунт {id}")));
            }
            Ok(())
        })
    }

    pub fn set_default(&self, id: &str) -> AppResult<()> {
        self.modify(|d| {
            if !d.accounts.iter().any(|a| a.id == id) {
                return Err(AppError::NotFound(format!("аккаунт {id}")));
            }
            d.default_id = Some(id.to_string());
            Ok(())
        })
    }

    pub fn default_account(&self) -> AppResult<Account> {
        let d = self.load()?;
        d.default_id
            .and_then(|id| d.accounts.into_iter().find(|a| a.id == id))
            .ok_or(AppError::NoAccount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_vanilla() {
        // Известное значение UUID.nameUUIDFromBytes("OfflinePlayer:Notch").
        assert_eq!(offline_uuid("Notch"), "b50ad385829d3141a2167e7d7539ba7f");
    }

    #[test]
    fn validates_names() {
        assert!(validate_offline_name("Steve_1").is_ok());
        assert!(validate_offline_name("ab").is_err());
        assert!(validate_offline_name("Стив").is_err());
        assert!(validate_offline_name("a".repeat(17).as_str()).is_err());
    }

    #[test]
    fn add_default_remove() {
        let dir = tempfile::tempdir().unwrap();
        let s = AccountStore::new(dir.path().join("accounts.json"));
        assert!(matches!(s.default_account(), Err(AppError::NoAccount)));
        let a = s.add_offline("Alex").unwrap();
        let b = s.add_offline("Steve").unwrap();
        assert!(s.add_offline("alex").is_err(), "дубликат без учёта регистра");
        assert_eq!(s.default_account().unwrap().id, a.id, "первый аккаунт — по умолчанию");
        s.set_default(&b.id).unwrap();
        assert_eq!(s.default_account().unwrap().id, b.id);
        s.remove(&b.id).unwrap();
        assert_eq!(s.default_account().unwrap().id, a.id, "после удаления — следующий");
    }

    use super::microsoft::tests::{endpoints, mount_chain};
    use super::secrets::MemoryStore;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn profile(name: &str) -> McProfile {
        McProfile { id: "0123456789abcdef0123456789abcdef".into(), name: name.into(), skins: vec![], capes: vec![] }
    }

    #[tokio::test]
    async fn login_then_cached_then_refreshed_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(dir.path().join("accounts.json"));
        let secrets = MemoryStore::default();
        let tokens = MsaTokens { access_token: "a".into(), refresh_token: "refresh-1".into() };
        let fresh = McToken { token: "cached".into(), expires_at: Utc::now().timestamp() + 3600 };
        let acc = finish_login(&secrets, &store, &tokens, &fresh, &profile("OldName")).unwrap();
        assert_eq!(acc.id, "msa-0123456789abcdef0123456789abcdef");
        assert_eq!(store.default_account().unwrap().id, acc.id);
        // Повторный вход тем же аккаунтом не плодит дубликаты.
        finish_login(&secrets, &store, &tokens, &fresh, &profile("OldName")).unwrap();
        assert_eq!(store.load().unwrap().accounts.len(), 1);

        // Сервер, на котором ничего не замокано: кэшированный токен не требует сети.
        let server = MockServer::start().await;
        let ep = endpoints(&server);
        let client = reqwest::Client::new();
        let ctx = MsaContext { client: &client, ep: &ep, client_id: "c", secrets: &secrets };
        let (t, _) = ensure_token(&ctx, &store, &acc, false).await.unwrap();
        assert_eq!(t, "cached");

        // Истёкший токен → обновление через refresh-токен, ник подтягивается.
        let expired = McToken { token: "old".into(), expires_at: 0 };
        secrets.set(&minecraft_key(&acc.id), &serde_json::to_string(&expired).unwrap()).unwrap();
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("refresh_token=refresh-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "msa-access", "refresh_token": "refresh-2"
            })))
            .mount(&server)
            .await;
        mount_chain(&server, "NewName").await;
        let (t, updated) = ensure_token(&ctx, &store, &acc, false).await.unwrap();
        assert_eq!(t, "mc-token");
        assert_eq!(updated.username, "NewName");
        assert_eq!(updated.skin.as_ref().unwrap().variant, "SLIM");
        assert_eq!(secrets.get(&refresh_key(&acc.id)).unwrap().as_deref(), Some("refresh-2"), "refresh-токен ротируется");
        assert_eq!(store.get(&acc.id).unwrap().username, "NewName");
    }

    #[tokio::test]
    async fn missing_refresh_token_requires_relogin() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(dir.path().join("accounts.json"));
        let acc = store.upsert(Account::from_profile(&profile("X"), Utc::now())).unwrap();
        let secrets = MemoryStore::default();
        let ep = Endpoints::default();
        let client = reqwest::Client::new();
        let ctx = MsaContext { client: &client, ep: &ep, client_id: "c", secrets: &secrets };
        let r = ensure_token(&ctx, &store, &acc, false).await;
        assert!(matches!(r, Err(AppError::Auth(AuthError::ReloginRequired))));
    }
}
