//! Установка ванильной версии: version JSON → индекс assets → клиент, библиотеки,
//! конфиг логирования и объекты assets. Всё качается только с серверов Mojang.
//!
//! Раскладка в папке данных:
//! - `versions/<id>/<id>.json`, `versions/<id>/<id>.jar`
//! - `libraries/<maven path>`
//! - `assets/indexes/<assetIndex>.json`, `assets/objects/..`, `assets/log_configs/<file>`

use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use super::assets::{asset_tasks, AssetIndex};
use super::libraries::resolve_libraries;
use super::manifest::get_manifest;
use super::rules::Env;
use super::version::VersionJson;
use crate::downloads::queue::{download_all, DownloadOptions, DownloadTask, Progress};
use crate::error::{AppError, AppResult};
use crate::paths::AppPaths;
use crate::util::read_json_opt;

/// Колбэки отчёта о ходе установки (реализуются задачей `JobCtx` или тестом).
pub trait Reporter: Send + Sync {
    fn stage(&self, stage: &str);
    fn progress(&self, p: Progress);
}

impl Reporter for crate::downloads::jobs::JobCtx {
    fn stage(&self, stage: &str) {
        crate::downloads::jobs::JobCtx::stage(self, stage)
    }
    fn progress(&self, p: Progress) {
        crate::downloads::jobs::JobCtx::progress(self, p)
    }
}

pub struct InstallParams<'a> {
    pub client: &'a reqwest::Client,
    pub paths: &'a AppPaths,
    pub opts: DownloadOptions,
    pub cancel: &'a CancellationToken,
    pub env: Env,
    /// Не качать объекты assets (для тестов).
    pub skip_asset_objects: bool,
}

pub fn version_dir(paths: &AppPaths, id: &str) -> PathBuf {
    paths.versions_dir().join(id)
}

pub fn version_json_path(paths: &AppPaths, id: &str) -> PathBuf {
    version_dir(paths, id).join(format!("{id}.json"))
}

pub fn client_jar_path(paths: &AppPaths, id: &str) -> PathBuf {
    version_dir(paths, id).join(format!("{id}.jar"))
}

async fn fetch_single(p: &InstallParams<'_>, url: &str, dest: &Path, sha1: Option<String>, size: Option<u64>) -> AppResult<()> {
    let task = DownloadTask { url: url.to_string(), dest: dest.to_path_buf(), sha1, size };
    download_all(p.client, vec![task], p.opts, p.cancel, |_| {}).await
}

fn read_required<T: serde::de::DeserializeOwned>(path: &Path) -> AppResult<T> {
    read_json_opt(path)?.ok_or_else(|| AppError::NotFound(path.display().to_string()))
}

pub async fn install_version(
    p: &InstallParams<'_>,
    version_id: &str,
    report: &dyn Reporter,
) -> AppResult<VersionJson> {
    report.stage("manifest");
    let mut manifest = get_manifest(p.client, &p.paths.meta_dir(), false).await?;
    if manifest.find(version_id).is_none() {
        // Версия могла выйти после кэширования манифеста.
        manifest = get_manifest(p.client, &p.paths.meta_dir(), true).await?;
    }
    let entry = manifest
        .find(version_id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("версия {version_id}")))?;

    report.stage("versionJson");
    let json_path = version_json_path(p.paths, version_id);
    fetch_single(p, &entry.url, &json_path, entry.sha1.clone(), None).await?;
    let version: VersionJson = read_required(&json_path)?;

    report.stage("assetIndex");
    let index = match &version.asset_index {
        Some(ai) => {
            let path = p.paths.assets_dir().join("indexes").join(format!("{}.json", ai.id));
            fetch_single(p, &ai.url, &path, ai.sha1.clone(), ai.size).await?;
            Some(read_required::<AssetIndex>(&path)?)
        }
        None => None,
    };

    report.stage("files");
    let mut tasks = Vec::new();
    if let Some(client) = version.downloads.get("client") {
        tasks.push(DownloadTask {
            url: client.url.clone(),
            dest: client_jar_path(p.paths, version_id),
            sha1: client.sha1.clone(),
            size: client.size,
        });
    }
    let libs_dir = p.paths.libraries_dir();
    for lib in resolve_libraries(&version.libraries, &p.env) {
        let dest = libs_dir.join(&lib.path);
        match lib.url {
            Some(url) => tasks.push(DownloadTask { url, dest, sha1: lib.sha1, size: lib.size }),
            None if !dest.exists() => {
                return Err(AppError::NotFound(format!("библиотека {} (нет ссылки для скачивания)", lib.name)))
            }
            None => {}
        }
    }
    if let Some(log) = version.logging.as_ref().and_then(|l| l.client.as_ref()) {
        tasks.push(DownloadTask {
            url: log.file.url.clone(),
            dest: p.paths.assets_dir().join("log_configs").join(&log.file.id),
            sha1: log.file.sha1.clone(),
            size: log.file.size,
        });
    }
    if let (Some(index), false) = (&index, p.skip_asset_objects) {
        tasks.extend(asset_tasks(index, &p.paths.assets_dir()));
    }

    tracing::info!("установка {version_id}: {} файлов", tasks.len());
    download_all(p.client, tasks, p.opts, p.cancel, |pr| report.progress(pr)).await?;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopReporter;
    impl Reporter for NoopReporter {
        fn stage(&self, s: &str) {
            println!("этап: {s}");
        }
        fn progress(&self, _: Progress) {}
    }

    /// Реальная загрузка с серверов Mojang (без объектов assets).
    /// Запуск: `cargo test -- --ignored install_real_version --nocapture`
    #[tokio::test]
    #[ignore = "требует сети"]
    async fn install_real_version() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths { data_dir: dir.path().to_path_buf(), portable: false };
        paths.ensure().unwrap();
        let client = crate::downloads::build_http_client(&Default::default()).unwrap();
        let cancel = CancellationToken::new();
        let params = InstallParams {
            client: &client,
            paths: &paths,
            opts: DownloadOptions { concurrency: 8, retries: 2 },
            cancel: &cancel,
            env: Env::current(),
            skip_asset_objects: true,
        };
        let v = install_version(&params, "1.20.1", &NoopReporter).await.unwrap();
        assert_eq!(v.required_java(), 17);
        assert!(client_jar_path(&paths, "1.20.1").exists());
        assert!(paths.assets_dir().join("indexes/5.json").exists());
        let libs = resolve_libraries(&v.libraries, &Env::current());
        for l in &libs {
            assert!(paths.libraries_dir().join(&l.path).exists(), "нет {}", l.name);
        }
        println!("скачано библиотек: {}", libs.len());

        // Повторная установка ничего не качает и проходит быстро.
        let t = std::time::Instant::now();
        install_version(&params, "1.20.1", &NoopReporter).await.unwrap();
        println!("повторная проверка: {:?}", t.elapsed());
    }
}
