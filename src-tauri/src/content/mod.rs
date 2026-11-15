//! Содержимое инстанса: моды, ресурспаки, шейдеры (файлы в игровой папке),
//! миры и интеграция с Modrinth.
//!
//! Отключение — как у других лаунчеров: к имени файла добавляется `.disabled`,
//! игра такие файлы не загружает. Связь файла с проектом Modrinth не хранится
//! отдельно, а определяется по SHA1 файла через API Modrinth — поэтому
//! распознаются и моды, добавленные вручную.

pub mod curseforge;
pub mod meta;
pub mod modrinth;
pub mod nbt;
pub mod worlds;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use meta::ContentMeta;

pub const DISABLED: &str = ".disabled";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Mods,
    Resourcepacks,
    Shaderpacks,
}

impl ContentKind {
    pub fn folder(&self) -> &'static str {
        match self {
            ContentKind::Mods => "mods",
            ContentKind::Resourcepacks => "resourcepacks",
            ContentKind::Shaderpacks => "shaderpacks",
        }
    }

    /// `project_type` в Modrinth.
    pub fn modrinth_type(&self) -> &'static str {
        match self {
            ContentKind::Mods => "mod",
            ContentKind::Resourcepacks => "resourcepack",
            ContentKind::Shaderpacks => "shader",
        }
    }

    /// Подходит ли файл/папка для этой категории (имя без `.disabled`).
    pub fn accepts(&self, name: &str, is_dir: bool) -> bool {
        let lower = name.to_lowercase();
        match self {
            ContentKind::Mods => !is_dir && (lower.ends_with(".jar") || lower.ends_with(".zip") || lower.ends_with(".litemod")),
            _ => is_dir || lower.ends_with(".zip"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentFile {
    /// Имя на диске (с `.disabled`, если выключен).
    pub file_name: String,
    pub enabled: bool,
    pub is_dir: bool,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ContentMeta>,
    /// Мод собран под другой загрузчик (или в инстансе загрузчика нет).
    pub incompatible: bool,
}

#[derive(Clone)]
struct Cached {
    len: u64,
    modified: Option<SystemTime>,
    sha1: Option<String>,
    meta: Option<ContentMeta>,
}

/// Кэш разбора файлов: повторное открытие вкладки не читает сотни jar заново.
#[derive(Default)]
pub struct ContentCache(Mutex<HashMap<PathBuf, Cached>>);

fn sha1_of(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = sha1_smol::Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.digest().to_string())
}

/// Имя без суффикса `.disabled`.
pub fn base_name(file_name: &str) -> &str {
    file_name.strip_suffix(DISABLED).unwrap_or(file_name)
}

/// Перечисляет содержимое папки категории (блокирующая операция).
pub fn list(dir: &Path, kind: ContentKind, cache: &ContentCache) -> AppResult<Vec<ContentFile>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let Ok(md) = entry.metadata() else { continue };
        let is_dir = md.is_dir();
        let enabled = !file_name.ends_with(DISABLED);
        if file_name.starts_with('.') || !kind.accepts(base_name(&file_name), is_dir) {
            continue;
        }
        let path = entry.path();
        let modified = md.modified().ok();
        let len = if is_dir { 0 } else { md.len() };
        let hit = cache
            .0
            .lock()
            .unwrap()
            .get(&path)
            .filter(|c| c.len == len && c.modified == modified)
            .cloned();
        let info = match hit {
            Some(c) => c,
            None => {
                let meta = match kind {
                    ContentKind::Mods => meta::read_mod(&path),
                    _ => meta::read_pack(&path),
                };
                let c = Cached { len, modified, sha1: if is_dir { None } else { sha1_of(&path) }, meta };
                cache.0.lock().unwrap().insert(path.clone(), c.clone());
                c
            }
        };
        out.push(ContentFile {
            file_name,
            enabled,
            is_dir,
            size: len,
            modified: modified.map(DateTime::<Utc>::from),
            sha1: info.sha1,
            meta: info.meta,
            incompatible: false,
        });
    }
    out.sort_by_cached_key(|f| {
        f.meta.as_ref().and_then(|m| m.name.clone()).unwrap_or_else(|| base_name(&f.file_name).to_string()).to_lowercase()
    });
    Ok(out)
}

/// Путь к элементу папки по имени из UI (без выхода за пределы папки).
pub fn entry_path(dir: &Path, file_name: &str) -> AppResult<PathBuf> {
    if file_name.is_empty() || file_name.contains(['/', '\\']) || file_name == "." || file_name == ".." {
        return Err(AppError::InvalidInput(format!("некорректное имя файла: {file_name}")));
    }
    let p = dir.join(file_name);
    if !p.exists() {
        return Err(AppError::NotFound(file_name.to_string()));
    }
    Ok(p)
}

/// Включает/выключает элемент. Возвращает новое имя.
pub fn set_enabled(dir: &Path, file_name: &str, enabled: bool) -> AppResult<String> {
    let from = entry_path(dir, file_name)?;
    let is_enabled = !file_name.ends_with(DISABLED);
    if is_enabled == enabled {
        return Ok(file_name.to_string());
    }
    let new_name = if enabled { base_name(file_name).to_string() } else { format!("{file_name}{DISABLED}") };
    let to = dir.join(&new_name);
    if to.exists() {
        return Err(AppError::InvalidInput(format!("файл {new_name} уже существует")));
    }
    std::fs::rename(from, to)?;
    Ok(new_name)
}

/// Удаляет элементы в корзину ОС.
pub fn delete(dir: &Path, names: &[String]) -> AppResult<()> {
    for n in names {
        let p = entry_path(dir, n)?;
        trash::delete(&p).map_err(|e| AppError::Other(format!("не удалось удалить {n}: {e}")))?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddResult {
    pub added: Vec<String>,
    /// Файлы, не подходящие к категории.
    pub skipped: Vec<String>,
}

/// Копирует файлы (перетащенные или выбранные в диалоге) в папку категории.
pub fn add_files(dir: &Path, kind: ContentKind, sources: &[PathBuf]) -> AppResult<AddResult> {
    std::fs::create_dir_all(dir)?;
    let mut res = AddResult::default();
    for src in sources {
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        let is_dir = src.is_dir();
        if !src.exists() || !kind.accepts(base_name(&name), is_dir) {
            res.skipped.push(name);
            continue;
        }
        let target = dir.join(&name);
        if src.canonicalize().ok() == target.canonicalize().ok() && target.exists() {
            continue; // перетащили файл из этой же папки
        }
        if is_dir {
            copy_dir(src, &target)?;
        } else {
            std::fs::copy(src, &target)?;
        }
        res.added.push(name);
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_toggle_delete_add() {
        let d = tempfile::tempdir().unwrap();
        let mods = d.path().join("mods");
        std::fs::create_dir(&mods).unwrap();
        meta::tests_support::zip(&mods.join("b.jar"), &[("fabric.mod.json", br#"{"id":"b","name":"Bravo"}"#)]);
        meta::tests_support::zip(&mods.join("a.jar.disabled"), &[("x", b"1")]);
        std::fs::write(mods.join("readme.txt"), "x").unwrap();
        let cache = ContentCache::default();

        let l = list(&mods, ContentKind::Mods, &cache).unwrap();
        assert_eq!(l.iter().map(|f| f.file_name.as_str()).collect::<Vec<_>>(), vec!["a.jar.disabled", "b.jar"]);
        assert!(!l[0].enabled && l[1].enabled);
        assert_eq!(l[1].meta.as_ref().unwrap().name.as_deref(), Some("Bravo"));
        assert_eq!(l[1].sha1.as_ref().unwrap().len(), 40);

        assert_eq!(set_enabled(&mods, "a.jar.disabled", true).unwrap(), "a.jar");
        assert_eq!(set_enabled(&mods, "b.jar", false).unwrap(), "b.jar.disabled");
        assert!(mods.join("b.jar.disabled").exists());
        assert!(set_enabled(&mods, "../x", true).is_err());

        let ext = d.path().join("ext");
        std::fs::create_dir(&ext).unwrap();
        std::fs::write(ext.join("new.jar"), "j").unwrap();
        std::fs::write(ext.join("pic.png"), "p").unwrap();
        let r = add_files(&mods, ContentKind::Mods, &[ext.join("new.jar"), ext.join("pic.png")]).unwrap();
        assert_eq!(r.added, vec!["new.jar"]);
        assert_eq!(r.skipped, vec!["pic.png"]);

        // Ресурспак-папка принимается, мод-папка — нет.
        assert!(ContentKind::Resourcepacks.accepts("Pack", true));
        assert!(!ContentKind::Mods.accepts("Pack", true));
    }
}
