use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::java::download::install_temurin;
use crate::java::{probe, recommended_max_memory_mb, JavaInstall};
use crate::minecraft::rules::Arch;
use crate::state::AppState;

#[tauri::command]
pub async fn detect_java(state: State<'_, AppState>, force: bool) -> AppResult<Vec<JavaInstall>> {
    Ok(state.java_installs(force).await)
}

/// Проверка конкретного исполняемого файла java.
#[tauri::command]
pub async fn test_java(state: State<'_, AppState>, path: String) -> AppResult<JavaInstall> {
    probe(Path::new(path.trim()), Some(&state.paths.java_dir())).await
}

/// Скачивает Temurin указанной версии. Возвращает id задачи.
#[tauri::command]
pub fn download_java(app: AppHandle, state: State<'_, AppState>, major: u32) -> AppResult<u64> {
    if !(8..=99).contains(&major) {
        return Err(AppError::InvalidInput(format!("некорректная версия Java: {major}")));
    }
    Ok(state.jobs.spawn(&app, "downloadJava", format!("Java {major} (Temurin)"), None, move |ctx| async move {
        let state = ctx.app.state::<AppState>();
        install_temurin(
            &state.client().await,
            &state.paths.java_dir(),
            &state.paths.cache_dir(),
            major,
            Arch::current(),
            state.download_options().await,
            &ctx.cancel,
            &ctx,
        )
        .await?;
        state.invalidate_java_cache().await;
        Ok(())
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub total_memory_mb: u64,
    pub recommended_max_memory_mb: u64,
    pub cpus: usize,
}

#[tauri::command]
pub fn system_info() -> SystemInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total = sys.total_memory() / 1024 / 1024;
    SystemInfo {
        total_memory_mb: total,
        recommended_max_memory_mb: recommended_max_memory_mb(total),
        cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
    }
}
