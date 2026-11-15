//! Подготовка файлов перед запуском: быстрая проверка наличия, распаковка
//! natives, раскладка assets для старых версий.

use std::io::Read;
use std::path::{Path, PathBuf};

use super::assets::AssetIndex;
use super::libraries::ResolvedLibrary;
use super::version::VersionJson;
use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;
use crate::util::read_json_opt;

const NATIVE_EXTS: [&str; 4] = ["dll", "so", "dylib", "jnilib"];

/// Быстрая проверка (только наличие файлов, без хэшей): можно ли запускать без установки.
pub fn files_present(paths: &AppPaths, version: &VersionJson, libs: &[ResolvedLibrary], client_jar: &Path) -> bool {
    if version.downloads.contains_key("client") && !client_jar.is_file() {
        return false;
    }
    let lib_dir = paths.libraries_dir();
    if libs.iter().any(|l| !lib_dir.join(&l.path).is_file()) {
        return false;
    }
    if let Some(log) = version.logging.as_ref().and_then(|l| l.client.as_ref()) {
        if !paths.assets_dir().join("log_configs").join(&log.file.id).is_file() {
            return false;
        }
    }
    match load_asset_index(paths, version) {
        Ok(Some(index)) => {
            let objects = paths.assets_dir().join("objects");
            index.objects.values().all(|o| objects.join(o.rel_path()).is_file())
        }
        Ok(None) => version.asset_index.is_none(),
        Err(_) => false,
    }
}

pub fn load_asset_index(paths: &AppPaths, version: &VersionJson) -> AppResult<Option<AssetIndex>> {
    match &version.asset_index {
        Some(ai) => read_json_opt(&paths.assets_dir().join("indexes").join(format!("{}.json", ai.id))),
        None => Ok(None),
    }
}

/// Папка для natives инстанса.
///
/// LWJGL 2 грузит часть библиотек (OpenAL) через ANSI-функции Windows, которые
/// не понимают не-ASCII пути (кириллица в имени пользователя или инстанса).
/// Поэтому на Windows для таких путей берём короткое имя 8.3, а если и оно не
/// ASCII — общую папку в `%ProgramData%`.
pub fn natives_dir(inst_dir: &Path, inst_id: &str) -> PathBuf {
    let preferred = inst_dir.join("natives");
    if !cfg!(windows) || preferred.to_string_lossy().is_ascii() {
        return preferred;
    }
    if let Some(short) = short_path(inst_dir).filter(|p| p.to_string_lossy().is_ascii()) {
        return short.join("natives");
    }
    let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let digest = md5::compute(inst_dir.to_string_lossy().as_bytes());
    let ascii_id: String = inst_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect();
    base.join("Cubelaunch").join("natives").join(format!("{ascii_id}-{:x}", digest).chars().take(40).collect::<String>())
}

#[cfg(windows)]
fn short_path(p: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    // SAFETY: буферы корректной длины, строка завершена нулём.
    let n = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..n])))
}

#[cfg(not(windows))]
fn short_path(_: &Path) -> Option<PathBuf> {
    None
}

/// Распаковывает natives в чистую папку.
/// - Старые natives (не в classpath): всё содержимое, кроме `extract.exclude`.
/// - Новые (LWJGL 3.3+): только сами библиотеки, «сплющенные» в корень папки.
pub fn extract_natives(libs: &[ResolvedLibrary], libraries_dir: &Path, natives_dir: &Path) -> AppResult<()> {
    if natives_dir.exists() {
        std::fs::remove_dir_all(natives_dir)?;
    }
    std::fs::create_dir_all(natives_dir)?;
    for lib in libs.iter().filter(|l| l.native) {
        let jar = libraries_dir.join(&lib.path);
        let file = std::fs::File::open(&jar).map_err(|e| AppError::Launch(format!("{}: {e}", jar.display())))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| AppError::Launch(format!("natives {} повреждены: {e}", jar.display())))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| AppError::Launch(e.to_string()))?;
            if entry.is_dir() {
                continue;
            }
            let Some(rel) = entry.enclosed_name() else { continue };
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.starts_with("META-INF/") || lib.extract_exclude.iter().any(|ex| rel_str.starts_with(ex)) {
                continue;
            }
            let out: PathBuf = if lib.classpath {
                let is_native = rel
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| NATIVE_EXTS.contains(&e.to_ascii_lowercase().as_str()));
                if !is_native {
                    continue;
                }
                natives_dir.join(rel.file_name().unwrap())
            } else {
                natives_dir.join(&rel)
            };
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf)?;
            std::fs::write(&out, buf)?;
        }
    }
    Ok(())
}

/// Для старых версий раскладывает объекты assets по именам. Возвращает путь
/// для плейсхолдера `${game_assets}`.
pub fn prepare_legacy_assets(paths: &AppPaths, version: &VersionJson, game_dir: &Path) -> AppResult<PathBuf> {
    let assets = paths.assets_dir();
    let Some(index) = load_asset_index(paths, version)? else { return Ok(assets) };
    let target = if index.map_to_resources {
        game_dir.join("resources")
    } else if index.is_virtual {
        assets.join("virtual").join(version.assets_id())
    } else {
        return Ok(assets);
    };
    let objects = assets.join("objects");
    for (name, obj) in &index.objects {
        let rel = Path::new(name);
        if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            continue; // не даём выйти за пределы папки
        }
        let dest = target.join(rel);
        if dest.metadata().is_ok_and(|m| m.len() == obj.size) {
            continue;
        }
        if let Some(p) = dest.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(objects.join(obj.rel_path()), &dest)?;
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_jar(path: &Path, files: &[(&str, &[u8])]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    fn lib(path: &str, classpath: bool, exclude: &[&str]) -> ResolvedLibrary {
        ResolvedLibrary {
            name: path.into(),
            path: PathBuf::from(path),
            url: None,
            sha1: None,
            size: None,
            native: true,
            classpath,
            extract_exclude: exclude.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn natives_dir_is_ascii() {
        let dir = tempfile::tempdir().unwrap();
        let inst = dir.path().join("тест");
        std::fs::create_dir_all(&inst).unwrap();
        let n = natives_dir(&inst, "тест");
        if cfg!(windows) {
            assert!(n.to_string_lossy().is_ascii(), "{}", n.display());
        } else {
            assert_eq!(n, inst.join("natives"));
        }
        let ascii = dir.path().join("plain");
        assert_eq!(natives_dir(&ascii, "plain"), ascii.join("natives"));
    }

    #[test]
    fn extracts_legacy_and_modern_natives() {
        let dir = tempfile::tempdir().unwrap();
        let libs_dir = dir.path().join("libs");
        make_jar(
            &libs_dir.join("old.jar"),
            &[("lwjgl64.dll", b"a"), ("META-INF/MANIFEST.MF", b"m"), ("skip/me.txt", b"s")],
        );
        make_jar(
            &libs_dir.join("new.jar"),
            &[("windows/x64/org/lwjgl/lwjgl.dll", b"b"), ("windows/x64/org/lwjgl/lwjgl.dll.sha1", b"c")],
        );
        let natives = dir.path().join("natives");
        std::fs::create_dir_all(&natives).unwrap();
        std::fs::write(natives.join("stale.dll"), b"old").unwrap();

        extract_natives(&[lib("old.jar", false, &["skip/"]), lib("new.jar", true, &[])], &libs_dir, &natives).unwrap();

        let mut names: Vec<String> = std::fs::read_dir(&natives)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["lwjgl.dll", "lwjgl64.dll"]);
    }
}
