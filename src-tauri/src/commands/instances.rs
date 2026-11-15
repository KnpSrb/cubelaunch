use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, AppResult};
use crate::events;
use crate::instances::{Instance, InstanceOverrides, LoaderKind, LoaderSpec};
use crate::loaders::meta::{list_versions, LoaderVersion};
use crate::loaders::{short_version, validate_token};
use crate::minecraft::launch::install_instance_files;
use crate::minecraft::logparse::LogLine;
use crate::minecraft::rules::Env;
use crate::minecraft::session::GameStatus;
use crate::state::AppState;

pub fn notify_changed(app: &AppHandle) {
    let _ = app.emit(events::INSTANCES_CHANGED, ());
}

#[tauri::command]
pub fn list_instances(state: State<'_, AppState>) -> AppResult<Vec<Instance>> {
    state.instances.list()
}

#[tauri::command]
pub fn get_instance(state: State<'_, AppState>, id: String) -> AppResult<Instance> {
    state.instances.get(&id)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceArgs {
    pub name: String,
    pub group: Option<String>,
    pub minecraft: String,
    #[serde(default)]
    pub loader: Option<LoaderSpec>,
}

fn validate_loader(minecraft: &str, loader: &Option<LoaderSpec>) -> AppResult<()> {
    if let Some(l) = loader {
        validate_token(minecraft, "Minecraft")?;
        validate_token(&l.version, l.kind.display())?;
    }
    Ok(())
}

/// Создаёт инстанс и сразу запускает установку файлов версии.
#[tauri::command]
pub fn create_instance(app: AppHandle, state: State<'_, AppState>, args: CreateInstanceArgs) -> AppResult<Instance> {
    validate_loader(&args.minecraft, &args.loader)?;
    let inst = state.instances.create(&args.name, args.group, &args.minecraft, args.loader)?;
    tracing::info!("создан инстанс {} ({})", inst.id, inst.minecraft);
    notify_changed(&app);
    start_install(&app, &state, &inst)?;
    Ok(inst)
}

/// (Пере)устанавливает файлы версии инстанса. Возвращает id задачи.
#[tauri::command]
pub fn install_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<u64> {
    let inst = state.instances.get(&id)?;
    start_install(&app, &state, &inst)
}

fn ensure_idle(state: &AppState, id: &str) -> AppResult<()> {
    if state.jobs.running_for_instance(id) {
        return Err(AppError::InvalidInput("для этого инстанса уже выполняется задача".into()));
    }
    Ok(())
}

fn start_install(app: &AppHandle, state: &AppState, inst: &Instance) -> AppResult<u64> {
    ensure_idle(state, &inst.id)?;
    let id = inst.id.clone();
    let loader = inst
        .loader
        .as_ref()
        .map(|l| format!(" + {} {}", l.kind.display(), short_version(&inst.minecraft, &l.version)))
        .unwrap_or_default();
    let title = format!("{} — Minecraft {}{loader}", inst.name, inst.minecraft);
    Ok(state.jobs.spawn(app, "installVersion", title, Some(id.clone()), move |ctx| async move {
        let state = ctx.app.state::<AppState>();
        let inst = match state.instances.get(&id) {
            Ok(i) => i,
            // Инстанс удалили, пока задача ждала очереди.
            Err(AppError::NotFound(_)) => return Ok(()),
            Err(e) => return Err(e),
        };
        install_instance_files(&state, &ctx, &inst, Env::current()).await?;
        Ok(())
    }))
}

/// Версии загрузчика для версии Minecraft (новые сверху).
#[tauri::command]
pub async fn loader_versions(
    state: State<'_, AppState>,
    kind: LoaderKind,
    minecraft: String,
) -> AppResult<Vec<LoaderVersion>> {
    let client = state.client().await;
    list_versions(&client, &state.paths.meta_dir(), kind, &minecraft).await
}

/// Меняет версию Minecraft и/или загрузчик инстанса и запускает установку.
/// Моды и миры остаются в папке инстанса.
#[tauri::command]
pub fn set_instance_version(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    minecraft: String,
    loader: Option<LoaderSpec>,
) -> AppResult<Instance> {
    let minecraft = minecraft.trim().to_string();
    if minecraft.is_empty() {
        return Err(AppError::InvalidInput("не выбрана версия Minecraft".into()));
    }
    validate_loader(&minecraft, &loader)?;
    ensure_idle(&state, &id)?;
    if state.games.is_running(&id) {
        return Err(AppError::InvalidInput("сначала остановите игру".into()));
    }
    let inst = state.instances.update(&id, |i| {
        i.minecraft = minecraft;
        i.loader = loader;
        i.installed = false;
    })?;
    tracing::info!("инстанс {id}: Minecraft {}, загрузчик {:?}", inst.minecraft, inst.loader);
    notify_changed(&app);
    start_install(&app, &state, &inst)?;
    Ok(inst)
}

/// Запускает игру (с подготовкой: Java, файлы, natives). Возвращает id задачи подготовки.
#[tauri::command]
pub fn launch_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<u64> {
    ensure_idle(&state, &id)?;
    if state.games.is_running(&id) {
        return Err(AppError::AlreadyRunning);
    }
    if state.jobs.list().iter().any(|j| j.kind == "selfUpdate" && j.state.is_active()) {
        return Err(AppError::Update("идёт обновление лаунчера".into()));
    }
    // Проверяем аккаунт сразу, чтобы UI мог предложить его добавить.
    state.accounts.default_account()?;
    let inst = state.instances.get(&id)?;
    let title = format!("Запуск «{}»", inst.name);
    Ok(state.jobs.spawn(&app, "launch", title, Some(id.clone()), move |ctx| async move {
        crate::minecraft::launch::launch(&ctx, &id).await
    }))
}

#[tauri::command]
pub fn kill_game(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let s = state.games.get(&id).ok_or_else(|| AppError::NotFound(format!("запущенная игра {id}")))?;
    s.kill.cancel();
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLog {
    pub status: Option<GameStatus>,
    pub lines: Vec<LogLine>,
}

#[tauri::command]
pub fn get_game_log(state: State<'_, AppState>, id: String) -> GameLog {
    match state.games.get(&id) {
        Some(s) => GameLog { status: Some(s.status.lock().unwrap().clone()), lines: s.lines() },
        None => GameLog { status: None, lines: vec![] },
    }
}

#[tauri::command]
pub fn running_games(state: State<'_, AppState>) -> Vec<GameStatus> {
    state.games.running()
}

/// Сохраняет лог последнего запуска в файл (путь выбирает пользователь).
#[tauri::command]
pub fn save_game_log(state: State<'_, AppState>, id: String, path: String) -> AppResult<()> {
    let s = state.games.get(&id).ok_or_else(|| AppError::NotFound(format!("лог инстанса {id}")))?;
    let mut out = String::new();
    for l in s.lines() {
        let level = format!("{:?}", l.level).to_uppercase();
        match (&l.time, &l.thread) {
            (Some(t), Some(th)) => out.push_str(&format!("[{t}] [{th}/{level}]: {}\n", l.text)),
            (Some(t), None) => out.push_str(&format!("[{t}] [{level}]: {}\n", l.text)),
            _ => out.push_str(&format!("{}\n", l.text)),
        }
    }
    std::fs::write(path, out)?;
    Ok(())
}

#[tauri::command]
pub fn update_instance_overrides(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    overrides: InstanceOverrides,
) -> AppResult<Instance> {
    if let (Some(min), Some(max)) = (overrides.min_memory_mb, overrides.max_memory_mb) {
        if max < min {
            return Err(AppError::InvalidInput("максимальная память меньше минимальной".into()));
        }
    }
    let inst = state.instances.update(&id, |i| i.overrides = overrides)?;
    notify_changed(&app);
    Ok(inst)
}

#[tauri::command]
pub fn rename_instance(app: AppHandle, state: State<'_, AppState>, id: String, name: String) -> AppResult<Instance> {
    let inst = state.instances.rename(&id, &name)?;
    notify_changed(&app);
    Ok(inst)
}

#[tauri::command]
pub fn set_instance_group(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    group: Option<String>,
) -> AppResult<Instance> {
    let inst = state.instances.set_group(&id, group)?;
    notify_changed(&app);
    Ok(inst)
}

#[tauri::command]
pub fn delete_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    if state.jobs.running_for_instance(&id) || state.games.is_running(&id) {
        return Err(AppError::InvalidInput("сначала остановите игру и задачи этого инстанса".into()));
    }
    state.instances.delete(&id)?;
    tracing::info!("инстанс {id} перемещён в корзину");
    notify_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn open_instance_folder(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let dir = state.instances.game_dir(&id)?;
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| AppError::Other(format!("не удалось открыть папку: {e}")))
}
