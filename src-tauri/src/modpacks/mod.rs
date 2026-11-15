//! Модпаки: импорт/экспорт `.mrpack` (Modrinth) и zip CurseForge, установка как
//! нового инстанса и обновление.
//!
//! Инстанс модпака хранит в `instance.json` ссылку на источник (`modpack`), а в
//! `instances/<id>/modpack.json` — список файлов, положенных из пака (путь → SHA1).
//! По этому списку при обновлении понятно, что заменить, что удалить и что
//! пользователь поменял сам (такие конфиги не трогаем).

pub mod apply;
pub mod export;
pub mod format;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::util::{read_json_opt, write_json_atomic};

pub const STATE_FILE: &str = "modpack.json";
pub const STAGING_DIR: &str = ".modpack-staging";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackSource {
    Modrinth,
    Curseforge,
    /// Импорт из файла — обновляется только из файла.
    File,
}

/// Откуда взят модпак инстанса (поле `modpack` в `instance.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackLink {
    pub source: PackSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    pub name: String,
    /// Версия пака (как её называет автор).
    #[serde(default)]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Файл пака, который нельзя скачать автоматически (CurseForge: автор запретил
/// сторонние лаунчеры). Пользователь качает его сам, лаунчер находит в «Загрузках».
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingFile {
    pub name: String,
    pub file_name: String,
    pub page_url: String,
    /// Куда положить (относительно игровой папки).
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ManagedState {
    /// Путь (через `/`, относительно игровой папки) → SHA1 положенного файла.
    pub files: BTreeMap<String, String>,
    pub missing: Vec<MissingFile>,
}

impl ManagedState {
    pub fn load(inst_dir: &Path) -> AppResult<Self> {
        Ok(read_json_opt(&inst_dir.join(STATE_FILE))?.unwrap_or_default())
    }

    pub fn save(&self, inst_dir: &Path) -> AppResult<()> {
        write_json_atomic(&inst_dir.join(STATE_FILE), self)
    }
}

/// Проверяет относительный путь из пака (защита от zip-slip): только обычные
/// компоненты, без `..`, корня, дисков и пустых частей. Возвращает путь с `/`.
pub fn safe_rel(path: &str) -> AppResult<String> {
    let bad = || AppError::Content(format!("недопустимый путь в модпаке: {path}"));
    let norm = path.replace('\\', "/");
    if norm.is_empty() || norm.starts_with('/') || norm.contains(':') || norm.contains('\0') {
        return Err(bad());
    }
    let mut parts = Vec::new();
    for part in norm.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.trim().is_empty() || part.ends_with(['.', ' ']) {
            return Err(bad());
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(bad());
    }
    let joined = parts.join("/");
    // Дополнительно — по правилам std::path текущей ОС.
    if !Path::new(&joined).components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(bad());
    }
    Ok(joined)
}

/// Полный путь в папке `base` для проверенного относительного пути.
pub fn join_rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, part| p.join(part))
}

/// Ищет файлы, скачанные вручную, в папке «Загрузки» (и в `extra`).
/// Совпадение — по имени и (если известны) размеру и SHA1.
pub fn find_downloaded(missing: &[MissingFile], extra: &[PathBuf]) -> Vec<(usize, PathBuf)> {
    let mut found = Vec::new();
    let dirs: Vec<PathBuf> = dirs::download_dir().into_iter().collect();
    for (i, m) in missing.iter().enumerate() {
        let candidates = extra
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy() == m.file_name))
            .cloned()
            .chain(dirs.iter().map(|d| d.join(&m.file_name)));
        for c in candidates {
            if matches_file(&c, m) {
                found.push((i, c));
                break;
            }
        }
    }
    found
}

fn matches_file(path: &Path, m: &MissingFile) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    if !meta.is_file() || (m.size > 0 && meta.len() != m.size) {
        return false;
    }
    match &m.sha1 {
        Some(expected) => apply::sha1_path(path).is_ok_and(|h| &h == expected),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_paths() {
        assert_eq!(safe_rel("mods/a.jar").unwrap(), "mods/a.jar");
        assert_eq!(safe_rel("config\\x\\y.toml").unwrap(), "config/x/y.toml");
        assert_eq!(safe_rel("./config//a").unwrap(), "config/a");
        for bad in ["../x", "mods/../../x", "/etc/passwd", "C:/Windows/x", "c:x", "", "a/ ", "a/b.", "..", "mods/..\\x"] {
            assert!(safe_rel(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn finds_manual_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("blocked.jar");
        std::fs::write(&f, b"data").unwrap();
        let m = MissingFile {
            name: "B".into(),
            file_name: "blocked.jar".into(),
            page_url: "https://x".into(),
            path: "mods/blocked.jar".into(),
            sha1: Some(sha1_smol::Sha1::from(b"data").digest().to_string()),
            size: 4,
        };
        let wrong = MissingFile { sha1: Some("00".into()), ..m.clone() };
        let found = find_downloaded(&[wrong, m], std::slice::from_ref(&f));
        assert_eq!(found, vec![(1, f)]);
    }
}
