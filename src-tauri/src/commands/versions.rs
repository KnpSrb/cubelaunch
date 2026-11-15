use tauri::State;

use crate::downloads::jobs::JobInfo;
use crate::error::AppResult;
use crate::minecraft::manifest::{get_manifest, VersionManifest};
use crate::state::AppState;

#[tauri::command]
pub async fn get_version_manifest(state: State<'_, AppState>, force: bool) -> AppResult<VersionManifest> {
    let client = state.client().await;
    get_manifest(&client, &state.paths.meta_dir(), force).await
}

#[tauri::command]
pub fn list_jobs(state: State<'_, AppState>) -> Vec<JobInfo> {
    state.jobs.list()
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, id: u64) -> bool {
    state.jobs.cancel(id)
}

#[tauri::command]
pub fn dismiss_job(state: State<'_, AppState>, id: u64) {
    state.jobs.dismiss(id)
}

/// Повторяет упавшую или отменённую задачу. Возвращает id новой задачи.
#[tauri::command]
pub fn retry_job(app: tauri::AppHandle, state: State<'_, AppState>, id: u64) -> crate::error::AppResult<u64> {
    state.jobs.retry(&app, id)
}
