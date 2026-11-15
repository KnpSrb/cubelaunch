//! Автоскачивание Eclipse Temurin (Adoptium API) с проверкой SHA-256.
//!
//! Java ставится в `<data>/java/temurin-<major>-<arch>/`.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::detect::find_java_in;
use super::{probe, JavaInstall};
use crate::downloads::queue::{download_all, DownloadOptions, DownloadTask};
use crate::error::{AppError, AppResult};
use crate::minecraft::install::Reporter;
use crate::minecraft::rules::{Arch, OsName};

const API: &str = "https://api.adoptium.net/v3/assets/latest";

#[derive(Debug, Deserialize)]
struct Asset {
    binary: Binary,
}

#[derive(Debug, Deserialize)]
struct Binary {
    package: Package,
}

#[derive(Debug, Deserialize)]
struct Package {
    checksum: String,
    link: String,
    name: String,
    size: u64,
}

fn adoptium_os(os: OsName) -> &'static str {
    match os {
        OsName::Windows => "windows",
        OsName::Linux => "linux",
        OsName::Osx => "mac",
    }
}

fn adoptium_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::X86_64 => "x64",
        Arch::Arm64 => "aarch64",
        Arch::X86 => "x32",
        Arch::Arm32 => "arm",
    }
}

pub fn target_dir(java_root: &Path, major: u32, arch: Arch) -> PathBuf {
    java_root.join(format!("temurin-{major}-{}", adoptium_arch(arch)))
}

async fn query(client: &reqwest::Client, major: u32, arch: Arch, image: &str) -> AppResult<Option<Package>> {
    let url = format!(
        "{API}/{major}/hotspot?architecture={}&image_type={image}&os={}&vendor=eclipse",
        adoptium_arch(arch),
        adoptium_os(OsName::current())
    );
    let resp = client.get(&url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let assets: Vec<Asset> = resp.error_for_status()?.json().await?;
    Ok(assets.into_iter().next().map(|a| a.binary.package))
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// Распаковывает zip или tar.gz в `dest` (без выхода за пределы папки).
pub fn extract_archive(archive: &Path, dest: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dest)?;
    let name = archive.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name.ends_with(".zip") {
        let file = std::fs::File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| AppError::Other(format!("архив повреждён: {e}")))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| AppError::Other(e.to_string()))?;
            let Some(rel) = entry.enclosed_name() else { continue };
            let out = dest.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut f = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut f)?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode))?;
            }
        }
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let file = std::fs::File::open(archive)?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        tar.set_preserve_permissions(true);
        // `unpack` сам отбрасывает пути, выходящие за пределы `dest`.
        tar.unpack(dest)?;
    } else {
        return Err(AppError::Other(format!("неизвестный формат архива: {name}")));
    }
    Ok(())
}

/// Скачивает и ставит Temurin нужной версии. Если сборки под архитектуру нет
/// (например, Java 8 под Apple Silicon), пробует x86_64 (Rosetta).
#[allow(clippy::too_many_arguments)]
pub async fn install_temurin(
    client: &reqwest::Client,
    java_root: &Path,
    cache_dir: &Path,
    major: u32,
    arch: Arch,
    opts: DownloadOptions,
    cancel: &CancellationToken,
    report: &dyn Reporter,
) -> AppResult<JavaInstall> {
    report.stage("javaDownload");
    let mut chosen = None;
    let mut archs = vec![arch];
    if OsName::current() == OsName::Osx && arch == Arch::Arm64 {
        archs.push(Arch::X86_64);
    }
    'outer: for a in archs {
        for image in ["jre", "jdk"] {
            if let Some(pkg) = query(client, major, a, image).await? {
                chosen = Some((a, pkg));
                break 'outer;
            }
        }
    }
    let (arch, pkg) = chosen.ok_or_else(|| AppError::NotFound(format!("сборка Temurin {major} для этой платформы")))?;

    let target = target_dir(java_root, major, arch);
    // Уже установлена и работает — ничего не делаем.
    let mut existing = Vec::new();
    find_java_in(&target, 3, &mut existing);
    if let Some(exe) = existing.first() {
        if let Ok(j) = probe(exe, Some(java_root)).await {
            return Ok(j);
        }
    }

    tracing::info!("скачивание Java: {}", pkg.name);
    let archive = cache_dir.join(&pkg.name);
    let task = DownloadTask { url: pkg.link.clone(), dest: archive.clone(), sha1: None, size: Some(pkg.size) };
    download_all(client, vec![task], opts, cancel, |p| report.progress(p)).await?;

    report.stage("javaExtract");
    let expected = pkg.checksum.to_lowercase();
    let tmp = java_root.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    let result = tokio::task::spawn_blocking({
        let archive = archive.clone();
        let tmp = tmp.clone();
        let target = target.clone();
        move || -> AppResult<()> {
            let actual = sha256_file(&archive)?;
            if actual != expected {
                let _ = std::fs::remove_file(&archive);
                return Err(AppError::Checksum { file: archive.display().to_string(), expected, actual });
            }
            extract_archive(&archive, &tmp)?;
            if target.exists() {
                std::fs::remove_dir_all(&target)?;
            }
            std::fs::rename(&tmp, &target)?;
            let _ = std::fs::remove_file(&archive);
            Ok(())
        }
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }

    let mut found = Vec::new();
    find_java_in(&target, 4, &mut found);
    let exe = found
        .into_iter()
        .next()
        .ok_or_else(|| AppError::JavaInvalid("в скачанном архиве нет bin/java".into()))?;
    probe(&exe, Some(java_root)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_extraction_stays_inside() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("jdk/bin/java", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.start_file("../evil.txt", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let out = dir.path().join("out");
        extract_archive(&archive, &out).unwrap();
        assert!(out.join("jdk/bin/java").exists());
        assert!(!dir.path().join("evil.txt").exists());
    }

    #[test]
    fn target_dir_name() {
        assert!(target_dir(Path::new("/j"), 17, Arch::Arm64).ends_with("temurin-17-aarch64"));
    }

    /// Запуск: `cargo test -- --ignored adoptium_query --nocapture`
    #[tokio::test]
    #[ignore = "требует сети"]
    async fn adoptium_query() {
        let client = reqwest::Client::new();
        for m in [8, 17, 21] {
            let p = query(&client, m, Arch::current(), "jre").await.unwrap().expect("нет сборки");
            println!("{m}: {} {} байт", p.name, p.size);
            assert_eq!(p.checksum.len(), 64);
        }
    }
}
