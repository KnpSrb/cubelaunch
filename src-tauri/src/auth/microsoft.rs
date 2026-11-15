//! Вход через Microsoft:
//! OAuth2 device code → Xbox Live (XBL) → XSTS → Minecraft Services → профиль.
//!
//! Все адреса собраны в [`Endpoints`], чтобы поток можно было прогнать в тестах
//! против локального мок-сервера.

use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, AppResult, AuthError};

pub const SCOPE: &str = "XboxLive.signin offline_access";

/// Client ID, вшитый при сборке (`CUBELAUNCH_MSA_CLIENT_ID=... cargo build`).
pub const BUILT_IN_CLIENT_ID: Option<&str> = option_env!("CUBELAUNCH_MSA_CLIENT_ID");

#[derive(Debug, Clone)]
pub struct Endpoints {
    /// `.../consumers/oauth2/v2.0` — к нему добавляются `/devicecode` и `/token`.
    pub oauth: String,
    pub xbl: String,
    pub xsts: String,
    /// `https://api.minecraftservices.com`
    pub minecraft: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            oauth: "https://login.microsoftonline.com/consumers/oauth2/v2.0".into(),
            xbl: "https://user.auth.xboxlive.com/user/authenticate".into(),
            xsts: "https://xsts.auth.xboxlive.com/xsts/authorize".into(),
            minecraft: "https://api.minecraftservices.com".into(),
        }
    }
}

/// Итоговый Client ID: из настроек, иначе вшитый при сборке.
pub fn client_id(from_settings: Option<&str>) -> AppResult<String> {
    from_settings
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(BUILT_IN_CLIENT_ID)
        .map(str::to_string)
        .ok_or_else(|| AuthError::NotConfigured.into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    #[serde(skip)]
    pub device_code: String,
}

#[derive(Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Clone, Deserialize)]
pub struct MsaTokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Deserialize)]
struct OAuthError {
    error: String,
    #[serde(default)]
    error_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McToken {
    pub token: String,
    /// Unix-время истечения, секунды.
    pub expires_at: i64,
}

impl McToken {
    /// Действителен ли ещё хотя бы 5 минут.
    pub fn is_fresh(&self) -> bool {
        self.expires_at - 300 > Utc::now().timestamp()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skin {
    pub url: String,
    /// `CLASSIC` или `SLIM`.
    pub variant: String,
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Cape {
    pub url: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub alias: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct McProfile {
    /// UUID без дефисов.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<Skin>,
    #[serde(default)]
    pub capes: Vec<Cape>,
}

impl McProfile {
    pub fn active_skin(&self) -> Option<&Skin> {
        self.skins.iter().find(|s| s.state == "ACTIVE").or(self.skins.first())
    }
    pub fn active_cape(&self) -> Option<&Cape> {
        self.capes.iter().find(|c| c.state == "ACTIVE")
    }
}

fn other(context: &str, e: impl std::fmt::Display) -> AppError {
    AuthError::Other(format!("{context}: {e}")).into()
}

pub async fn request_device_code(client: &reqwest::Client, ep: &Endpoints, client_id: &str) -> AppResult<DeviceCode> {
    let resp = client
        .post(format!("{}/devicecode", ep.oauth))
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(match serde_json::from_str::<OAuthError>(&body) {
            // Неверный Client ID или приложение не настроено как публичный клиент
            // для личных учётных записей Microsoft.
            Ok(e) if matches!(e.error.as_str(), "unauthorized_client" | "invalid_client" | "invalid_request") => AuthError::Other(format!(
                "Microsoft не принимает этот Client ID ({}). Проверьте, что приложение Azure существует, \
                 поддерживает личные учётные записи Microsoft и в нём включены «public client flows».",
                e.error
            ))
            .into(),
            Ok(e) => other(&e.error, e.error_description.lines().next().unwrap_or_default()),
            Err(_) => other("не удалось получить код входа", body),
        });
    }
    let r: DeviceCodeResponse = resp.json().await?;
    Ok(DeviceCode {
        user_code: r.user_code,
        verification_uri: r.verification_uri,
        expires_in: r.expires_in,
        interval: r.interval,
        device_code: r.device_code,
    })
}

/// Ждёт, пока пользователь введёт код на странице Microsoft.
pub async fn poll_device_token(
    client: &reqwest::Client,
    ep: &Endpoints,
    client_id: &str,
    dc: &DeviceCode,
    cancel: &CancellationToken,
) -> AppResult<MsaTokens> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(dc.expires_in);
    let mut interval = dc.interval;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
            _ = cancel.cancelled() => return Err(AppError::Cancelled),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AuthError::CodeExpired.into());
        }
        let resp = client
            .post(format!("{}/token", ep.oauth))
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", dc.device_code.as_str()),
            ])
            .send()
            .await?;
        if resp.status().is_success() {
            return Ok(resp.json().await?);
        }
        let err: OAuthError = resp.json().await.map_err(|e| other("ответ Microsoft", e))?;
        match err.error.as_str() {
            "authorization_pending" => {}
            "slow_down" => interval += 5,
            "authorization_declined" | "access_denied" => return Err(AuthError::Declined.into()),
            "expired_token" | "code_expired" => return Err(AuthError::CodeExpired.into()),
            _ => return Err(other(&err.error, err.error_description)),
        }
    }
}

pub async fn refresh_msa(client: &reqwest::Client, ep: &Endpoints, client_id: &str, refresh: &str) -> AppResult<MsaTokens> {
    let resp = client
        .post(format!("{}/token", ep.oauth))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh),
            ("scope", SCOPE),
        ])
        .send()
        .await?;
    if resp.status().is_success() {
        return Ok(resp.json().await?);
    }
    let err: OAuthError = resp.json().await.map_err(|e| other("ответ Microsoft", e))?;
    if err.error == "invalid_grant" || err.error == "interaction_required" {
        return Err(AuthError::ReloginRequired.into());
    }
    Err(other(&err.error, err.error_description))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxResponse {
    token: String,
    display_claims: DisplayClaims,
}

#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<Xui>,
}

#[derive(Deserialize)]
struct Xui {
    uhs: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxError {
    #[serde(default)]
    x_err: u64,
}

fn xsts_error(code: u64) -> AuthError {
    match code {
        2148916233 => AuthError::NoXbox,
        2148916235 => AuthError::CountryBlocked,
        2148916236 | 2148916237 => AuthError::AdultVerification,
        2148916238 => AuthError::ChildAccount,
        c => AuthError::Other(format!("Xbox Live вернул код {c}")),
    }
}

async fn xbox_post(client: &reqwest::Client, url: &str, body: serde_json::Value) -> AppResult<(String, String)> {
    let resp = client.post(url).header("Accept", "application/json").json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err: Option<XboxError> = resp.json().await.ok();
        return Err(match err {
            Some(e) if e.x_err != 0 => xsts_error(e.x_err).into(),
            _ => other("Xbox Live", status),
        });
    }
    let r: XboxResponse = resp.json().await?;
    let uhs = r.display_claims.xui.into_iter().next().map(|x| x.uhs).ok_or_else(|| other("Xbox Live", "нет uhs"))?;
    Ok((r.token, uhs))
}

pub async fn xbox_live(client: &reqwest::Client, ep: &Endpoints, msa_access: &str) -> AppResult<(String, String)> {
    xbox_post(
        client,
        &ep.xbl,
        json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={msa_access}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }),
    )
    .await
}

pub async fn xsts(client: &reqwest::Client, ep: &Endpoints, xbl_token: &str) -> AppResult<(String, String)> {
    xbox_post(
        client,
        &ep.xsts,
        json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }),
    )
    .await
}

pub async fn minecraft_login(client: &reqwest::Client, ep: &Endpoints, uhs: &str, xsts_token: &str) -> AppResult<McToken> {
    #[derive(Deserialize)]
    struct R {
        access_token: String,
        expires_in: i64,
    }
    let resp = client
        .post(format!("{}/authentication/login_with_xbox", ep.minecraft))
        .json(&json!({ "identityToken": format!("XBL3.0 x={uhs};{xsts_token}") }))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err(AuthError::AppNotApproved.into());
    }
    if !resp.status().is_success() {
        return Err(other("Minecraft Services", resp.status()));
    }
    let r: R = resp.json().await?;
    Ok(McToken { token: r.access_token, expires_at: Utc::now().timestamp() + r.expires_in })
}

pub async fn profile(client: &reqwest::Client, ep: &Endpoints, mc_token: &str) -> AppResult<McProfile> {
    let resp = client
        .get(format!("{}/minecraft/profile", ep.minecraft))
        .bearer_auth(mc_token)
        .send()
        .await?;
    match resp.status() {
        reqwest::StatusCode::NOT_FOUND => Err(AuthError::NoGame.into()),
        reqwest::StatusCode::UNAUTHORIZED => Err(AuthError::ReloginRequired.into()),
        s if !s.is_success() => Err(other("профиль Minecraft", s)),
        _ => Ok(resp.json().await?),
    }
}

/// Этапы входа — для отображения в UI.
pub trait LoginProgress: Send + Sync {
    fn stage(&self, stage: &str);
}

impl<F: Fn(&str) + Send + Sync> LoginProgress for F {
    fn stage(&self, stage: &str) {
        self(stage)
    }
}

/// От токена Microsoft до токена и профиля Minecraft.
pub async fn minecraft_chain(
    client: &reqwest::Client,
    ep: &Endpoints,
    msa_access: &str,
    progress: &dyn LoginProgress,
) -> AppResult<(McToken, McProfile)> {
    progress.stage("xbox");
    let (xbl, _) = xbox_live(client, ep, msa_access).await?;
    progress.stage("xsts");
    let (xsts_token, uhs) = xsts(client, ep, &xbl).await?;
    progress.stage("minecraft");
    let mc = minecraft_login(client, ep, &uhs, &xsts_token).await?;
    progress.stage("profile");
    let prof = profile(client, ep, &mc.token).await?;
    Ok((mc, prof))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub fn endpoints(server: &MockServer) -> Endpoints {
        let u = server.uri();
        Endpoints {
            oauth: format!("{u}/oauth"),
            xbl: format!("{u}/xbl"),
            xsts: format!("{u}/xsts"),
            minecraft: format!("{u}/mc"),
        }
    }

    /// Мок «счастливого пути» для всего потока, кроме device code.
    pub async fn mount_chain(server: &MockServer, name: &str) {
        Mock::given(method("POST"))
            .and(path("/xbl"))
            .and(body_string_contains("d=msa-access"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Token": "xbl-token", "DisplayClaims": { "xui": [{ "uhs": "UHS1" }] }
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/xsts"))
            .and(body_string_contains("xbl-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "Token": "xsts-token", "DisplayClaims": { "xui": [{ "uhs": "UHS1" }] }
            })))
            .mount(server)
            .await;
        Mock::given(method("POST"))
            .and(path("/mc/authentication/login_with_xbox"))
            .and(body_string_contains("XBL3.0 x=UHS1;xsts-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "mc-token", "expires_in": 86400, "token_type": "Bearer"
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/mc/minecraft/profile"))
            .and(header("authorization", "Bearer mc-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "0123456789abcdef0123456789abcdef",
                "name": name,
                "skins": [{ "id": "s", "state": "ACTIVE", "url": "http://textures.minecraft.net/texture/abc", "variant": "SLIM" }],
                "capes": []
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn device_code_flow_with_pending_and_slow_down() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/devicecode"))
            .and(body_string_contains("client_id=test-client"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "device_code": "DEV", "user_code": "ABCD-1234",
                "verification_uri": "https://microsoft.com/link", "expires_in": 60, "interval": 0
            })))
            .mount(&server)
            .await;
        // Сначала «ждём ввода», потом токен.
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "error": "authorization_pending" })))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("device_code=DEV"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "msa-access", "refresh_token": "msa-refresh", "expires_in": 3600
            })))
            .mount(&server)
            .await;
        mount_chain(&server, "Steve").await;

        let ep = endpoints(&server);
        let client = reqwest::Client::new();
        let dc = request_device_code(&client, &ep, "test-client").await.unwrap();
        assert_eq!(dc.user_code, "ABCD-1234");
        let tokens = poll_device_token(&client, &ep, "test-client", &dc, &CancellationToken::new()).await.unwrap();
        assert_eq!(tokens.refresh_token, "msa-refresh");

        let stages = std::sync::Mutex::new(Vec::new());
        let (mc, prof) = minecraft_chain(&client, &ep, &tokens.access_token, &|s: &str| stages.lock().unwrap().push(s.to_string()))
            .await
            .unwrap();
        assert_eq!(mc.token, "mc-token");
        assert!(mc.is_fresh());
        assert_eq!(prof.name, "Steve");
        assert_eq!(prof.active_skin().unwrap().variant, "SLIM");
        assert_eq!(*stages.lock().unwrap(), vec!["xbox", "xsts", "minecraft", "profile"]);
    }

    #[tokio::test]
    async fn bad_client_id_gives_readable_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/devicecode"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": "unauthorized_client", "error_description": "AADSTS700016: Application not found\r\nTrace ID: x"
            })))
            .mount(&server)
            .await;
        let r = request_device_code(&reqwest::Client::new(), &endpoints(&server), "bad").await;
        match r {
            Err(AppError::Auth(AuthError::Other(m))) => assert!(m.contains("public client flows"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn declined_and_cancelled() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "error": "authorization_declined" })))
            .mount(&server)
            .await;
        let ep = endpoints(&server);
        let client = reqwest::Client::new();
        let dc = DeviceCode {
            user_code: "X".into(),
            verification_uri: String::new(),
            expires_in: 60,
            interval: 0,
            device_code: "D".into(),
        };
        let r = poll_device_token(&client, &ep, "c", &dc, &CancellationToken::new()).await;
        assert!(matches!(r, Err(AppError::Auth(AuthError::Declined))));

        let cancel = CancellationToken::new();
        cancel.cancel();
        let slow = DeviceCode { interval: 30, ..dc };
        let r = poll_device_token(&client, &ep, "c", &slow, &cancel).await;
        assert!(matches!(r, Err(AppError::Cancelled)));
    }

    #[tokio::test]
    async fn xsts_errors_are_mapped() {
        for (code, expected) in [
            (2148916233u64, AuthError::NoXbox),
            (2148916238, AuthError::ChildAccount),
            (2148916235, AuthError::CountryBlocked),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/xsts"))
                .respond_with(ResponseTemplate::new(401).set_body_json(json!({ "Identity": "0", "XErr": code, "Message": "" })))
                .mount(&server)
                .await;
            let r = xsts(&reqwest::Client::new(), &endpoints(&server), "xbl").await;
            assert!(matches!(r, Err(AppError::Auth(ref e)) if *e == expected), "{code}: {r:?}");
        }
    }

    #[tokio::test]
    async fn minecraft_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mc/authentication/login_with_xbox"))
            .respond_with(ResponseTemplate::new(403).set_body_string("Invalid app registration"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/mc/minecraft/profile"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let ep = endpoints(&server);
        let c = reqwest::Client::new();
        assert!(matches!(minecraft_login(&c, &ep, "u", "x").await, Err(AppError::Auth(AuthError::AppNotApproved))));
        assert!(matches!(profile(&c, &ep, "t").await, Err(AppError::Auth(AuthError::NoGame))));
    }

    #[tokio::test]
    async fn refresh_invalid_grant_requires_relogin() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "error": "invalid_grant", "error_description": "expired" })))
            .mount(&server)
            .await;
        let r = refresh_msa(&reqwest::Client::new(), &endpoints(&server), "c", "old").await;
        assert!(matches!(r, Err(AppError::Auth(AuthError::ReloginRequired))));
    }

    #[test]
    fn client_id_resolution() {
        assert_eq!(client_id(Some(" abc ")).unwrap(), "abc");
        if BUILT_IN_CLIENT_ID.is_none() {
            assert!(matches!(client_id(None), Err(AppError::Auth(AuthError::NotConfigured))));
            assert!(matches!(client_id(Some("")), Err(AppError::Auth(AuthError::NotConfigured))));
        }
    }
}
