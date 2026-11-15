//! Сеть и единая очередь загрузок.
//!
//! - общий HTTP-клиент с учётом настроек прокси и таймаута;
//! - [`queue`] — параллельная загрузка с SHA1, докачкой, повторами и отменой;
//! - [`jobs`] — фоновые задачи с прогрессом для UI.

pub mod jobs;
pub mod queue;

use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::settings::{ProxyMode, Settings};

pub const USER_AGENT: &str = concat!("Cubelaunch/", env!("CARGO_PKG_VERSION"));

/// Собирает HTTP-клиент по текущим настройкам.
pub fn build_http_client(settings: &Settings) -> AppResult<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(settings.downloads.timeout_secs as u64))
        .read_timeout(Duration::from_secs(settings.downloads.timeout_secs as u64));

    let p = &settings.proxy;
    b = match p.mode {
        // reqwest по умолчанию учитывает системные переменные HTTP(S)_PROXY.
        ProxyMode::System => b,
        ProxyMode::None => b.no_proxy(),
        ProxyMode::Http | ProxyMode::Socks5 => {
            let scheme = if p.mode == ProxyMode::Http { "http" } else { "socks5h" };
            let url = format!("{scheme}://{}:{}", p.host.trim(), p.port);
            let mut proxy = reqwest::Proxy::all(&url)
                .map_err(|e| AppError::InvalidSettings(format!("прокси: {e}")))?;
            if !p.username.is_empty() {
                // Пароль прокси появится вместе с keyring (этап 4).
                proxy = proxy.basic_auth(&p.username, "");
            }
            b.proxy(proxy)
        }
    };
    Ok(b.build()?)
}
