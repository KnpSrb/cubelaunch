//! Самообновление лаунчера (плагин Tauri updater).
//!
//! Манифест `latest.json` лежит в последнем релизе GitHub; адрес и публичный ключ
//! подписи заданы в `tauri.conf.json`. Обновление ставится только по кнопке и
//! только когда не запущена игра и нет активных задач.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::downloads::queue::Progress;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Найденное обновление (ждёт, пока пользователь нажмёт «Установить»).
#[derive(Default)]
pub struct PendingUpdate(std::sync::Mutex<Option<Update>>);

/// Почему обновление нельзя поставить автоматически (тогда UI даёт ссылку на релиз).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ManualReason {
    /// Отладочная сборка (`tauri dev`).
    Dev,
    /// Портативный режим: установщик поставил бы лаунчер в другое место.
    Portable,
    /// Запущен не из установщика (собранный вручную бинарник).
    Unpackaged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub version: String,
    /// Описание релиза (markdown из GitHub).
    pub notes: Option<String>,
    /// Дата публикации (RFC 3339).
    pub date: Option<String>,
    pub manual: Option<ManualReason>,
    /// Страница релизов — для ручной установки.
    pub release_url: Option<String>,
}

fn endpoint(app: &AppHandle) -> Option<String> {
    app.config()
        .plugins
        .0
        .get("updater")?
        .get("endpoints")?
        .get(0)?
        .as_str()
        .map(str::to_string)
}

/// `…/releases/latest/download/latest.json` → `…/releases/latest`.
fn release_page(endpoint: &str) -> Option<String> {
    endpoint
        .split_once("/download/")
        .map(|(base, _)| base.to_string())
        .filter(|s| s.starts_with("https://"))
}

fn manual_reason(state: &AppState) -> Option<ManualReason> {
    if cfg!(debug_assertions) {
        Some(ManualReason::Dev)
    } else if state.paths.portable {
        Some(ManualReason::Portable)
    } else if tauri::utils::platform::bundle_type().is_none() {
        Some(ManualReason::Unpackaged)
    } else {
        None
    }
}

fn updater_err(e: tauri_plugin_updater::Error) -> AppError {
    AppError::Update(e.to_string())
}

/// Проверяет наличие обновления. `None` — стоит последняя версия.
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingUpdate>,
) -> AppResult<Option<UpdateInfo>> {
    let url = endpoint(&app).unwrap_or_default();
    if url.is_empty() || url.contains("OWNER/REPO") {
        return Err(AppError::Update("адрес обновлений не задан в этой сборке".into()));
    }
    let mut builder = app.updater_builder();
    // Прокси из настроек лаунчера (системный подхватывается и так).
    {
        use crate::settings::ProxyMode;
        let p = state.settings.read().await.proxy.clone();
        match p.mode {
            ProxyMode::System => {}
            ProxyMode::None => builder = builder.no_proxy(),
            ProxyMode::Http | ProxyMode::Socks5 => {
                let scheme = if p.mode == ProxyMode::Http { "http" } else { "socks5h" };
                if let Ok(u) = format!("{scheme}://{}:{}", p.host, p.port).parse() {
                    builder = builder.proxy(u);
                }
            }
        }
    }
    let update = builder.build().map_err(updater_err)?.check().await.map_err(updater_err)?;
    let Some(update) = update else {
        *pending.0.lock().unwrap() = None;
        return Ok(None);
    };
    tracing::info!("доступно обновление лаунчера: {} → {}", update.current_version, update.version);
    let info = UpdateInfo {
        current: update.current_version.clone(),
        version: update.version.clone(),
        notes: update.body.clone().filter(|s| !s.trim().is_empty()),
        date: update.raw_json.get("pub_date").and_then(|v| v.as_str()).map(str::to_string),
        manual: manual_reason(&state),
        release_url: release_page(&url),
    };
    *pending.0.lock().unwrap() = Some(update);
    Ok(Some(info))
}

/// Почему сейчас нельзя ставить обновление (идёт игра или задачи).
fn ensure_quiet(state: &AppState, own_job: Option<u64>) -> AppResult<()> {
    if !state.games.running().is_empty() {
        return Err(AppError::Update("закройте игру перед обновлением лаунчера".into()));
    }
    if state.jobs.list().iter().any(|j| j.state.is_active() && Some(j.id) != own_job) {
        return Err(AppError::Update("дождитесь окончания загрузок и других задач".into()));
    }
    Ok(())
}

/// Скачивает и ставит найденное обновление, затем перезапускает лаунчер.
/// Возвращает id задачи (прогресс виден в центре загрузок).
#[tauri::command]
pub fn install_update(app: AppHandle, state: State<'_, AppState>, pending: State<'_, PendingUpdate>) -> AppResult<u64> {
    if let Some(r) = manual_reason(&state) {
        return Err(AppError::Update(format!("обновление в этом режиме ставится вручную ({r:?})")));
    }
    let update = pending
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| AppError::Update("сначала проверьте наличие обновления".into()))?;
    ensure_quiet(&state, None)?;
    let title = format!("Cubelaunch {}", update.version);
    Ok(state.jobs.clone().spawn(&app, "selfUpdate", title, None, move |ctx| async move {
        ctx.stage("download");
        let mut progress = Progress { files_total: 1, ..Default::default() };
        let bytes = tokio::select! {
            r = update.download(
                |chunk, total| {
                    progress.bytes_done += chunk as u64;
                    progress.bytes_total = total.unwrap_or(0);
                    ctx.progress(progress);
                },
                || {},
            ) => r.map_err(updater_err)?,
            _ = ctx.cancel.cancelled() => return Err(AppError::Cancelled),
        };
        // Пока качали, могли запустить игру.
        let state = ctx.app.state::<AppState>();
        ensure_quiet(&state, Some(ctx.id))?;
        ctx.stage("install");
        tracing::info!("устанавливается обновление {}", update.version);
        // На Windows установщик NSIS сам закрывает лаунчер и запускает новую версию.
        update.install(bytes).map_err(updater_err)?;
        ctx.app.restart();
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_page_from_endpoint() {
        assert_eq!(
            release_page("https://github.com/a/b/releases/latest/download/latest.json").as_deref(),
            Some("https://github.com/a/b/releases/latest")
        );
        assert_eq!(release_page("http://127.0.0.1/download/latest.json"), None);
        assert_eq!(release_page("https://example.com/latest.json"), None);
    }
}
