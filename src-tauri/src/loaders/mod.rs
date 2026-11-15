//! Загрузчики модов: Fabric, Quilt, Forge, NeoForge.
//!
//! Загрузчик описывается профилем — version JSON с `inheritsFrom` на ванильную
//! версию. Профиль хранится в `versions/<profile id>/<profile id>.json` и при
//! запуске накладывается на ванильный (`VersionJson::merge_onto`).
//!
//! - Fabric и Quilt: профиль берётся из их meta-API, библиотеки — с их maven.
//! - Forge и NeoForge: официальный установщик (jar с их maven) разбирается
//!   лаунчером: профиль и библиотеки берутся из него, а процессоры (патчинг
//!   клиента) запускаются нашей Java так же, как это делает сам установщик.
//!
//! Файлы игры не распространяются: всё качается с официальных серверов
//! Mojang и проектов загрузчиков, патченый клиент собирается локально.

pub mod fabric;
pub mod forge;
pub mod meta;

use std::path::Path;

use crate::downloads::queue::{download_all, DownloadTask};
use crate::error::{AppError, AppResult};
use crate::instances::{LoaderKind, LoaderSpec};
use crate::minecraft::install::{version_json_path, InstallParams, Reporter};
use crate::minecraft::libraries::resolve_libraries;
use crate::minecraft::version::{Library, VersionJson};
use crate::paths::AppPaths;
use crate::util::{read_json_opt, write_json_atomic};

/// Проверка строки версии, которая попадает в URL и пути.
pub fn validate_token(s: &str, what: &str) -> AppResult<()> {
    let ok = !s.is_empty()
        && s.len() <= 100
        && !s.starts_with('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'));
    if ok {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!("некорректная версия {what}: {s}")))
    }
}

/// Короткая версия для показа: у Forge версии в maven вида `1.20.1-47.3.0`
/// (а у старых — `1.7.10-10.13.4.1614-1.7.10`).
pub fn short_version(mc: &str, version: &str) -> String {
    let v = version.strip_prefix(&format!("{mc}-")).unwrap_or(version);
    v.strip_suffix(&format!("-{mc}")).unwrap_or(v).to_string()
}

/// Идентификатор профиля загрузчика в папке `versions`.
pub fn profile_id(mc: &str, spec: &LoaderSpec) -> String {
    format!("{}-{}-{}", spec.kind.as_str(), mc, short_version(mc, &spec.version))
}

pub fn load_profile(paths: &AppPaths, mc: &str, spec: &LoaderSpec) -> AppResult<Option<VersionJson>> {
    read_json_opt(&version_json_path(paths, &profile_id(mc, spec)))
}

/// Сохраняет профиль под нашим id (исходный id у разных загрузчиков устроен по-разному).
pub fn save_profile(paths: &AppPaths, mc: &str, spec: &LoaderSpec, mut profile: VersionJson) -> AppResult<VersionJson> {
    profile.id = profile_id(mc, spec);
    profile.inherits_from = Some(mc.to_string());
    write_json_atomic(&version_json_path(paths, &profile.id), &profile)?;
    Ok(profile)
}

/// Качает библиотеки профиля. Библиотеки без ссылки уже должны лежать на диске.
pub async fn download_libraries(p: &InstallParams<'_>, libs: &[Library], report: &dyn Reporter) -> AppResult<()> {
    let dir = p.paths.libraries_dir();
    let mut tasks = Vec::new();
    for lib in resolve_libraries(libs, &p.env) {
        let dest = dir.join(&lib.path);
        match lib.url {
            Some(url) => tasks.push(DownloadTask { url, dest, sha1: lib.sha1, size: lib.size }),
            None if !dest.is_file() => {
                return Err(AppError::Loader(format!("нет файла библиотеки {} и ссылки для скачивания", lib.name)))
            }
            None => {}
        }
    }
    download_all(p.client, tasks, p.opts, p.cancel, |pr| report.progress(pr)).await
}

/// Ставит загрузчик поверх уже установленной ванильной версии и возвращает его профиль.
/// `java` нужна только Forge/NeoForge (для процессоров установщика).
pub async fn install_loader(
    p: &InstallParams<'_>,
    mc: &str,
    spec: &LoaderSpec,
    vanilla: &VersionJson,
    java: Option<&Path>,
    report: &dyn Reporter,
) -> AppResult<VersionJson> {
    validate_token(mc, "Minecraft")?;
    validate_token(&spec.version, spec.kind.display())?;
    match spec.kind {
        LoaderKind::Fabric | LoaderKind::Quilt => fabric::install(p, mc, spec, report).await,
        LoaderKind::Forge | LoaderKind::NeoForge => {
            let java = java.ok_or_else(|| AppError::Loader("для установки Forge/NeoForge нужна Java".into()))?;
            forge::install(p, mc, spec, vanilla, java, report).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_short_versions() {
        let f = LoaderSpec { kind: LoaderKind::Forge, version: "1.20.1-47.3.0".into() };
        assert_eq!(profile_id("1.20.1", &f), "forge-1.20.1-47.3.0");
        assert_eq!(short_version("1.7.10", "1.7.10-10.13.4.1614-1.7.10"), "10.13.4.1614");
        let q = LoaderSpec { kind: LoaderKind::Quilt, version: "0.27.1-beta.1".into() };
        assert_eq!(profile_id("1.21.1", &q), "quilt-1.21.1-0.27.1-beta.1");
    }

    /// Реальная установка загрузчиков в общую папку (без объектов assets).
    /// Запуск: `cargo test -- --ignored install_real_loaders --nocapture`.
    /// Папка задаётся `CUBELAUNCH_TEST_DIR`, Java для процессоров ищется в ней или скачивается.
    #[tokio::test]
    #[ignore = "требует сети и Java"]
    async fn install_real_loaders() {
        use crate::downloads::queue::{DownloadOptions, Progress};
        use crate::minecraft::install::install_version;
        use crate::minecraft::rules::{Arch, Env};

        struct Print;
        impl Reporter for Print {
            fn stage(&self, s: &str) {
                println!("  этап: {s}");
            }
            fn progress(&self, _: Progress) {}
        }

        let root = std::env::var("CUBELAUNCH_TEST_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("cubelaunch-loaders"));
        let paths = AppPaths { data_dir: root, portable: false };
        paths.ensure().unwrap();
        let client = crate::downloads::build_http_client(&Default::default()).unwrap();
        let cancel = tokio_util::sync::CancellationToken::new();
        let opts = DownloadOptions { concurrency: 16, retries: 2 };
        let p = InstallParams { client: &client, paths: &paths, opts, cancel: &cancel, env: Env::current(), skip_asset_objects: true };

        let cases = [
            ("1.21.1", LoaderKind::Fabric),
            ("1.21.1", LoaderKind::Quilt),
            ("1.21.1", LoaderKind::NeoForge),
            ("1.20.1", LoaderKind::Forge),
            ("1.12.2", LoaderKind::Forge),
        ];
        for (mc, kind) in cases {
            println!("== {mc} {kind:?}");
            let versions = meta::list_versions(&client, &paths.meta_dir(), kind, mc).await.unwrap();
            let pick = versions.iter().find(|v| v.recommended).or(versions.first()).expect("нет версий");
            println!("  версия загрузчика: {} ({} всего)", pick.version, versions.len());
            let spec = LoaderSpec { kind, version: pick.version.clone() };
            let vanilla = install_version(&p, mc, &Print).await.unwrap();
            let java = if kind.uses_installer() {
                let need = vanilla.required_java();
                let list = crate::java::detect::detect_all(&paths.java_dir()).await;
                let j = match crate::java::pick_best(&list, need, Arch::current()) {
                    Some(j) => j.clone(),
                    None => crate::java::download::install_temurin(&client, &paths.java_dir(), &paths.cache_dir(), need, Arch::current(), opts, &cancel, &Print)
                        .await
                        .unwrap(),
                };
                Some(std::path::PathBuf::from(j.path))
            } else {
                None
            };
            let t = std::time::Instant::now();
            let profile = install_loader(&p, mc, &spec, &vanilla, java.as_deref(), &Print).await.unwrap();
            let merged = profile.merge_onto(vanilla);
            let libs = resolve_libraries(&merged.libraries, &p.env);
            assert!(libs.iter().all(|l| paths.libraries_dir().join(&l.path).is_file()));
            println!("  готово за {:?}: {} библиотек, mainClass {}", t.elapsed(), libs.len(), merged.main_class.unwrap());
        }
    }

    #[test]
    fn rejects_path_like_versions() {
        assert!(validate_token("0.16.9", "x").is_ok());
        assert!(validate_token("1.20.1-47.3.0", "x").is_ok());
        assert!(validate_token("0.15.4+mixin.0.8.7", "x").is_ok());
        assert!(validate_token("../evil", "x").is_err());
        assert!(validate_token("a/b", "x").is_err());
        assert!(validate_token("", "x").is_err());
    }
}
