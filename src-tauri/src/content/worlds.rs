//! Миры инстанса (`.minecraft/saves/<папка>`): список, резервные копии, удаление.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::Serialize;

use super::nbt::{self, Tag};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct World {
    /// Имя папки (идентификатор).
    pub folder: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played: Option<DateTime<Utc>>,
    /// `survival` | `creative` | `adventure` | `spectator`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_mode: Option<String>,
    pub hardcore: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub file_name: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,
}

pub fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

fn game_mode(v: i64) -> &'static str {
    match v {
        1 => "creative",
        2 => "adventure",
        3 => "spectator",
        _ => "survival",
    }
}

pub fn read_world(dir: &Path) -> Option<World> {
    let level = dir.join("level.dat");
    if !level.is_file() {
        return None;
    }
    let folder = dir.file_name()?.to_string_lossy().to_string();
    let data = nbt::read_file(&level);
    let d = data.as_ref().and_then(|t| t.get("Data"));
    let icon = std::fs::read(dir.join("icon.png"))
        .ok()
        .filter(|b| b.len() < 256 * 1024 && b.starts_with(b"\x89PNG"))
        .map(|b| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b)));
    Some(World {
        name: d.and_then(|d| d.get("LevelName")).and_then(Tag::as_str).filter(|n| !n.is_empty()).unwrap_or(&folder).to_string(),
        last_played: d
            .and_then(|d| d.get("LastPlayed"))
            .and_then(Tag::as_i64)
            .and_then(DateTime::from_timestamp_millis),
        game_mode: d.and_then(|d| d.get("GameType")).and_then(Tag::as_i64).map(|g| game_mode(g).to_string()),
        hardcore: d.and_then(|d| d.get("hardcore")).and_then(Tag::as_i64).is_some_and(|h| h != 0),
        version: d.and_then(|d| d.path(&["Version", "Name"])).and_then(Tag::as_str).map(str::to_string),
        icon,
        size: dir_size(dir),
        folder,
    })
}

pub fn list_worlds(saves: &Path) -> Vec<World> {
    let mut out: Vec<World> = std::fs::read_dir(saves)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| read_world(&e.path()))
        .collect();
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played).then_with(|| a.name.cmp(&b.name)));
    out
}

pub fn list_backups(backups: &Path) -> Vec<Backup> {
    let mut out: Vec<Backup> = std::fs::read_dir(backups)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".zip"))
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            Some(Backup {
                file_name: e.file_name().to_string_lossy().to_string(),
                size: md.len(),
                created: md.modified().ok().map(DateTime::<Utc>::from),
            })
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.created));
    out
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        if e.file_type()?.is_dir() {
            collect_files(root, &p, out)?;
        } else if e.file_name() != "session.lock" {
            // session.lock занят запущенной игрой и в копии не нужен.
            let rel = p.strip_prefix(root).unwrap_or(&p).components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            out.push((p, rel));
        }
    }
    Ok(())
}

/// Архивирует мир в `backups/<папка>-<дата>.zip`. `progress(done, total)`.
pub fn backup_world(
    world_dir: &Path,
    backups: &Path,
    cancel: &tokio_util::sync::CancellationToken,
    progress: impl Fn(usize, usize),
) -> AppResult<PathBuf> {
    let folder = world_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut files = Vec::new();
    collect_files(world_dir, world_dir, &mut files)?;
    std::fs::create_dir_all(backups)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let target = backups.join(format!("{folder}-{stamp}.zip"));
    let tmp = target.with_extension("zip.part");
    let result = (|| -> AppResult<()> {
        let mut zip = zip::ZipWriter::new(File::create(&tmp)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let total = files.len();
        for (i, (path, rel)) in files.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            zip.start_file(format!("{folder}/{rel}"), opts).map_err(|e| AppError::Other(e.to_string()))?;
            let mut f = match File::open(path) {
                Ok(f) => f,
                // Файл могла удалить работающая игра — пропускаем.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            std::io::copy(&mut f, &mut zip)?;
            progress(i + 1, total);
        }
        zip.finish().map_err(|e| AppError::Other(e.to_string()))?.flush()?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_backs_up() {
        let d = tempfile::tempdir().unwrap();
        let saves = d.path().join("saves");
        let w = saves.join("New World");
        std::fs::create_dir_all(w.join("region")).unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&nbt::tests::level_dat("Мир", 1_700_000_000_000, 0, true)).unwrap();
        std::fs::write(w.join("level.dat"), gz.finish().unwrap()).unwrap();
        std::fs::write(w.join("region/r.0.0.mca"), vec![7u8; 1000]).unwrap();
        std::fs::write(w.join("session.lock"), "x").unwrap();
        std::fs::create_dir_all(saves.join("not a world")).unwrap();

        let list = list_worlds(&saves);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Мир");
        assert_eq!(list[0].folder, "New World");
        assert_eq!(list[0].game_mode.as_deref(), Some("survival"));
        assert!(list[0].hardcore);
        assert_eq!(list[0].version.as_deref(), Some("1.21.1"));
        assert!(list[0].size > 1000);

        let backups = d.path().join("backups");
        let cancel = tokio_util::sync::CancellationToken::new();
        let zip_path = backup_world(&w, &backups, &cancel, |_, _| {}).unwrap();
        let mut z = zip::ZipArchive::new(File::open(&zip_path).unwrap()).unwrap();
        let names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
        assert!(names.contains(&"New World/level.dat".to_string()));
        assert!(names.contains(&"New World/region/r.0.0.mca".to_string()));
        assert!(!names.iter().any(|n| n.ends_with("session.lock")));
        assert_eq!(list_backups(&backups).len(), 1);
    }
}
