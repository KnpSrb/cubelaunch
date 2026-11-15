use serde::Serialize;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub portable: bool,
    pub os: &'static str,
    pub arch: &'static str,
}

#[tauri::command]
pub fn app_info(app: tauri::AppHandle, state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        // Версия из tauri.conf.json — та же, с которой сравнивает updater.
        version: app.package_info().version.to_string(),
        data_dir: state.paths.data_dir.display().to_string(),
        portable: state.paths.portable,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// Какие папки лаунчера можно открыть из UI.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LauncherFolder {
    Data,
    Instances,
    Logs,
    Java,
}

#[tauri::command]
pub fn open_launcher_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    folder: LauncherFolder,
) -> AppResult<()> {
    let p = &state.paths;
    let path = match folder {
        LauncherFolder::Data => p.data_dir.clone(),
        LauncherFolder::Instances => p.instances_dir(),
        LauncherFolder::Logs => p.logs_dir(),
        LauncherFolder::Java => p.java_dir(),
    };
    std::fs::create_dir_all(&path)?;
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| AppError::Other(format!("не удалось открыть папку: {e}")))
}
