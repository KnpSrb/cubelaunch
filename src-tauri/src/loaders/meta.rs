//! Списки версий загрузчиков для выбранной версии Minecraft.
//!
//! Ответы кэшируются в `meta/loaders/` на час; без сети используется
//! устаревший кэш.

use std::cmp::Ordering;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::validate_token;
use crate::error::{AppError, AppResult};
use crate::instances::LoaderKind;
use crate::util::{is_fresh, read_json_opt, write_json_atomic};

pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
pub const QUILT_META: &str = "https://meta.quiltmc.org/v3";
pub const FORGE_FILES: &str = "https://files.minecraftforge.net/net/minecraftforge/forge";
pub const NEOFORGE_API: &str = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged";

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersion {
    /// Полная версия, как её понимает репозиторий загрузчика.
    pub version: String,
    /// Для показа (у Forge без префикса версии Minecraft).
    pub label: String,
    pub stable: bool,
    /// Рекомендуемая (выбирается по умолчанию).
    pub recommended: bool,
}

/// GET JSON с кэшем на диске. `Ok(None)` — сервер ответил 400/404 (версия не поддерживается).
async fn fetch_cached(client: &reqwest::Client, url: &str, cache: &Path) -> AppResult<Option<Value>> {
    if is_fresh(cache, CACHE_TTL) {
        if let Ok(Some(v)) = read_json_opt::<Value>(cache) {
            return Ok(Some(v));
        }
    }
    let fetched: AppResult<Option<Value>> = async {
        let resp = client.get(url).send().await?;
        if matches!(resp.status().as_u16(), 400 | 404) {
            return Ok(None);
        }
        Ok(Some(resp.error_for_status()?.json::<Value>().await?))
    }
    .await;
    match fetched {
        Ok(Some(v)) => {
            write_json_atomic(cache, &v)?;
            Ok(Some(v))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            tracing::warn!("не удалось получить {url}: {e}");
            read_json_opt::<Value>(cache)?.map(Some).ok_or(e)
        }
    }
}

/// Сравнение версий по числовым фрагментам: `47.10.0` > `47.9.1`, `0.16.10` > `0.16.9`.
pub fn cmp_versions(a: &str, b: &str) -> Ordering {
    fn parts(s: &str) -> Vec<Result<u64, String>> {
        s.split(['.', '-', '+', '_'])
            .map(|p| p.parse::<u64>().map_err(|_| p.to_string()))
            .collect()
    }
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(pb.iter()) {
        let o = match (x, y) {
            (Ok(x), Ok(y)) => x.cmp(y),
            // Число старше метки: 1.0.0 > 1.0.0-beta.
            (Ok(_), Err(_)) => Ordering::Greater,
            (Err(_), Ok(_)) => Ordering::Less,
            (Err(x), Err(y)) => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    // Более длинная версия без меток старше; с меткой (`-beta`) — младше.
    match pa.len().cmp(&pb.len()) {
        Ordering::Greater if pa[pb.len()..].iter().any(|p| p.is_err()) => Ordering::Less,
        Ordering::Less if pb[pa.len()..].iter().any(|p| p.is_err()) => Ordering::Greater,
        o => o,
    }
}

fn is_prerelease(v: &str) -> bool {
    let l = v.to_lowercase();
    ["alpha", "beta", "pre", "rc"].iter().any(|t| l.contains(t))
}

/// Новые сверху; первая стабильная — рекомендуемая (если репозиторий не сказал иное).
fn finish(mut list: Vec<LoaderVersion>, recommend_first_stable: bool) -> Vec<LoaderVersion> {
    list.sort_by(|a, b| cmp_versions(&b.version, &a.version));
    if recommend_first_stable && !list.iter().any(|v| v.recommended) {
        if let Some(v) = list.iter_mut().find(|v| v.stable) {
            v.recommended = true;
        }
    }
    list
}

pub fn parse_fabric(v: &Value) -> Vec<LoaderVersion> {
    let list = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let l = e.get("loader")?;
            let version = l.get("version")?.as_str()?.to_string();
            let stable = l.get("stable").and_then(Value::as_bool).unwrap_or(!is_prerelease(&version));
            Some(LoaderVersion { label: version.clone(), version, stable, recommended: false })
        })
        .collect();
    finish(list, true)
}

/// Forge: `maven-metadata.json` (MC → список версий) + `promotions_slim.json`.
pub fn parse_forge(metadata: &Value, promos: Option<&Value>, mc: &str) -> Vec<LoaderVersion> {
    let promo = |kind: &str| {
        promos
            .and_then(|p| p.get("promos"))
            .and_then(|p| p.get(format!("{mc}-{kind}")))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let recommended = promo("recommended");
    let list = metadata
        .get(mc)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|full| {
            let label = super::short_version(mc, full);
            LoaderVersion {
                version: full.to_string(),
                recommended: recommended.as_deref() == Some(label.as_str()),
                stable: !is_prerelease(&label),
                label,
            }
        })
        .collect();
    finish(list, false)
}

/// Версия Minecraft по версии NeoForge: `21.1.77` → `1.21.1`, `21.0.5` → `1.21`,
/// `26.1.0.12` → `26.1`, `26.1.1.3` → `26.1.1` (новая схема с 2026 года).
pub fn neoforge_minecraft(v: &str) -> Option<String> {
    let core = v.split(['-', '+']).next()?;
    let nums: Vec<u32> = core.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match nums.as_slice() {
        [a, b, c, _, ..] if *a >= 25 => Some(if *c == 0 { format!("{a}.{b}") } else { format!("{a}.{b}.{c}") }),
        [a, b, _] if (20..25).contains(a) => Some(if *b == 0 { format!("1.{a}") } else { format!("1.{a}.{b}") }),
        _ => None,
    }
}

/// NeoForge: список версий `neoforge` (+ `forge` для 1.20.1, где NeoForge ещё был форком Forge).
pub fn parse_neoforge(neoforge: Option<&Value>, legacy_forge: Option<&Value>, mc: &str) -> Vec<LoaderVersion> {
    let versions = |v: Option<&Value>| -> Vec<String> {
        v.and_then(|v| v.get("versions"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    };
    let mut list: Vec<LoaderVersion> = versions(neoforge)
        .into_iter()
        .filter(|v| neoforge_minecraft(v).as_deref() == Some(mc))
        .map(|v| LoaderVersion { label: v.clone(), stable: !is_prerelease(&v), version: v, recommended: false })
        .collect();
    if mc == "1.20.1" {
        list.extend(versions(legacy_forge).into_iter().filter(|v| v.starts_with("1.20.1-")).map(|v| LoaderVersion {
            label: super::short_version(mc, &v),
            stable: !is_prerelease(&v),
            version: v,
            recommended: false,
        }));
    }
    finish(list, true)
}

pub async fn list_versions(
    client: &reqwest::Client,
    meta_dir: &Path,
    kind: LoaderKind,
    mc: &str,
) -> AppResult<Vec<LoaderVersion>> {
    validate_token(mc, "Minecraft")?;
    let dir = meta_dir.join("loaders");
    Ok(match kind {
        LoaderKind::Fabric => {
            let url = format!("{FABRIC_META}/versions/loader/{mc}");
            fetch_cached(client, &url, &dir.join(format!("fabric-{mc}.json"))).await?.map(|v| parse_fabric(&v)).unwrap_or_default()
        }
        LoaderKind::Quilt => {
            let url = format!("{QUILT_META}/versions/loader/{mc}");
            fetch_cached(client, &url, &dir.join(format!("quilt-{mc}.json"))).await?.map(|v| parse_fabric(&v)).unwrap_or_default()
        }
        LoaderKind::Forge => {
            let meta = fetch_cached(client, &format!("{FORGE_FILES}/maven-metadata.json"), &dir.join("forge-metadata.json"))
                .await?
                .ok_or_else(|| AppError::NotFound("список версий Forge".into()))?;
            // Без промо-версий список всё равно полезен.
            let promos = fetch_cached(client, &format!("{FORGE_FILES}/promotions_slim.json"), &dir.join("forge-promotions.json"))
                .await
                .ok()
                .flatten();
            parse_forge(&meta, promos.as_ref(), mc)
        }
        LoaderKind::NeoForge => {
            let neo = fetch_cached(client, &format!("{NEOFORGE_API}/neoforge"), &dir.join("neoforge.json")).await?;
            let legacy = if mc == "1.20.1" {
                fetch_cached(client, &format!("{NEOFORGE_API}/forge"), &dir.join("neoforge-forge.json")).await?
            } else {
                None
            };
            parse_neoforge(neo.as_ref(), legacy.as_ref(), mc)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn version_ordering() {
        assert_eq!(cmp_versions("0.16.10", "0.16.9"), Ordering::Greater);
        assert_eq!(cmp_versions("47.10.0", "47.9.1"), Ordering::Greater);
        assert_eq!(cmp_versions("21.1.0-beta", "21.1.0"), Ordering::Less);
        assert_eq!(cmp_versions("0.27.1-beta.1", "0.27.0"), Ordering::Greater);
        assert_eq!(cmp_versions("1.0", "1.0"), Ordering::Equal);
    }

    #[test]
    fn fabric_and_quilt() {
        let v = json!([
            { "loader": { "version": "0.16.9", "stable": true } },
            { "loader": { "version": "0.16.10", "stable": false } },
            { "loader": { "version": "0.16.8", "stable": true } }
        ]);
        let list = parse_fabric(&v);
        assert_eq!(list.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(), vec!["0.16.10", "0.16.9", "0.16.8"]);
        assert!(list[1].recommended && !list[0].recommended && !list[2].recommended);

        // Quilt: флага stable нет — определяем по метке.
        let q = parse_fabric(&json!([{ "loader": { "version": "0.28.0-beta.1" } }, { "loader": { "version": "0.27.1" } }]));
        assert_eq!(q[0].version, "0.28.0-beta.1");
        assert!(!q[0].stable && q[1].stable && q[1].recommended);
        assert!(parse_fabric(&json!([])).is_empty());
    }

    #[test]
    fn forge_with_promotions() {
        let meta = json!({
            "1.20.1": ["1.20.1-47.2.0", "1.20.1-47.3.0", "1.20.1-47.10.1"],
            "1.7.10": ["1.7.10-10.13.4.1614-1.7.10"]
        });
        let promos = json!({ "promos": { "1.20.1-recommended": "47.3.0", "1.20.1-latest": "47.10.1" } });
        let list = parse_forge(&meta, Some(&promos), "1.20.1");
        assert_eq!(list.iter().map(|v| v.label.as_str()).collect::<Vec<_>>(), vec!["47.10.1", "47.3.0", "47.2.0"]);
        assert!(list[1].recommended && !list[0].recommended);
        assert_eq!(list[1].version, "1.20.1-47.3.0");
        let old = parse_forge(&meta, None, "1.7.10");
        assert_eq!(old[0].label, "10.13.4.1614");
        assert!(parse_forge(&meta, None, "1.99").is_empty());
    }

    #[test]
    fn neoforge_versions_map_to_minecraft() {
        assert_eq!(neoforge_minecraft("21.1.77").as_deref(), Some("1.21.1"));
        assert_eq!(neoforge_minecraft("21.0.5-beta").as_deref(), Some("1.21"));
        assert_eq!(neoforge_minecraft("20.4.237").as_deref(), Some("1.20.4"));
        assert_eq!(neoforge_minecraft("26.1.0.12").as_deref(), Some("26.1"));
        assert_eq!(neoforge_minecraft("26.1.1.3-beta").as_deref(), Some("26.1.1"));
        assert_eq!(neoforge_minecraft("0.25w14craftmine.3-beta"), None);

        let neo = json!({ "versions": ["21.1.76", "21.1.77", "21.0.5-beta", "21.1.78-beta"] });
        let list = parse_neoforge(Some(&neo), None, "1.21.1");
        assert_eq!(list.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(), vec!["21.1.78-beta", "21.1.77", "21.1.76"]);
        assert!(list[1].recommended && !list[0].stable);

        let legacy = json!({ "versions": ["1.20.1-47.1.5", "1.20.1-47.1.106"] });
        let l = parse_neoforge(Some(&neo), Some(&legacy), "1.20.1");
        assert_eq!(l[0].version, "1.20.1-47.1.106");
        assert_eq!(l[0].label, "47.1.106");
    }
}
