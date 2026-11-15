//! Группы, клонирование, экспорт и импорт инстанса целиком.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use super::content::blocking;
use super::instances::notify_changed;
use super::modpacks::resolve_loader;
use crate::downloads::queue::Progress;
use crate::error::{AppError, AppResult};
use crate::instances::archive::{self, ArchiveKind, InstanceArchive};
use crate::instances::{Instance, LoaderSpec};
use crate::loaders::validate_token;
use crate::minecraft::launch::install_instance_files;
use crate::minecraft::rules::Env;
use crate::modpacks::export::ExportCandidate;
use crate::modpacks::{ManagedState, STATE_FILE};
use crate::state::AppState;

fn files_progress(done: usize, total: usize) -> Progress {
    Progress { files_done: done, files_total: total, bytes_done: 0, bytes_total: 0 }
}

// ---------- Группы ----------

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    state.groups.list(&state.instances)
}

#[tauri::command]
pub fn set_group_order(app: AppHandle, state: State<'_, AppState>, order: Vec<String>) -> AppResult<Vec<String>> {
    let out = state.groups.set_order(&state.instances, order)?;
    notify_changed(&app);
    Ok(out)
}

#[tauri::command]
pub fn create_group(app: AppHandle, state: State<'_, AppState>, name: String) -> AppResult<Vec<String>> {
    let out = state.groups.create(&state.instances, &name)?;
    notify_changed(&app);
    Ok(out)
}

#[tauri::command]
pub fn rename_group(app: AppHandle, state: State<'_, AppState>, old: String, new: String) -> AppResult<Vec<String>> {
    let out = state.groups.rename(&state.instances, &old, &new)?;
    notify_changed(&app);
    Ok(out)
}

#[tauri::command]
pub fn delete_group(app: AppHandle, state: State<'_, AppState>, name: String) -> AppResult<Vec<String>> {
    let out = state.groups.delete(&state.instances, &name)?;
    notify_changed(&app);
    Ok(out)
}

// ---------- Клонирование ----------

/// Создаёт копию инстанса (настройки, модпак, игровая папка; миры — по выбору).
/// Файлы копируются фоновой задачей `cloneInstance`.
#[tauri::command]
pub fn clone_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
    group: Option<String>,
    copy_saves: bool,
) -> AppResult<Instance> {
    let src = state.instances.get(&id)?;
    let inst = state.instances.create(&name, group, &src.minecraft, src.loader.clone())?;
    let inst = state.instances.update(&inst.id, |i| {
        i.icon = src.icon.clone();
        i.overrides = src.overrides.clone();
        i.modpack = src.modpack.clone();
        // Файлы версии общие для всех инстансов — копия сразу готова к запуску.
        i.installed = src.installed;
    })?;
    let src_state = state.instances.dir(&id)?.join(STATE_FILE);
    if src_state.exists() {
        std::fs::copy(&src_state, state.instances.dir(&inst.id)?.join(STATE_FILE))?;
    }
    tracing::info!("клон {} → {} (миры: {copy_saves})", id, inst.id);
    notify_changed(&app);
    let (from, to) = (state.instances.game_dir(&id)?, state.instances.game_dir(&inst.id)?);
    let title = format!("Копия «{}» → «{}»", src.name, inst.name);
    state.jobs.spawn(&app, "cloneInstance", title, Some(inst.id.clone()), move |ctx| async move {
        ctx.stage("copyFiles");
        let c = ctx.clone();
        let skip: &[&str] = if copy_saves { &[] } else { &["saves"] };
        blocking(move || archive::copy_game(&from, &to, skip, &c.cancel, |d, t| c.progress(files_progress(d, t)))).await
    });
    Ok(inst)
}

// ---------- Экспорт инстанса ----------

#[tauri::command]
pub fn instance_export_candidates(state: State<'_, AppState>, id: String) -> AppResult<Vec<ExportCandidate>> {
    Ok(archive::candidates(&state.instances.game_dir(&id)?))
}

#[tauri::command]
pub fn export_instance(app: AppHandle, state: State<'_, AppState>, id: String, path: String, include: Vec<String>) -> AppResult<u64> {
    let inst = state.instances.get(&id)?;
    let out = PathBuf::from(&path);
    if !out.is_absolute() {
        return Err(AppError::InvalidInput("путь должен быть абсолютным".into()));
    }
    let game = state.instances.game_dir(&id)?;
    let inst_dir = state.instances.dir(&id)?;
    let title = format!("Экспорт инстанса «{}»", inst.name);
    Ok(state.jobs.spawn(&app, "exportInstance", title, Some(id), move |ctx| async move {
        ctx.stage("archive");
        let c = ctx.clone();
        blocking(move || {
            let files = archive::collect(&game, &include)?;
            let managed = inst_dir.join(STATE_FILE).exists().then(|| ManagedState::load(&inst_dir)).transpose()?;
            archive::write(&out, &inst, managed.as_ref(), &files, &c.cancel, |d, t| c.progress(files_progress(d, t)))
        })
        .await
    }))
}

// ---------- Импорт инстанса (вызывается из import_modpack) ----------

/// Создаёт инстанс из архива Cubelaunch или Prism и запускает распаковку и установку.
pub async fn import_archive(app: &AppHandle, path: PathBuf, a: InstanceArchive, name: Option<String>, group: Option<String>) -> AppResult<Instance> {
    let state = app.state::<AppState>();
    validate_token(&a.minecraft, "Minecraft")?;
    let loader: Option<LoaderSpec> = match a.kind {
        ArchiveKind::Cubelaunch => a.instance.as_ref().and_then(|i| i.loader.clone()),
        ArchiveKind::Prism => resolve_loader(&state, &a.minecraft, &a.loader).await?,
    };
    if let Some(l) = &loader {
        validate_token(&l.version, l.kind.display())?;
    }
    let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| a.name.clone());
    let inst = state.instances.create(&name, group, &a.minecraft, loader)?;
    let src = a.instance.clone();
    let overrides = a.overrides.clone();
    let inst = state.instances.update(&inst.id, |i| {
        i.overrides = overrides;
        i.overrides.java_path = None;
        if let Some(s) = &src {
            i.icon = s.icon.clone();
            i.modpack = s.modpack.clone();
        }
    })?;
    if let Some(m) = &a.managed {
        m.save(&state.instances.dir(&inst.id)?)?;
    }
    if !a.warnings.is_empty() {
        tracing::warn!("импорт {}: не переносится — {}", inst.id, a.warnings.join(", "));
    }
    tracing::info!("импорт инстанса {:?} → {}", a.kind, inst.id);
    notify_changed(app);

    let id = inst.id.clone();
    let game = state.instances.game_dir(&id)?;
    let prefix = a.game_prefix.clone();
    let title = format!("Импорт «{}»", inst.name);
    state.jobs.spawn(app, "importInstance", title, Some(id.clone()), move |ctx| async move {
        let state = ctx.app.state::<AppState>();
        ctx.stage("extract");
        let c = ctx.clone();
        blocking(move || archive::extract_game(&path, &prefix, &game, &c.cancel, |d, t| c.progress(files_progress(d, t)))).await?;
        let inst = state.instances.get(&id)?;
        install_instance_files(&state, &ctx, &inst, Env::current()).await?;
        Ok(())
    });
    Ok(inst)
}
