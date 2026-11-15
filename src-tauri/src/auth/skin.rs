//! Скины: скачиваем текстуру скина/плаща с серверов Mojang и отдаём UI как
//! data URL (без проблем с CORS у WebGL-просмотрщика). Кэш — `<data>/cache/skins`.

use std::path::Path;

use base64::Engine;
use serde::Serialize;

use super::Account;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinData {
    /// `data:image/png;base64,...`
    pub skin: String,
    /// `classic` или `slim`.
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cape: Option<String>,
}

/// Текстуры отдаются только с доменов Mojang — не ходим по произвольным ссылкам.
fn allowed(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| h == "textures.minecraft.net")
}

async fn fetch_png(client: &reqwest::Client, cache_dir: &Path, url: &str) -> AppResult<String> {
    if !allowed(url) {
        return Err(AppError::InvalidInput(format!("недопустимый адрес текстуры: {url}")));
    }
    // Имя файла текстуры — её хэш, поэтому кэш никогда не устаревает.
    let name = url.rsplit('/').next().unwrap_or("skin").chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>();
    let path = cache_dir.join(format!("{name}.png"));
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => {
            // Mojang отдаёт ссылки http — переводим на https.
            let https = url.replacen("http://", "https://", 1);
            let b = client.get(&https).send().await?.error_for_status()?.bytes().await?.to_vec();
            if !b.starts_with(b"\x89PNG") {
                return Err(AppError::Other("сервер вернул не PNG".into()));
            }
            tokio::fs::create_dir_all(cache_dir).await?;
            tokio::fs::write(&path, &b).await?;
            b
        }
    };
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

pub async fn load(client: &reqwest::Client, cache_dir: &Path, account: &Account) -> AppResult<Option<SkinData>> {
    let Some(skin) = &account.skin else { return Ok(None) };
    let cape = match &account.cape {
        Some(c) => fetch_png(client, cache_dir, &c.url).await.ok(),
        None => None,
    };
    Ok(Some(SkinData {
        skin: fetch_png(client, cache_dir, &skin.url).await?,
        model: if skin.variant.eq_ignore_ascii_case("slim") { "slim" } else { "classic" }.into(),
        cape,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mojang_texture_hosts() {
        assert!(allowed("http://textures.minecraft.net/texture/abc"));
        assert!(!allowed("https://evil.example/texture/abc"));
        assert!(!allowed("file:///etc/passwd"));
    }
}
