//! Fabric и Quilt: готовый профиль из meta-API + библиотеки с их maven.

use super::meta::{FABRIC_META, QUILT_META};
use super::{download_libraries, load_profile, save_profile};
use crate::error::{AppError, AppResult};
use crate::instances::{LoaderKind, LoaderSpec};
use crate::minecraft::install::{InstallParams, Reporter};
use crate::minecraft::version::VersionJson;

pub fn profile_url(kind: LoaderKind, mc: &str, loader: &str) -> String {
    match kind {
        LoaderKind::Quilt => format!("{QUILT_META}/versions/loader/{mc}/{loader}/profile/json"),
        _ => format!("{FABRIC_META}/versions/loader/{mc}/{loader}/profile/json"),
    }
}

pub async fn install(p: &InstallParams<'_>, mc: &str, spec: &LoaderSpec, report: &dyn Reporter) -> AppResult<VersionJson> {
    report.stage("loaderProfile");
    let profile = match load_profile(p.paths, mc, spec)? {
        Some(v) => v,
        None => {
            let url = profile_url(spec.kind, mc, &spec.version);
            let resp = p.client.get(&url).send().await?;
            if matches!(resp.status().as_u16(), 400 | 404) {
                return Err(AppError::Loader(format!(
                    "{} {} не поддерживает Minecraft {mc}",
                    spec.kind.display(),
                    spec.version
                )));
            }
            let profile: VersionJson = resp.error_for_status()?.json().await?;
            if profile.inherits_from.as_deref().is_some_and(|i| i != mc) {
                return Err(AppError::Loader(format!("профиль {} рассчитан на другую версию Minecraft", profile.id)));
            }
            save_profile(p.paths, mc, spec, profile)?
        }
    };
    report.stage("loaderLibraries");
    download_libraries(p, &profile.libraries, report).await?;
    Ok(profile)
}
