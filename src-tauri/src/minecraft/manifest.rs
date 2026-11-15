//! Манифест версий Mojang (`version_manifest_v2.json`) с кэшем на диске.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::util::{is_fresh, read_json_opt, write_json_atomic};

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestVersion {
    pub id: String,
    /// `release` | `snapshot` | `old_beta` | `old_alpha`
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub release_time: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

impl VersionManifest {
    pub fn find(&self, id: &str) -> Option<&ManifestVersion> {
        self.versions.iter().find(|v| v.id == id)
    }
}

/// Возвращает манифест: из свежего кэша, иначе из сети; при ошибке сети — из
/// устаревшего кэша (чтобы лаунчер работал офлайн).
pub async fn get_manifest(
    client: &reqwest::Client,
    meta_dir: &Path,
    force: bool,
) -> AppResult<VersionManifest> {
    let cache = meta_dir.join("version_manifest_v2.json");
    if !force && is_fresh(&cache, CACHE_TTL) {
        if let Ok(Some(m)) = read_json_opt::<VersionManifest>(&cache) {
            return Ok(m);
        }
    }
    let fetched: AppResult<VersionManifest> = async {
        let resp = client.get(MANIFEST_URL).send().await?.error_for_status()?;
        Ok(resp.json::<VersionManifest>().await?)
    }
    .await;
    match fetched {
        Ok(m) => {
            write_json_atomic(&cache, &m)?;
            Ok(m)
        }
        Err(e) => {
            tracing::warn!("не удалось обновить манифест версий: {e}");
            read_json_opt::<VersionManifest>(&cache)?.ok_or(e)
        }
    }
}
