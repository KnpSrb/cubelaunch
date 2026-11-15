//! Forge и NeoForge: разбор официального установщика.
//!
//! Установщик (`*-installer.jar`) качается с maven проекта. Из него берутся:
//! - `install_profile.json` — библиотеки установщика, переменные (`data`) и
//!   процессоры, которые собирают патченый клиент из ванильного jar;
//! - `version.json` — профиль запуска (`inheritsFrom` на ванильную версию);
//! - `maven/…` — встроенные библиотеки, которых нет в репозиториях.
//!
//! Процессоры — обычные java-программы; запускаем их с теми же аргументами,
//! что и установщик для стороны `client`. Если выходные файлы уже на месте и
//! их SHA1 совпадает, процессор пропускается (быстрая переустановка).
//!
//! Старые установщики (до 1.12.2) хранят профиль в `versionInfo`, а
//! universal-jar лежит в корне архива (`install.filePath`).

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use serde_json::Value;
use zip::ZipArchive;

use super::{profile_id, save_profile};
use crate::downloads::queue::{download_all, sha1_file, DownloadTask, Progress};
use crate::error::{AppError, AppResult};
use crate::instances::{LoaderKind, LoaderSpec};
use crate::java::no_console;
use crate::minecraft::install::{client_jar_path, InstallParams, Reporter};
use crate::minecraft::libraries::{resolve_libraries, MavenCoord};
use crate::minecraft::version::{Library, VersionJson};

pub const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/";
pub const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases/";

pub fn installer_url(spec: &LoaderSpec) -> String {
    let v = &spec.version;
    let (base, coord) = match spec.kind {
        LoaderKind::NeoForge if v.starts_with("1.20.1-") => (NEOFORGE_MAVEN, format!("net.neoforged:forge:{v}:installer")),
        LoaderKind::NeoForge => (NEOFORGE_MAVEN, format!("net.neoforged:neoforge:{v}:installer")),
        _ => (FORGE_MAVEN, format!("net.minecraftforge:forge:{v}:installer")),
    };
    // Координаты собраны из проверенной версии, разбор не падает.
    let path = MavenCoord::parse(&coord).map(|c| c.path()).unwrap_or_default();
    format!("{base}{path}")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallProfile {
    #[serde(default)]
    minecraft: Option<String>,
    #[serde(default)]
    json: Option<String>,
    #[serde(default)]
    data: HashMap<String, SidedValue>,
    #[serde(default)]
    processors: Vec<Processor>,
    #[serde(default)]
    libraries: Vec<Library>,
    /// Старый формат.
    #[serde(default)]
    install: Option<LegacyInstall>,
    #[serde(default)]
    version_info: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SidedValue {
    #[serde(default)]
    client: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Processor {
    #[serde(default)]
    sides: Option<Vec<String>>,
    jar: String,
    #[serde(default)]
    classpath: Vec<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    outputs: HashMap<String, String>,
}

impl Processor {
    fn for_client(&self) -> bool {
        self.sides.as_ref().is_none_or(|s| s.iter().any(|x| x == "client"))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyInstall {
    path: String,
    file_path: String,
    #[serde(default)]
    minecraft: Option<String>,
}

/// Результат разбора установщика.
#[derive(Debug)]
pub struct Prepared {
    pub version: VersionJson,
    /// Всё, что нужно скачать: библиотеки установщика и профиля.
    pub libraries: Vec<Library>,
    pub processors: Vec<Processor>,
    /// Переменные для аргументов процессоров (уже разрешённые в пути/литералы).
    pub data: HashMap<String, String>,
}

fn zip_err(e: zip::result::ZipError) -> AppError {
    AppError::Loader(format!("повреждённый установщик: {e}"))
}

fn coord_path(libs_dir: &Path, coord: &str) -> AppResult<PathBuf> {
    let c = MavenCoord::parse(coord).ok_or_else(|| AppError::Loader(format!("некорректные координаты {coord}")))?;
    Ok(libs_dir.join(c.path()))
}

/// Извлекает файл из архива. `false` — такого файла в архиве нет.
fn extract_entry(zip: &mut ZipArchive<File>, name: &str, dest: &Path) -> AppResult<bool> {
    let mut entry = match zip.by_name(name) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(false),
        Err(e) => return Err(zip_err(e)),
    };
    if dest.metadata().is_ok_and(|m| m.len() == entry.size()) {
        return Ok(true);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("part");
    let mut out = File::create(&tmp)?;
    std::io::copy(&mut entry, &mut out)?;
    drop(out);
    std::fs::rename(&tmp, dest)?;
    Ok(true)
}

fn read_entry(zip: &mut ZipArchive<File>, name: &str) -> AppResult<Vec<u8>> {
    let mut e = zip.by_name(name).map_err(|_| AppError::Loader(format!("в установщике нет {name}")))?;
    let mut buf = Vec::new();
    e.read_to_end(&mut buf)?;
    Ok(buf)
}

fn check_minecraft(found: Option<&str>, mc: &str) -> AppResult<()> {
    match found {
        Some(f) if f != mc => Err(AppError::Loader(format!("установщик рассчитан на Minecraft {f}, а не {mc}"))),
        _ => Ok(()),
    }
}

/// Старые установщики ссылаются на снятое зеркало files.minecraftforge.net/maven.
fn fix_legacy_url(url: &str) -> String {
    for old in ["http://files.minecraftforge.net/maven/", "https://files.minecraftforge.net/maven/", "http://files.minecraftforge.net/maven"] {
        if let Some(rest) = url.strip_prefix(old) {
            return format!("{FORGE_MAVEN}{}", rest.trim_start_matches('/'));
        }
    }
    url.replacen("http://", "https://", 1)
}

/// Разбирает установщик: профиль, библиотеки, встроенные файлы и переменные.
pub fn prepare(installer: &Path, mc: &str, libs_dir: &Path, work: &Path) -> AppResult<Prepared> {
    let mut zip = ZipArchive::new(File::open(installer)?).map_err(zip_err)?;
    let profile: InstallProfile = serde_json::from_slice(&read_entry(&mut zip, "install_profile.json")?)?;

    // Старый формат (1.5–1.12.2).
    if let Some(mut info) = profile.version_info {
        let install = profile
            .install
            .ok_or_else(|| AppError::Loader("в старом установщике нет секции install".into()))?;
        check_minecraft(install.minecraft.as_deref(), mc)?;
        if let Some(libs) = info.get_mut("libraries").and_then(Value::as_array_mut) {
            // Явно серверные библиотеки клиенту не нужны.
            libs.retain(|l| l.get("clientreq").and_then(Value::as_bool) != Some(false));
            for l in libs.iter_mut() {
                if let Some(url) = l.get("url").and_then(Value::as_str).map(fix_legacy_url) {
                    l["url"] = Value::String(url);
                }
            }
        }
        let version: VersionJson = serde_json::from_value(info)?;
        let dest = coord_path(libs_dir, &install.path)?;
        if !extract_entry(&mut zip, &install.file_path, &dest)? {
            return Err(AppError::Loader(format!("в установщике нет {}", install.file_path)));
        }
        return Ok(Prepared { libraries: version.libraries.clone(), version, processors: vec![], data: HashMap::new() });
    }

    check_minecraft(profile.minecraft.as_deref(), mc)?;
    let json_name = profile.json.as_deref().unwrap_or("/version.json").trim_start_matches('/').to_string();
    let version: VersionJson = serde_json::from_slice(&read_entry(&mut zip, &json_name)?)?;

    let mut libraries = profile.libraries;
    libraries.extend(version.libraries.iter().cloned());
    // Встроенные библиотеки (ссылка пустая) достаём из maven/ установщика.
    for lib in &libraries {
        let Some(a) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) else { continue };
        if !a.url.is_empty() {
            continue;
        }
        let path = match &a.path {
            Some(p) => p.clone(),
            None => MavenCoord::parse(&lib.name).map(|c| c.path()).unwrap_or_default(),
        };
        if !path.is_empty() {
            // Нет в архиве — значит файл создаст процессор.
            extract_entry(&mut zip, &format!("maven/{path}"), &libs_dir.join(&path))?;
        }
    }

    let mut data = HashMap::new();
    for (key, value) in profile.data {
        let v = value.client;
        let resolved = if let Some(c) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            coord_path(libs_dir, c)?.display().to_string()
        } else if let Some(l) = v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            l.to_string()
        } else if let Some(inner) = v.strip_prefix('/') {
            let dest = work.join(inner);
            if !extract_entry(&mut zip, inner, &dest)? {
                return Err(AppError::Loader(format!("в установщике нет {inner}")));
            }
            dest.display().to_string()
        } else {
            v
        };
        data.insert(key, resolved);
    }

    let processors = profile.processors.into_iter().filter(Processor::for_client).collect();
    Ok(Prepared { version, libraries, processors, data })
}

/// Подстановка в аргумент процессора: `[coord]` → путь, `'literal'`, `{KEY}` → переменная.
pub fn expand(arg: &str, data: &HashMap<String, String>, libs_dir: &Path) -> AppResult<String> {
    if let Some(c) = arg.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return Ok(coord_path(libs_dir, c)?.display().to_string());
    }
    if let Some(l) = arg.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Ok(l.to_string());
    }
    let mut out = String::new();
    let mut rest = arg;
    while let Some(start) = rest.find('{') {
        let Some(len) = rest[start..].find('}') else { break };
        let key = &rest[start + 1..start + len];
        let value = data
            .get(key)
            .ok_or_else(|| AppError::Loader(format!("неизвестная переменная установщика {{{key}}}")))?;
        out.push_str(&rest[..start]);
        out.push_str(value);
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `Main-Class` из манифеста jar.
pub fn main_class(jar: &Path) -> AppResult<String> {
    let mut zip = ZipArchive::new(File::open(jar)?).map_err(zip_err)?;
    let text = String::from_utf8_lossy(&read_entry(&mut zip, "META-INF/MANIFEST.MF")?).to_string();
    // Длинные строки манифеста переносятся с пробелом в начале продолжения.
    let unfolded = text.replace("\r\n", "\n").replace("\n ", "");
    unfolded
        .lines()
        .find_map(|l| l.strip_prefix("Main-Class:").map(|m| m.trim().to_string()))
        .filter(|m| !m.is_empty())
        .ok_or_else(|| AppError::Loader(format!("в {} не указан Main-Class", jar.display())))
}

async fn outputs_valid(outputs: &[(PathBuf, String)]) -> bool {
    for (path, sha1) in outputs {
        if !path.is_file() {
            return false;
        }
        match sha1_file(path).await {
            Ok(actual) if actual.eq_ignore_ascii_case(sha1) => {}
            _ => return false,
        }
    }
    true
}

fn tail(bytes: &[u8], lines: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

async fn run_processor(
    proc: &Processor,
    data: &HashMap<String, String>,
    libs_dir: &Path,
    java: &Path,
    work: &Path,
    cancel: &tokio_util::sync::CancellationToken,
) -> AppResult<()> {
    let outputs = proc
        .outputs
        .iter()
        .map(|(k, v)| Ok((PathBuf::from(expand(k, data, libs_dir)?), expand(v, data, libs_dir)?)))
        .collect::<AppResult<Vec<_>>>()?;
    if !outputs.is_empty() && outputs_valid(&outputs).await {
        tracing::info!("процессор {} пропущен: результат уже есть", proc.jar);
        return Ok(());
    }

    let jar = coord_path(libs_dir, &proc.jar)?;
    let main = {
        let jar = jar.clone();
        tokio::task::spawn_blocking(move || main_class(&jar)).await.map_err(|e| AppError::Other(e.to_string()))??
    };
    let mut cp = vec![jar];
    for c in &proc.classpath {
        cp.push(coord_path(libs_dir, c)?);
    }
    let cp = std::env::join_paths(&cp).map_err(|e| AppError::Loader(format!("некорректный classpath: {e}")))?;
    let args = proc.args.iter().map(|a| expand(a, data, libs_dir)).collect::<AppResult<Vec<_>>>()?;

    tracing::info!("процессор {}: {main} {}", proc.jar, args.join(" "));
    std::fs::create_dir_all(work)?;
    let mut cmd = tokio::process::Command::new(java);
    cmd.arg("-cp")
        .arg(cp)
        .arg(&main)
        .args(&args)
        .current_dir(work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    no_console(&mut cmd);
    let out = tokio::select! {
        o = cmd.output() => o.map_err(|e| AppError::Loader(format!("не удалось запустить Java {}: {e}", java.display())))?,
        _ = cancel.cancelled() => return Err(AppError::Cancelled),
    };
    tracing::debug!("вывод процессора {}:\n{}", proc.jar, String::from_utf8_lossy(&out.stdout));
    if !out.status.success() {
        let mut log = tail(&out.stdout, 10);
        let err = tail(&out.stderr, 15);
        if !err.is_empty() {
            log = format!("{log}\n{err}");
        }
        return Err(AppError::Loader(format!(
            "процессор {} завершился с кодом {}:\n{}",
            proc.jar,
            out.status.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into()),
            log.trim()
        )));
    }
    for (path, sha1) in &outputs {
        let actual = sha1_file(path)
            .await
            .map_err(|_| AppError::Loader(format!("процессор {} не создал {}", proc.jar, path.display())))?;
        if !actual.eq_ignore_ascii_case(sha1) {
            return Err(AppError::Checksum { file: path.display().to_string(), expected: sha1.clone(), actual });
        }
    }
    Ok(())
}

/// SHA1 из файла `<url>.sha1` рядом с артефактом (если репозиторий его отдаёт).
async fn remote_sha1(client: &reqwest::Client, url: &str) -> Option<String> {
    let resp = client.get(format!("{url}.sha1")).send().await.ok()?.error_for_status().ok()?;
    let text = resp.text().await.ok()?;
    let hash = text.split_whitespace().next()?.to_lowercase();
    (hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit())).then_some(hash)
}

pub async fn install(
    p: &InstallParams<'_>,
    mc: &str,
    spec: &LoaderSpec,
    vanilla: &VersionJson,
    java: &Path,
    report: &dyn Reporter,
) -> AppResult<VersionJson> {
    report.stage("loaderInstaller");
    let url = installer_url(spec);
    let file = url.rsplit('/').next().unwrap_or("installer.jar").to_string();
    let installer = p.paths.cache_dir().join("installers").join(&file);
    let sha1 = remote_sha1(p.client, &url).await;
    let task = DownloadTask { url: url.clone(), dest: installer.clone(), sha1, size: None };
    download_all(p.client, vec![task], p.opts, p.cancel, |_| {}).await.map_err(|e| match e {
        AppError::Download { ref reason, .. } if reason.contains("404") => {
            AppError::Loader(format!("установщик {} {} не найден в репозитории ({url})", spec.kind.display(), spec.version))
        }
        e => e,
    })?;

    let libs_dir = p.paths.libraries_dir();
    let work = p.paths.cache_dir().join("loader-work").join(profile_id(mc, spec));
    let prepared = {
        let (inst, mc, libs, work) = (installer.clone(), mc.to_string(), libs_dir.clone(), work.clone());
        tokio::task::spawn_blocking(move || prepare(&inst, &mc, &libs, &work))
            .await
            .map_err(|e| AppError::Other(e.to_string()))??
    };

    // Библиотеки со ссылками качаем; без ссылки — встроенные или создаются процессорами.
    report.stage("loaderLibraries");
    let mut tasks = Vec::new();
    for lib in resolve_libraries(&prepared.libraries, &p.env) {
        if let Some(url) = lib.url {
            tasks.push(DownloadTask { url, dest: libs_dir.join(&lib.path), sha1: lib.sha1, size: lib.size });
        }
    }
    download_all(p.client, tasks, p.opts, p.cancel, |pr| report.progress(pr)).await?;

    if !prepared.processors.is_empty() {
        let mut data = prepared.data;
        let client_jar = client_jar_path(p.paths, mc);
        data.insert("SIDE".into(), "client".into());
        data.insert("MINECRAFT_JAR".into(), client_jar.display().to_string());
        data.insert("MINECRAFT_VERSION".into(), mc.to_string());
        data.insert("ROOT".into(), work.display().to_string());
        data.insert("INSTALLER".into(), installer.display().to_string());
        data.insert("LIBRARY_DIR".into(), libs_dir.display().to_string());

        // Установщики 1.16 и старше сами качают маппинги Mojang, если для них нет процессора.
        let has_task = prepared.processors.iter().any(|pr| pr.args.iter().any(|a| a == "DOWNLOAD_MOJMAPS"));
        if let (Some(target), false) = (data.get("MOJMAPS"), has_task) {
            if let Some(d) = vanilla.downloads.get("client_mappings") {
                let t = DownloadTask { url: d.url.clone(), dest: PathBuf::from(target), sha1: d.sha1.clone(), size: d.size };
                download_all(p.client, vec![t], p.opts, p.cancel, |_| {}).await?;
            }
        }

        report.stage("loaderProcessors");
        let total = prepared.processors.len();
        for (i, proc) in prepared.processors.iter().enumerate() {
            report.progress(Progress { files_done: i, files_total: total, bytes_done: 0, bytes_total: 0 });
            run_processor(proc, &data, &libs_dir, java, &work, p.cancel).await?;
        }
        report.progress(Progress { files_done: total, files_total: total, bytes_done: 0, bytes_total: 0 });
    }

    // Всё, что нужно для запуска, должно быть на месте.
    for lib in resolve_libraries(&prepared.version.libraries, &p.env) {
        if !libs_dir.join(&lib.path).is_file() {
            return Err(AppError::Loader(format!("после установки нет библиотеки {}", lib.name)));
        }
    }
    let _ = std::fs::remove_dir_all(&work);
    save_profile(p.paths, mc, spec, prepared.version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn installer_urls() {
        let f = LoaderSpec { kind: LoaderKind::Forge, version: "1.20.1-47.3.0".into() };
        assert_eq!(
            installer_url(&f),
            "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar"
        );
        let n = LoaderSpec { kind: LoaderKind::NeoForge, version: "21.1.77".into() };
        assert_eq!(
            installer_url(&n),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.77/neoforge-21.1.77-installer.jar"
        );
        let n1 = LoaderSpec { kind: LoaderKind::NeoForge, version: "1.20.1-47.1.106".into() };
        assert!(installer_url(&n1).ends_with("net/neoforged/forge/1.20.1-47.1.106/forge-1.20.1-47.1.106-installer.jar"));
    }

    #[test]
    fn expands_processor_arguments() {
        let libs = Path::new("L");
        let data = HashMap::from([("MAPPINGS".to_string(), "/m.txt".to_string()), ("SIDE".to_string(), "client".to_string())]);
        assert_eq!(expand("{MAPPINGS}", &data, libs).unwrap(), "/m.txt");
        assert_eq!(expand("--side={SIDE}!", &data, libs).unwrap(), "--side=client!");
        assert_eq!(expand("'abc'", &data, libs).unwrap(), "abc");
        assert_eq!(
            expand("[net.minecraft:client:1.20.1:slim]", &data, libs).unwrap(),
            Path::new("L").join("net/minecraft/client/1.20.1/client-1.20.1-slim.jar").display().to_string()
        );
        assert!(matches!(expand("{NOPE}", &data, libs), Err(AppError::Loader(_))));
        assert_eq!(expand("plain", &data, libs).unwrap(), "plain");
    }

    #[test]
    fn reads_main_class() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("t.jar");
        make_zip(&jar, &[("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nMain-Class: net.minecraftforge.binarypatcher.Con\r\n sole\r\n\r\n")]);
        assert_eq!(main_class(&jar).unwrap(), "net.minecraftforge.binarypatcher.Console");
    }

    #[test]
    fn prepares_modern_installer() {
        let dir = tempfile::tempdir().unwrap();
        let installer = dir.path().join("installer.jar");
        let profile = r#"{
          "spec": 1, "minecraft": "1.20.1", "json": "/version.json",
          "data": {
            "MAPPINGS": { "client": "[de.oceanlabs.mcp:mcp_config:1.20.1:mappings@txt]", "server": "x" },
            "BINPATCH": { "client": "/data/client.lzma", "server": "/data/server.lzma" },
            "MCP_VERSION": { "client": "'1.20.1-20230612'", "server": "'x'" }
          },
          "processors": [
            { "sides": ["server"], "jar": "a:b:1", "args": [] },
            { "jar": "net.minecraftforge:installertools:1.4.1", "classpath": [], "args": ["--task", "MCP_DATA"] }
          ],
          "libraries": [
            { "name": "net.minecraftforge:installertools:1.4.1",
              "downloads": { "artifact": { "path": "net/minecraftforge/installertools/1.4.1/installertools-1.4.1.jar", "url": "https://maven.minecraftforge.net/x.jar", "sha1": "aa", "size": 1 } } }
          ]
        }"#;
        let version = r#"{
          "id": "1.20.1-forge-47.3.0", "inheritsFrom": "1.20.1", "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
          "libraries": [
            { "name": "net.minecraftforge:forge:1.20.1-47.3.0:universal",
              "downloads": { "artifact": { "path": "net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-universal.jar", "url": "", "sha1": "bb", "size": 3 } } },
            { "name": "net.minecraftforge:forge:1.20.1-47.3.0:client",
              "downloads": { "artifact": { "path": "net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-client.jar", "url": "" } } }
          ]
        }"#;
        make_zip(
            &installer,
            &[
                ("install_profile.json", profile.as_bytes()),
                ("version.json", version.as_bytes()),
                ("data/client.lzma", b"patch"),
                ("maven/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-universal.jar", b"uni"),
            ],
        );
        let libs = dir.path().join("libraries");
        let work = dir.path().join("work");
        let p = prepare(&installer, "1.20.1", &libs, &work).unwrap();

        assert_eq!(p.version.main_class.as_deref(), Some("cpw.mods.bootstraplauncher.BootstrapLauncher"));
        assert_eq!(p.libraries.len(), 3);
        assert_eq!(p.processors.len(), 1, "серверный процессор отброшен");
        // Встроенная библиотека извлечена, создаваемая процессором — нет.
        assert!(libs.join("net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-universal.jar").is_file());
        assert!(!libs.join("net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-client.jar").exists());
        assert_eq!(std::fs::read(work.join("data/client.lzma")).unwrap(), b"patch");
        assert_eq!(p.data["MCP_VERSION"], "1.20.1-20230612");
        assert!(p.data["MAPPINGS"].ends_with("mcp_config-1.20.1-mappings.txt"));
        assert!(p.data["BINPATCH"].ends_with("client.lzma"));

        assert!(matches!(prepare(&installer, "1.19.2", &libs, &work), Err(AppError::Loader(_))));
    }

    #[test]
    fn prepares_legacy_installer() {
        let dir = tempfile::tempdir().unwrap();
        let installer = dir.path().join("installer.jar");
        let profile = r#"{
          "install": { "path": "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10",
                       "filePath": "forge-1.7.10-10.13.4.1614-1.7.10-universal.jar", "minecraft": "1.7.10" },
          "versionInfo": {
            "id": "1.7.10-Forge10.13.4.1614-1.7.10", "inheritsFrom": "1.7.10",
            "mainClass": "net.minecraft.launchwrapper.Launch",
            "minecraftArguments": "--username ${auth_player_name} --tweakClass cpw.mods.fml.common.launcher.FMLTweaker",
            "libraries": [
              { "name": "net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10", "url": "http://files.minecraftforge.net/maven/" },
              { "name": "net.minecraft:launchwrapper:1.12", "serverreq": true },
              { "name": "com.typesafe:config:1.2.1", "url": "http://files.minecraftforge.net/maven/", "clientreq": true },
              { "name": "lzma:lzma:0.0.1", "serverreq": true, "clientreq": false }
            ]
          }
        }"#;
        make_zip(
            &installer,
            &[("install_profile.json", profile.as_bytes()), ("forge-1.7.10-10.13.4.1614-1.7.10-universal.jar", b"universal")],
        );
        let libs = dir.path().join("libraries");
        let p = prepare(&installer, "1.7.10", &libs, &dir.path().join("w")).unwrap();
        let names: Vec<_> = p.version.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["net.minecraftforge:forge:1.7.10-10.13.4.1614-1.7.10", "net.minecraft:launchwrapper:1.12", "com.typesafe:config:1.2.1"]);
        assert_eq!(p.version.libraries[2].url.as_deref(), Some("https://maven.minecraftforge.net/"));
        assert!(p.processors.is_empty());
        let universal = libs.join("net/minecraftforge/forge/1.7.10-10.13.4.1614-1.7.10/forge-1.7.10-10.13.4.1614-1.7.10.jar");
        assert_eq!(std::fs::read(universal).unwrap(), b"universal");
    }
}
