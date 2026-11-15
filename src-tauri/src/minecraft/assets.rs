//! Индекс assets и задачи на скачивание объектов.
//!
//! Объекты хранятся общими для всех инстансов: `assets/objects/<2 символа хэша>/<хэш>`.
//! Для старых версий (`virtual` / `map_to_resources`) при запуске (этап 3) объекты
//! дополнительно раскладываются по именам.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::downloads::queue::DownloadTask;

pub const RESOURCES_URL: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    #[serde(default)]
    pub objects: HashMap<String, AssetObject>,
    #[serde(rename = "virtual", default)]
    pub is_virtual: bool,
    #[serde(default)]
    pub map_to_resources: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    pub fn rel_path(&self) -> String {
        format!("{}/{}", &self.hash[..2], self.hash)
    }
}

pub fn asset_tasks(index: &AssetIndex, assets_dir: &Path) -> Vec<DownloadTask> {
    let objects = assets_dir.join("objects");
    let mut seen = std::collections::HashSet::new();
    index
        .objects
        .values()
        .filter(|o| o.hash.len() == 40 && seen.insert(o.hash.clone()))
        .map(|o| DownloadTask {
            url: format!("{RESOURCES_URL}/{}", o.rel_path()),
            dest: objects.join(&o.hash[..2]).join(&o.hash),
            sha1: Some(o.hash.clone()),
            size: Some(o.size),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_deduplicated_tasks() {
        let idx: AssetIndex = serde_json::from_str(
            r#"{ "objects": {
                "minecraft/sounds/a.ogg": { "hash": "0123456789abcdef0123456789abcdef01234567", "size": 10 },
                "minecraft/sounds/b.ogg": { "hash": "0123456789abcdef0123456789abcdef01234567", "size": 10 },
                "icons/icon.png": { "hash": "ffeeddccbbaa99887766554433221100ffeeddcc", "size": 5 }
            } }"#,
        )
        .unwrap();
        let tasks = asset_tasks(&idx, Path::new("/data/assets"));
        assert_eq!(tasks.len(), 2);
        let t = tasks.iter().find(|t| t.size == Some(5)).unwrap();
        assert_eq!(
            t.url,
            "https://resources.download.minecraft.net/ff/ffeeddccbbaa99887766554433221100ffeeddcc"
        );
        assert!(t.dest.ends_with("objects/ff/ffeeddccbbaa99887766554433221100ffeeddcc"));
    }
}
