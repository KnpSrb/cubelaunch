use std::path::PathBuf;

use tauri::{Emitter, State};

use crate::error::{AppError, AppResult};
use crate::events;
use crate::paths;
use crate::settings::Settings;
use crate::state::AppState;

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<Settings> {
    let saved = state.update_settings(settings).await?;
    let _ = app.emit(events::SETTINGS_CHANGED, &saved);
    Ok(saved)
}

#[tauri::command]
pub async fn reset_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Settings> {
    let saved = state.update_settings(Settings::default()).await?;
    let _ = app.emit(events::SETTINGS_CHANGED, &saved);
    Ok(saved)
}

/// Меняет папку данных лаунчера. `None` — вернуть папку по умолчанию.
/// Данные не переносятся автоматически; изменение вступает в силу после перезапуска.
#[tauri::command]
pub fn set_data_dir(state: State<'_, AppState>, path: Option<String>) -> AppResult<bool> {
    if state.paths.portable {
        return Err(AppError::InvalidSettings(
            "в портативном режиме папка данных фиксирована".into(),
        ));
    }
    let dir = match path {
        Some(p) => {
            let p = PathBuf::from(p);
            if !p.is_absolute() {
                return Err(AppError::InvalidSettings("путь должен быть абсолютным".into()));
            }
            std::fs::create_dir_all(&p)?;
            Some(p)
        }
        None => None,
    };
    paths::set_data_dir_override(dir.as_deref())?;
    let target = dir.or_else(paths::default_data_dir);
    Ok(target.as_deref() != Some(state.paths.data_dir.as_path()))
}
