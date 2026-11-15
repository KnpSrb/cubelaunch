//! Экспорт инстанса в `.mrpack` или zip CurseForge. Файлы, найденные на Modrinth
//! (или CurseForge), записываются ссылками; остальное идёт в `overrides/`.
//! Файлы игры (клиент, библиотеки, assets) в архив не попадают.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::json;

use crate::content::DISABLED;
use crate::error::{AppError, AppResult};
use crate::instances::{LoaderKind, LoaderSpec};
use crate::loaders::short_version;

/// Папки и файлы игровой папки, которые по умолчанию включаются в экспорт.
pub const DEFAULT_INCLUDE: [&str; 10] =
    ["mods", "config", "resourcepacks", "shaderpacks", "kubejs", "defaultconfigs", "scripts", "global_packs", "options.txt", "servers.dat"];
/// Никогда не экспортируются (временное, логи, личное).
const NEVER: [&str; 8] = ["logs", "crash-reports", ".fabric", ".mixin.out", "natives", "screenshots", "usercache.json", "usernamecache.json"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCandidate {
    pub name: String,
    pub is_dir: bool,
    pub default: bool,
}

/// Верхний уровень игровой папки — что можно включить в экспорт.
pub fn candidates(game: &Path) -> Vec<ExportCandidate> {
    let mut out: Vec<ExportCandidate> = std::fs::read_dir(game)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if NEVER.contains(&name.as_str()) {
                return None;
            }
            Some(ExportCandidate { default: DEFAULT_INCLUDE.contains(&name.as_str()), is_dir: e.path().is_dir(), name })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportFile {
    /// Относительный путь через `/`.
    pub rel: String,
    pub abs: PathBuf,
}

/// Файлы выбранных записей верхнего уровня (рекурсивно). Выключенные моды
/// (`.disabled`) пропускаются.
pub fn collect(game: &Path, include: &[String]) -> AppResult<Vec<ExportFile>> {
    let mut out = Vec::new();
    for name in include {
        if name.contains(['/', '\\']) || name == ".." || name == "." || name.is_empty() || NEVER.contains(&name.as_str()) {
            return Err(AppError::InvalidInput(format!("некорректная папка для экспорта: {name}")));
        }
        walk(&game.join(name), name, &mut out, false)?;
    }
    Ok(out)
}

pub fn walk(path: &Path, rel: &str, out: &mut Vec<ExportFile>, keep_disabled: bool) -> AppResult<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if meta.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            walk(&e.path(), &format!("{rel}/{name}"), out, keep_disabled)?;
        }
    } else if meta.is_file() && (keep_disabled || !rel.ends_with(DISABLED)) {
        out.push(ExportFile { rel: rel.to_string(), abs: path.to_path_buf() });
    }
    Ok(())
}

/// Можно ли файл заменить ссылкой (лежит прямо в папке модов/паков).
pub fn is_linkable(rel: &str) -> bool {
    let mut parts = rel.split('/');
    matches!((parts.next(), parts.next(), parts.next()), (Some("mods" | "resourcepacks" | "shaderpacks"), Some(_), None))
}

#[derive(Debug, Clone)]
pub struct ExportMeta {
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub minecraft: String,
    pub loader: Option<LoaderSpec>,
}

/// Файл, найденный на Modrinth.
#[derive(Debug, Clone)]
pub struct MrRemote {
    pub rel: String,
    pub url: String,
    pub sha1: String,
    pub sha512: String,
    pub size: u64,
}

fn loader_version(meta: &ExportMeta, l: &LoaderSpec) -> String {
    match l.kind {
        LoaderKind::Forge | LoaderKind::NeoForge => short_version(&meta.minecraft, &l.version),
        _ => l.version.clone(),
    }
}

pub fn mrpack_index(meta: &ExportMeta, remote: &[MrRemote]) -> serde_json::Value {
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), json!(meta.minecraft));
    if let Some(l) = &meta.loader {
        let key = match l.kind {
            LoaderKind::Fabric => "fabric-loader",
            LoaderKind::Quilt => "quilt-loader",
            LoaderKind::Forge => "forge",
            LoaderKind::NeoForge => "neoforge",
        };
        deps.insert(key.into(), json!(loader_version(meta, l)));
    }
    let files: Vec<_> = remote
        .iter()
        .map(|r| {
            let server = if r.rel.starts_with("mods/") { "required" } else { "unsupported" };
            json!({
                "path": r.rel,
                "hashes": { "sha1": r.sha1, "sha512": r.sha512 },
                "env": { "client": "required", "server": server },
                "downloads": [r.url],
                "fileSize": r.size,
            })
        })
        .collect();
    let mut idx = json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": meta.version,
        "name": meta.name,
        "files": files,
        "dependencies": deps,
    });
    if let Some(s) = meta.summary.as_ref().filter(|s| !s.trim().is_empty()) {
        idx["summary"] = json!(s);
    }
    idx
}

pub fn curseforge_manifest(meta: &ExportMeta, files: &[(u64, u64)]) -> serde_json::Value {
    let loaders: Vec<_> = meta
        .loader
        .iter()
        .map(|l| json!({ "id": format!("{}-{}", l.kind.as_str(), loader_version(meta, l)), "primary": true }))
        .collect();
    json!({
        "minecraft": { "version": meta.minecraft, "modLoaders": loaders },
        "manifestType": "minecraftModpack",
        "manifestVersion": 1,
        "name": meta.name,
        "version": meta.version,
        "author": "",
        "files": files.iter().map(|(p, f)| json!({ "projectID": p, "fileID": f, "required": true })).collect::<Vec<_>>(),
        "overrides": "overrides",
    })
}

/// Пишет архив: `index_name` с описанием и overrides. Сначала во временный файл.
pub fn write_archive(
    out: &Path,
    index_name: &str,
    index: &serde_json::Value,
    overrides: &[ExportFile],
    cancel: &tokio_util::sync::CancellationToken,
    on_progress: impl Fn(usize, usize),
) -> AppResult<()> {
    let tmp = out.with_extension("part");
    let result = (|| -> AppResult<()> {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let zerr = |e: zip::result::ZipError| AppError::Other(format!("ошибка записи архива: {e}"));
        z.start_file(index_name, opts).map_err(zerr)?;
        z.write_all(&serde_json::to_vec_pretty(index)?)?;
        for (i, f) in overrides.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            on_progress(i, overrides.len());
            z.start_file(format!("overrides/{}", f.rel), opts).map_err(zerr)?;
            std::io::copy(&mut std::fs::File::open(&f.abs)?, &mut z)?;
        }
        z.finish().map_err(zerr)?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, out)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modpacks::format::{parse_curseforge, parse_mrpack, read_index};

    fn meta(loader: Option<LoaderSpec>) -> ExportMeta {
        ExportMeta { name: "Мой пак".into(), version: "1.0".into(), summary: None, minecraft: "1.20.1".into(), loader }
    }

    #[test]
    fn index_roundtrips_through_parser() {
        let forge = LoaderSpec { kind: LoaderKind::Forge, version: "1.20.1-47.3.0".into() };
        let remote = vec![MrRemote { rel: "mods/a.jar".into(), url: "https://cdn.modrinth.com/a.jar".into(), sha1: "aa".into(), sha512: "bb".into(), size: 3 }];
        let idx = mrpack_index(&meta(Some(forge.clone())), &remote);
        let parsed = parse_mrpack(&serde_json::to_vec(&idx).unwrap()).unwrap();
        assert_eq!(parsed.loader, Some((LoaderKind::Forge, "47.3.0".into())));
        assert_eq!(parsed.files.len(), 1);

        let cf = curseforge_manifest(&meta(Some(forge)), &[(1, 2)]);
        let parsed = parse_curseforge(&serde_json::to_vec(&cf).unwrap()).unwrap();
        assert_eq!(parsed.loader, Some((LoaderKind::Forge, "47.3.0".into())));
        assert_eq!(parsed.cf_files.len(), 1);
    }

    #[test]
    fn collects_and_writes_archive() {
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        for (p, d) in [("mods/a.jar", "a"), ("mods/off.jar.disabled", "x"), ("config/x/y.toml", "y"), ("logs/latest.log", "l"), ("options.txt", "o")] {
            let f = game.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, d).unwrap();
        }
        let c = candidates(&game);
        assert!(c.iter().all(|c| c.name != "logs"));
        assert!(c.iter().find(|c| c.name == "config").unwrap().default);

        let files = collect(&game, &["mods".into(), "config".into(), "options.txt".into()]).unwrap();
        let rels: Vec<_> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["mods/a.jar", "config/x/y.toml", "options.txt"]);
        assert!(collect(&game, &["../x".into()]).is_err());
        assert!(is_linkable("mods/a.jar") && !is_linkable("config/x/y.toml") && !is_linkable("mods/sub/a.jar"));

        let out = dir.path().join("pack.mrpack");
        let token = tokio_util::sync::CancellationToken::new();
        write_archive(&out, "modrinth.index.json", &mrpack_index(&meta(None), &[]), &files, &token, |_, _| {}).unwrap();
        let idx = read_index(&out).unwrap();
        assert_eq!(idx.name, "Мой пак");
        assert_eq!(crate::modpacks::format::count_overrides(&out, &idx.overrides).unwrap(), 3);
    }
}
