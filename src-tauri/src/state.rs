use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::auth::microsoft::{DeviceCode, Endpoints};
use crate::auth::secrets::{KeyringStore, SecretStore};
use crate::auth::AccountStore;
use crate::downloads::build_http_client;
use crate::downloads::jobs::Jobs;
use crate::downloads::queue::DownloadOptions;
use crate::error::AppResult;
use crate::instances::InstanceStore;
use crate::java::detect::detect_all;
use crate::java::JavaInstall;
use crate::minecraft::session::GameSessions;
use crate::paths::AppPaths;
use crate::settings::Settings;

/// Общее состояние приложения, доступное во всех Tauri-командах.
pub struct AppState {
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub http: RwLock<reqwest::Client>,
    pub instances: InstanceStore,
    /// Порядок групп инстансов (`groups.json`).
    pub groups: crate::instances::groups::GroupStore,
    pub accounts: AccountStore,
    pub jobs: Arc<Jobs>,
    pub games: GameSessions,
    /// Установки версий идут по одной: разные инстансы делят общие файлы
    /// (библиотеки, assets), и параллельная запись одного файла недопустима.
    pub install_lock: Mutex<()>,
    /// Кэш найденных JVM (поиск занимает секунды).
    java_cache: Mutex<Option<Vec<JavaInstall>>>,
    pub secrets: Box<dyn SecretStore>,
    pub msa_endpoints: Endpoints,
    /// Незавершённый вход через Microsoft (код показан пользователю).
    pub pending_login: std::sync::Mutex<Option<PendingLogin>>,
    /// Разобранные моды/паки (по пути, размеру и времени изменения).
    pub content_cache: crate::content::ContentCache,
    /// SHA1 → проект Modrinth (`None` — файл не с Modrinth). Живёт до перезапуска.
    pub modrinth_hashes: std::sync::Mutex<std::collections::HashMap<String, Option<crate::commands::content::ModrinthInfo>>>,
    /// Ключ CurseForge из keyring (кэш: внешний `None` — ещё не читали).
    cf_key: std::sync::Mutex<Option<Option<String>>>,
}

pub struct PendingLogin {
    pub device: DeviceCode,
    pub client_id: String,
    pub cancel: CancellationToken,
}

impl AppState {
    pub fn init(paths: AppPaths) -> AppResult<Self> {
        paths.ensure()?;
        let settings = Settings::load(&paths.settings_file())?;
        let http = build_http_client(&settings).or_else(|e| {
            tracing::warn!("некорректные сетевые настройки ({e}), используется клиент по умолчанию");
            build_http_client(&Settings::default())
        })?;
        let max_jobs = settings.downloads.max_jobs as usize;
        Ok(Self {
            instances: InstanceStore::new(paths.instances_dir()),
            groups: crate::instances::groups::GroupStore::new(paths.data_dir.join("groups.json")),
            accounts: AccountStore::new(paths.accounts_file()),
            paths,
            settings: RwLock::new(settings),
            http: RwLock::new(http),
            jobs: {
                let jobs = Arc::new(Jobs::default());
                jobs.set_limit(max_jobs);
                jobs
            },
            games: GameSessions::default(),
            install_lock: Mutex::new(()),
            java_cache: Mutex::new(None),
            secrets: Box::new(KeyringStore),
            msa_endpoints: Endpoints::default(),
            pending_login: std::sync::Mutex::new(None),
            content_cache: Default::default(),
            modrinth_hashes: Default::default(),
            cf_key: Default::default(),
        })
    }

    pub async fn msa_client_id(&self) -> AppResult<String> {
        crate::auth::microsoft::client_id(self.settings.read().await.msa_client_id.as_deref())
    }

    pub async fn update_settings(&self, new: Settings) -> AppResult<Settings> {
        new.save(&self.paths.settings_file())?;
        *self.http.write().await = build_http_client(&new)?;
        self.jobs.set_limit(new.downloads.max_jobs as usize);
        *self.settings.write().await = new.clone();
        Ok(new)
    }

    /// Ключ CurseForge, введённый пользователем (хранится в keyring).
    pub fn curseforge_user_key(&self) -> AppResult<Option<String>> {
        let mut cache = self.cf_key.lock().unwrap();
        if let Some(k) = cache.as_ref() {
            return Ok(k.clone());
        }
        let k = self.secrets.get(crate::auth::secrets::CURSEFORGE_KEY)?.filter(|k| !k.trim().is_empty());
        *cache = Some(k.clone());
        Ok(k)
    }

    pub fn set_curseforge_user_key(&self, key: Option<&str>) -> AppResult<()> {
        match key {
            Some(k) => self.secrets.set(crate::auth::secrets::CURSEFORGE_KEY, k)?,
            None => self.secrets.delete(crate::auth::secrets::CURSEFORGE_KEY)?,
        }
        *self.cf_key.lock().unwrap() = Some(key.map(str::to_string));
        Ok(())
    }

    /// Действующий ключ CurseForge: пользовательский, иначе вшитый при сборке.
    pub fn curseforge_key(&self) -> AppResult<String> {
        let user = self.curseforge_user_key().unwrap_or_else(|e| {
            tracing::warn!("не удалось прочитать ключ CurseForge: {e}");
            None
        });
        user.or_else(|| crate::content::curseforge::BUILT_IN_KEY.map(str::to_string))
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| crate::error::AppError::CurseforgeKey("CurseForge не настроен: укажите ключ API в настройках".into()))
    }

    pub async fn client(&self) -> reqwest::Client {
        self.http.read().await.clone()
    }

    pub async fn download_options(&self) -> DownloadOptions {
        let s = self.settings.read().await;
        DownloadOptions {
            concurrency: s.downloads.concurrency as usize,
            retries: s.downloads.retries,
        }
    }

    /// Найденные JVM (из кэша, если `force` не задан).
    pub async fn java_installs(&self, force: bool) -> Vec<JavaInstall> {
        let mut cache = self.java_cache.lock().await;
        if force || cache.is_none() {
            *cache = Some(detect_all(&self.paths.java_dir()).await);
        }
        cache.clone().unwrap_or_default()
    }

    pub async fn invalidate_java_cache(&self) {
        *self.java_cache.lock().await = None;
    }
}
