//! Применение файлов пака к игровой папке (установка и обновление).
//!
//! Новые файлы заранее лежат в staging-папке (скачаны и распакованы). Правила:
//! - файл из прошлой версии пака, которого нет в новой, удаляется, если пользователь
//!   его не менял (SHA1 совпадает с записанным); изменённый остаётся;
//! - скачиваемые файлы (моды, паки) всегда заменяются; если мод был выключен
//!   (`.disabled`), новая версия по тому же пути тоже выключена;
//! - файлы overrides (конфиги и пр.) пишутся, только если их нет или пользователь их
//!   не менял; свои правки пользователя сохраняются;
//! - файлы, которые пользователь добавил сам и которых нет в паке, не трогаются.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::join_rel;
use crate::content::DISABLED;
use crate::error::AppResult;

#[derive(Debug, Clone, PartialEq)]
pub struct NewEntry {
    pub rel: String,
    pub sha1: String,
    /// Из overrides (пользовательские правки сохраняются) или скачан (заменяется всегда).
    pub from_override: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct ApplyReport {
    /// Новое содержимое `modpack.json → files`.
    pub managed: BTreeMap<String, String>,
    pub written: usize,
    pub removed: usize,
    /// Изменённые пользователем файлы, которые оставлены как есть.
    pub kept: Vec<String>,
}

pub fn sha1_path(path: &Path) -> AppResult<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = sha1_smol::Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.digest().to_string())
}

fn sha1_opt(path: &Path) -> Option<String> {
    path.is_file().then(|| sha1_path(path).ok()).flatten()
}

fn disabled_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(DISABLED);
    PathBuf::from(s)
}

/// Перемещает файл (rename, при ошибке — копирование).
pub fn move_file(from: &Path, to: &Path) -> AppResult<()> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p)?;
    }
    if to.exists() {
        std::fs::remove_file(to)?;
    }
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to)?;
        let _ = std::fs::remove_file(from);
    }
    Ok(())
}

pub fn apply(game: &Path, staging: &Path, new: &[NewEntry], old: &BTreeMap<String, String>) -> AppResult<ApplyReport> {
    let mut report = ApplyReport::default();
    let new_paths: std::collections::HashSet<&str> = new.iter().map(|e| e.rel.as_str()).collect();

    // 1. Удаляем то, что ушло из пака (если пользователь не менял).
    for (rel, sha) in old {
        if new_paths.contains(rel.as_str()) {
            continue;
        }
        let path = join_rel(game, rel);
        for p in [path.clone(), disabled_path(&path)] {
            match sha1_opt(&p) {
                Some(cur) if &cur == sha => {
                    std::fs::remove_file(&p)?;
                    report.removed += 1;
                }
                Some(_) => report.kept.push(rel.clone()),
                None => {}
            }
        }
    }

    // 2. Кладём новые файлы.
    for e in new {
        let dest = join_rel(game, &e.rel);
        let src = join_rel(staging, &e.rel);
        let disabled = disabled_path(&dest);
        if e.from_override {
            let current = sha1_opt(&dest);
            let write = match &current {
                None => true,
                Some(cur) if cur == &e.sha1 => false,
                // Не менялся с прошлой установки — обновляем.
                Some(cur) => old.get(&e.rel) == Some(cur),
            };
            if write {
                move_file(&src, &dest)?;
                report.written += 1;
            } else if current.as_ref() != Some(&e.sha1) {
                report.kept.push(e.rel.clone());
            }
        } else {
            let was_disabled = !dest.exists() && disabled.exists();
            let target = if was_disabled { &disabled } else { &dest };
            if sha1_opt(target).as_ref() != Some(&e.sha1) {
                move_file(&src, target)?;
                report.written += 1;
            }
        }
        report.managed.insert(e.rel.clone(), e.sha1.clone());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(data: &[u8]) -> String {
        sha1_smol::Sha1::from(data).digest().to_string()
    }

    fn stage(staging: &Path, entries: &[(&str, &[u8], bool)]) -> Vec<NewEntry> {
        let _ = std::fs::remove_dir_all(staging);
        entries
            .iter()
            .map(|(rel, data, ov)| {
                let p = join_rel(staging, rel);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, data).unwrap();
                NewEntry { rel: rel.to_string(), sha1: sha(data), from_override: *ov }
            })
            .collect()
    }

    #[test]
    fn install_then_update_keeps_user_changes() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        let staging = dir.path().join("staging");

        // Установка v1.
        let v1 = stage(&staging, &[
            ("mods/a-1.jar", b"a1", false),
            ("mods/b-1.jar", b"b1", false),
            ("mods/gone.jar", b"gone", false),
            ("config/a.toml", b"a=1", true),
            ("config/b.toml", b"b=1", true),
            ("config/old.toml", b"old", true),
        ]);
        let r1 = apply(&game, &staging, &v1, &BTreeMap::new()).unwrap();
        assert_eq!(r1.written, 6);
        assert_eq!(r1.managed.len(), 6);

        // Пользователь: выключил мод b, поменял config/b.toml, добавил свой мод, поменял old.toml.
        std::fs::rename(game.join("mods/b-1.jar"), game.join("mods/b-1.jar.disabled")).unwrap();
        std::fs::write(game.join("config/b.toml"), b"b=mine").unwrap();
        std::fs::write(game.join("config/old.toml"), b"old-mine").unwrap();
        std::fs::write(game.join("mods/mine.jar"), b"mine").unwrap();

        // Обновление v2: a обновился (новое имя), b тот же путь, но новый файл, gone и old.toml убраны.
        let v2 = stage(&staging, &[
            ("mods/a-2.jar", b"a2", false),
            ("mods/b-1.jar", b"b1-fixed", false),
            ("config/a.toml", b"a=2", true),
            ("config/b.toml", b"b=2", true),
        ]);
        let r2 = apply(&game, &staging, &v2, &r1.managed).unwrap();

        assert!(!game.join("mods/a-1.jar").exists());
        assert!(!game.join("mods/gone.jar").exists());
        assert_eq!(std::fs::read(game.join("mods/a-2.jar")).unwrap(), b"a2");
        // Выключенный мод обновлён и остался выключенным.
        assert!(!game.join("mods/b-1.jar").exists());
        assert_eq!(std::fs::read(game.join("mods/b-1.jar.disabled")).unwrap(), b"b1-fixed");
        // Нетронутый конфиг обновлён, изменённый — сохранён.
        assert_eq!(std::fs::read(game.join("config/a.toml")).unwrap(), b"a=2");
        assert_eq!(std::fs::read(game.join("config/b.toml")).unwrap(), b"b=mine");
        // Изменённый файл, убранный из пака, остаётся.
        assert_eq!(std::fs::read(game.join("config/old.toml")).unwrap(), b"old-mine");
        assert!(game.join("mods/mine.jar").exists());
        assert_eq!(r2.removed, 2);
        assert_eq!(r2.kept, vec!["config/old.toml".to_string(), "config/b.toml".to_string()]);
        assert_eq!(r2.managed.len(), 4);

        // Повторное применение той же версии ничего не переписывает.
        let v2again = stage(&staging, &[("mods/a-2.jar", b"a2", false), ("config/a.toml", b"a=2", true)]);
        let r3 = apply(&game, &staging, &v2again, &r2.managed).unwrap();
        assert_eq!(r3.written, 0);
    }

    #[test]
    fn existing_untracked_config_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("options.txt"), b"mine").unwrap();
        let v = stage(&staging, &[("options.txt", b"pack", true)]);
        let r = apply(&game, &staging, &v, &BTreeMap::new()).unwrap();
        assert_eq!(std::fs::read(game.join("options.txt")).unwrap(), b"mine");
        assert_eq!(r.kept, vec!["options.txt".to_string()]);
    }
}
