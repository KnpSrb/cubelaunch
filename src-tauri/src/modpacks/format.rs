//! Разбор архивов модпаков: `.mrpack` (`modrinth.index.json`) и zip CurseForge
//! (`manifest.json`), извлечение overrides.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{join_rel, safe_rel};
use crate::error::{AppError, AppResult};
use crate::instances::LoaderKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackFormat {
    Mrpack,
    Curseforge,
}

/// Файл, который нужно скачать.
#[derive(Debug, Clone, PartialEq)]
pub struct PackFile {
    pub path: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub urls: Vec<String>,
}

/// Ссылка на файл CurseForge (разрешается через API).
#[derive(Debug, Clone, PartialEq)]
pub struct CfRef {
    pub project_id: u64,
    pub file_id: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackIndex {
    pub format: PackFormat,
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub minecraft: String,
    /// Загрузчик и его версия в записи пака (для Forge — без версии игры).
    pub loader: Option<(LoaderKind, String)>,
    pub files: Vec<PackFile>,
    pub cf_files: Vec<CfRef>,
    /// Папки overrides внутри архива — в порядке применения.
    pub overrides: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrIndex {
    #[serde(default)]
    format_version: u32,
    #[serde(default)]
    game: String,
    #[serde(default)]
    version_id: String,
    name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    files: Vec<MrFile>,
    #[serde(default)]
    dependencies: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrFile {
    path: String,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: Option<MrEnv>,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    file_size: u64,
}

#[derive(Deserialize)]
struct MrEnv {
    #[serde(default)]
    client: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfManifest {
    minecraft: CfMinecraft,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    files: Vec<CfManifestFile>,
    #[serde(default = "default_overrides")]
    overrides: String,
}

fn default_overrides() -> String {
    "overrides".into()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMinecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CfLoader>,
}

#[derive(Deserialize)]
struct CfLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize)]
struct CfManifestFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    #[serde(default = "yes")]
    required: bool,
}

fn yes() -> bool {
    true
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::Content(msg.into())
}

fn open_zip(path: &Path) -> AppResult<zip::ZipArchive<std::fs::File>> {
    let f = std::fs::File::open(path)?;
    zip::ZipArchive::new(f).map_err(|e| bad(format!("файл не является архивом модпака: {e}")))
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> AppResult<Option<Vec<u8>>> {
    let mut e = match zip.by_name(name) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(bad(format!("архив повреждён: {e}"))),
    };
    let mut buf = Vec::new();
    e.read_to_end(&mut buf)?;
    Ok(Some(buf))
}

/// Только https: файлы пака проверяются по SHA1, но протокол без шифрования не берём.
fn https_only(urls: Vec<String>) -> Vec<String> {
    urls.into_iter().filter(|u| u.starts_with("https://")).collect()
}

/// Читает описание модпака из архива.
pub fn read_index(path: &Path) -> AppResult<PackIndex> {
    let mut zip = open_zip(path)?;
    if let Some(bytes) = read_entry(&mut zip, "modrinth.index.json")? {
        return parse_mrpack(&bytes);
    }
    if let Some(bytes) = read_entry(&mut zip, "manifest.json")? {
        return parse_curseforge(&bytes);
    }
    Err(bad("в архиве нет modrinth.index.json или manifest.json — это не модпак Modrinth или CurseForge"))
}

pub fn parse_mrpack(bytes: &[u8]) -> AppResult<PackIndex> {
    let idx: MrIndex = serde_json::from_slice(bytes).map_err(|e| bad(format!("modrinth.index.json: {e}")))?;
    if idx.format_version != 1 || (!idx.game.is_empty() && idx.game != "minecraft") {
        return Err(bad(format!("неподдерживаемый формат .mrpack (formatVersion {}, game {})", idx.format_version, idx.game)));
    }
    let minecraft = idx.dependencies.get("minecraft").cloned().ok_or_else(|| bad("в модпаке не указана версия Minecraft"))?;
    let loader = [
        ("fabric-loader", LoaderKind::Fabric),
        ("quilt-loader", LoaderKind::Quilt),
        ("neoforge", LoaderKind::NeoForge),
        ("forge", LoaderKind::Forge),
    ]
    .into_iter()
    .find_map(|(key, kind)| idx.dependencies.get(key).map(|v| (kind, v.clone())));
    let mut files = Vec::new();
    for f in idx.files {
        if f.env.as_ref().is_some_and(|e| e.client == "unsupported") {
            continue;
        }
        let path = safe_rel(&f.path)?;
        let urls = https_only(f.downloads);
        if urls.is_empty() {
            return Err(bad(format!("у файла {path} нет ссылки для скачивания")));
        }
        let sha1 = f.hashes.get("sha1").map(|s| s.to_lowercase());
        if sha1.is_none() {
            return Err(bad(format!("у файла {path} нет SHA1")));
        }
        files.push(PackFile { path, sha1, size: Some(f.file_size).filter(|s| *s > 0), urls });
    }
    Ok(PackIndex {
        format: PackFormat::Mrpack,
        name: idx.name,
        version: idx.version_id,
        summary: idx.summary.filter(|s| !s.trim().is_empty()),
        minecraft,
        loader,
        files,
        cf_files: vec![],
        overrides: vec!["overrides".into(), "client-overrides".into()],
    })
}

pub fn parse_curseforge(bytes: &[u8]) -> AppResult<PackIndex> {
    let m: CfManifest = serde_json::from_slice(bytes).map_err(|e| bad(format!("manifest.json: {e}")))?;
    let primary = m.minecraft.mod_loaders.iter().find(|l| l.primary).or(m.minecraft.mod_loaders.first());
    let loader = match primary {
        None => None,
        Some(l) => {
            let (kind, version) = l.id.split_once('-').ok_or_else(|| bad(format!("непонятный загрузчик {}", l.id)))?;
            let kind = match kind {
                "forge" => LoaderKind::Forge,
                "neoforge" => LoaderKind::NeoForge,
                "fabric" => LoaderKind::Fabric,
                "quilt" => LoaderKind::Quilt,
                other => return Err(bad(format!("загрузчик {other} не поддерживается"))),
            };
            Some((kind, version.to_string()))
        }
    };
    let overrides = safe_rel(&m.overrides)?;
    Ok(PackIndex {
        format: PackFormat::Curseforge,
        name: if m.name.trim().is_empty() { "Модпак CurseForge".into() } else { m.name },
        version: m.version,
        summary: None,
        minecraft: m.minecraft.version,
        loader,
        files: vec![],
        cf_files: m.files.into_iter().filter(|f| f.required).map(|f| CfRef { project_id: f.project_id, file_id: f.file_id }).collect(),
        overrides: vec![overrides],
    })
}

/// Сколько файлов в overrides (для предпросмотра).
pub fn count_overrides(path: &Path, prefixes: &[String]) -> AppResult<usize> {
    let zip = open_zip(path)?;
    Ok(zip
        .file_names()
        .filter(|n| !n.ends_with('/') && prefixes.iter().any(|p| n.starts_with(&format!("{p}/"))))
        .count())
}

/// Распаковывает overrides в `dest`. Возвращает путь → SHA1 (более поздняя папка
/// overrides перекрывает раннюю).
pub fn extract_overrides(path: &Path, prefixes: &[String], dest: &Path, cancel: &tokio_util::sync::CancellationToken) -> AppResult<BTreeMap<String, String>> {
    let mut zip = open_zip(path)?;
    let mut out = BTreeMap::new();
    for prefix in prefixes {
        let pfx = format!("{prefix}/");
        for i in 0..zip.len() {
            if cancel.is_cancelled() {
                return Err(AppError::Cancelled);
            }
            let mut e = zip.by_index(i).map_err(|e| bad(format!("архив повреждён: {e}")))?;
            let Some(rest) = e.name().strip_prefix(&pfx).map(str::to_string) else { continue };
            if e.is_dir() || rest.is_empty() {
                continue;
            }
            let rel = safe_rel(&rest)?;
            let target = join_rel(dest, &rel);
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut f = std::fs::File::create(&target)?;
            let mut h = sha1_smol::Sha1::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = e.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
                std::io::Write::write_all(&mut f, &buf[..n])?;
            }
            out.insert(rel, h.digest().to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;

    pub fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    pub const MRPACK_INDEX: &str = r#"{
        "formatVersion": 1, "game": "minecraft", "versionId": "1.2.0", "name": "Тестовый пак", "summary": "",
        "files": [
            { "path": "mods/a.jar", "hashes": { "sha1": "AAAA", "sha512": "x" }, "downloads": ["https://cdn.modrinth.com/a.jar", "http://insecure/a.jar"], "fileSize": 10 },
            { "path": "mods/server-only.jar", "hashes": { "sha1": "bb" }, "env": { "client": "unsupported", "server": "required" }, "downloads": ["https://cdn.modrinth.com/s.jar"] }
        ],
        "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.16.5" }
    }"#;

    #[test]
    fn parses_mrpack() {
        let p = parse_mrpack(MRPACK_INDEX.as_bytes()).unwrap();
        assert_eq!(p.minecraft, "1.21.1");
        assert_eq!(p.loader, Some((LoaderKind::Fabric, "0.16.5".into())));
        assert_eq!(p.files.len(), 1, "серверный файл пропущен");
        assert_eq!(p.files[0].urls, vec!["https://cdn.modrinth.com/a.jar"]);
        assert_eq!(p.files[0].sha1.as_deref(), Some("aaaa"));
        assert!(p.summary.is_none());

        let evil = MRPACK_INDEX.replace("mods/a.jar", "../../evil.jar");
        assert!(parse_mrpack(evil.as_bytes()).is_err());
        let no_mc = MRPACK_INDEX.replace("\"minecraft\": \"1.21.1\", ", "");
        assert!(parse_mrpack(no_mc.as_bytes()).is_err());
    }

    #[test]
    fn parses_curseforge_manifest() {
        let m = br#"{ "minecraft": { "version": "1.20.1", "modLoaders": [{ "id": "forge-47.2.0", "primary": true }] },
            "manifestType": "minecraftModpack", "manifestVersion": 1, "name": "CF Pack", "version": "3.1", "author": "me",
            "files": [{ "projectID": 238222, "fileID": 4712345, "required": true }, { "projectID": 1, "fileID": 2, "required": false }],
            "overrides": "overrides" }"#;
        let p = parse_curseforge(m).unwrap();
        assert_eq!(p.loader, Some((LoaderKind::Forge, "47.2.0".into())));
        assert_eq!(p.cf_files, vec![CfRef { project_id: 238222, file_id: 4712345 }]);
        assert_eq!(p.overrides, vec!["overrides"]);
    }

    #[test]
    fn reads_archive_and_extracts_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("p.mrpack");
        make_zip(&pack, &[
            ("modrinth.index.json", MRPACK_INDEX.as_bytes()),
            ("overrides/config/a.toml", b"common"),
            ("overrides/options.txt", b"gui:2"),
            ("client-overrides/config/a.toml", b"client"),
            ("overrides/../escape.txt", b"x"),
        ]);
        let idx = read_index(&pack).unwrap();
        assert_eq!(idx.format, PackFormat::Mrpack);
        assert_eq!(count_overrides(&pack, &idx.overrides).unwrap(), 4);
        let out = dir.path().join("out");
        let token = tokio_util::sync::CancellationToken::new();
        // Путь с `..` — ошибка, ничего за пределы не пишется.
        assert!(extract_overrides(&pack, &idx.overrides, &out, &token).is_err());
        assert!(!dir.path().join("escape.txt").exists());

        make_zip(&pack, &[
            ("modrinth.index.json", MRPACK_INDEX.as_bytes()),
            ("overrides/config/a.toml", b"common"),
            ("client-overrides/config/a.toml", b"client"),
        ]);
        let files = extract_overrides(&pack, &idx.overrides, &out, &token).unwrap();
        assert_eq!(std::fs::read(out.join("config/a.toml")).unwrap(), b"client");
        assert_eq!(files.len(), 1);

        let not_pack = dir.path().join("x.zip");
        make_zip(&not_pack, &[("readme.txt", b"hi")]);
        assert!(matches!(read_index(&not_pack), Err(AppError::Content(_))));
    }
}
