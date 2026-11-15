//! Команды содержимого инстанса: моды, ресурспаки, шейдеры, миры, Modrinth.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::content::curseforge::{self, fingerprint, CfFile, CurseForge};
use crate::content::meta::mod_compatible;
use crate::content::modrinth::{Modrinth, SearchResult, Target, Version};
use crate::content::worlds::{self, Backup, World};
use crate::content::{self, base_name, AddResult, ContentFile, ContentKind, DISABLED};
use crate::downloads::jobs::JobCtx;
use crate::downloads::queue::{download_all, DownloadTask, Progress};
use crate::error::{AppError, AppResult};
use crate::instances::Instance;
use crate::state::AppState;

pub(crate) fn kind_dir(state: &AppState, id: &str, kind: ContentKind) -> AppResult<PathBuf> {
    Ok(state.instances.game_dir(id)?.join(kind.folder()))
}

pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| AppError::Other(e.to_string()))?
}

pub(crate) async fn list_files(app: &AppHandle, id: &str, kind: ContentKind) -> AppResult<Vec<ContentFile>> {
    let dir = kind_dir(&app.state::<AppState>(), id, kind)?;
    let app = app.clone();
    blocking(move || content::list(&dir, kind, &app.state::<AppState>().content_cache)).await
}

pub(crate) fn target_for(inst: &Instance, kind: ContentKind) -> AppResult<Target> {
    Target::for_instance(kind, &inst.minecraft, inst.loader.as_ref().map(|l| l.kind.as_str()))
}

#[tauri::command]
pub async fn list_content(app: AppHandle, id: String, kind: ContentKind) -> AppResult<Vec<ContentFile>> {
    let inst = app.state::<AppState>().instances.get(&id)?;
    let mut files = list_files(&app, &id, kind).await?;
    if kind == ContentKind::Mods {
        for f in &mut files {
            let loaders = f.meta.as_ref().map(|m| m.loaders.as_slice()).unwrap_or_default();
            f.incompatible = match &inst.loader {
                Some(l) => !mod_compatible(loaders, l.kind.as_str(), &inst.minecraft),
                None => !loaders.is_empty(),
            };
        }
    }
    Ok(files)
}

#[tauri::command]
pub fn set_content_enabled(state: State<'_, AppState>, id: String, kind: ContentKind, file_name: String, enabled: bool) -> AppResult<String> {
    content::set_enabled(&kind_dir(&state, &id, kind)?, &file_name, enabled)
}

#[tauri::command]
pub fn delete_content(state: State<'_, AppState>, id: String, kind: ContentKind, file_names: Vec<String>) -> AppResult<()> {
    content::delete(&kind_dir(&state, &id, kind)?, &file_names)
}

/// Копирует файлы (drag & drop или выбор в диалоге) в папку категории.
#[tauri::command]
pub async fn add_content_files(app: AppHandle, id: String, kind: ContentKind, paths: Vec<String>) -> AppResult<AddResult> {
    let dir = kind_dir(&app.state::<AppState>(), &id, kind)?;
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    blocking(move || content::add_files(&dir, kind, &paths)).await
}

/// Открывает папку инстанса: `mods`, `resourcepacks`, `shaderpacks`, `saves`,
/// `saves/<мир>`, `screenshots` (в игровой папке) или `backups` (рядом с ней).
#[tauri::command]
pub fn open_content_folder(app: AppHandle, state: State<'_, AppState>, id: String, folder: String) -> AppResult<()> {
    let (top, rest) = folder.split_once('/').map(|(a, b)| (a, Some(b))).unwrap_or((folder.as_str(), None));
    let base = match top {
        "mods" | "resourcepacks" | "shaderpacks" | "saves" | "screenshots" => state.instances.game_dir(&id)?.join(top),
        "backups" => state.instances.dir(&id)?.join("backups"),
        _ => return Err(AppError::InvalidInput(format!("неизвестная папка {folder}"))),
    };
    let dir = match rest {
        Some(r) => content::entry_path(&base, r)?,
        None => base,
    };
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| AppError::Other(format!("не удалось открыть папку: {e}")))
}

// ---------- Modrinth ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthInfo {
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub title: String,
    pub slug: String,
    pub project_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Распознаёт файлы через Modrinth по SHA1 (с кэшем на время работы).
pub(crate) async fn identify(state: &AppState, files: &[ContentFile]) -> AppResult<HashMap<String, ModrinthInfo>> {
    let hashes: Vec<String> = files.iter().filter_map(|f| f.sha1.clone()).collect();
    let unknown: Vec<String> = {
        let cache = state.modrinth_hashes.lock().unwrap();
        hashes.iter().filter(|h| !cache.contains_key(*h)).cloned().collect()
    };
    if !unknown.is_empty() {
        let client = state.client().await;
        let m = Modrinth::new(&client);
        let versions = m.versions_by_hashes(&unknown).await?;
        let ids: Vec<String> = versions.values().map(|v| v.project_id.clone()).collect::<HashSet<_>>().into_iter().collect();
        let projects: HashMap<String, _> = m.projects(&ids).await?.into_iter().map(|p| (p.id.clone(), p)).collect();
        let mut cache = state.modrinth_hashes.lock().unwrap();
        for h in unknown {
            let info = versions.get(&h).and_then(|v| {
                let p = projects.get(&v.project_id)?;
                Some(ModrinthInfo {
                    project_id: v.project_id.clone(),
                    version_id: v.id.clone(),
                    version_number: v.version_number.clone(),
                    title: p.title.clone(),
                    slug: p.slug.clone(),
                    project_type: p.project_type.clone(),
                    icon_url: p.icon_url.clone(),
                })
            });
            cache.insert(h, info);
        }
    }
    let cache = state.modrinth_hashes.lock().unwrap();
    Ok(files
        .iter()
        .filter_map(|f| Some((f.file_name.clone(), cache.get(f.sha1.as_ref()?)?.clone()?)))
        .collect())
}

/// Какие файлы категории — проекты Modrinth (имя файла → проект).
#[tauri::command]
pub async fn content_modrinth_info(app: AppHandle, id: String, kind: ContentKind) -> AppResult<HashMap<String, ModrinthInfo>> {
    let files = list_files(&app, &id, kind).await?;
    identify(&app.state::<AppState>(), &files).await
}

#[tauri::command]
pub async fn modrinth_search(
    state: State<'_, AppState>,
    id: String,
    kind: ContentKind,
    query: String,
    sort: String,
    offset: u64,
) -> AppResult<SearchResult> {
    let inst = state.instances.get(&id)?;
    let target = target_for(&inst, kind)?;
    let client = state.client().await;
    Modrinth::new(&client).search(query.trim(), kind, &target, &sort, offset).await
}

/// Версии проекта, совместимые с инстансом (новые сверху).
#[tauri::command]
pub async fn modrinth_versions(state: State<'_, AppState>, id: String, kind: ContentKind, project: String) -> AppResult<Vec<Version>> {
    let inst = state.instances.get(&id)?;
    let target = target_for(&inst, kind)?;
    let client = state.client().await;
    let list = Modrinth::new(&client).versions(&project, &target).await?;
    Ok(list.into_iter().filter(|v| target.accepts(v)).collect())
}

/// Скачивает версии в папку категории. `replace` — файлы, которые заменяются
/// новой версией (проект → (старое имя, был ли выключен)).
pub(crate) async fn download_versions(
    ctx: &JobCtx,
    dir: &std::path::Path,
    versions: &[Version],
    replace: &HashMap<String, (String, bool)>,
) -> AppResult<Vec<String>> {
    let state = ctx.app.state::<AppState>();
    let mut tasks = Vec::new();
    let mut names = Vec::new();
    for v in versions {
        let f = v
            .primary_file()
            .ok_or_else(|| AppError::Content(format!("у версии {} нет файлов", v.version_number)))?;
        // Имя файла приходит с сервера — не даём ему выйти за пределы папки.
        if f.filename.contains(['/', '\\']) || f.filename.starts_with('.') {
            return Err(AppError::Content(format!("некорректное имя файла {}", f.filename)));
        }
        tasks.push(DownloadTask { url: f.url.clone(), dest: dir.join(&f.filename), sha1: f.hashes.sha1.clone(), size: Some(f.size).filter(|s| *s > 0) });
        names.push((v.project_id.clone(), f.filename.clone()));
    }
    ctx.stage("files");
    let client = state.client().await;
    download_all(&client, tasks, state.download_options().await, &ctx.cancel, |p| ctx.progress(p)).await?;

    let mut installed = Vec::new();
    for (project, new_name) in names {
        let mut final_name = new_name.clone();
        if let Some((old, was_disabled)) = replace.get(&project) {
            if *was_disabled {
                final_name = format!("{new_name}{DISABLED}");
                let _ = std::fs::remove_file(dir.join(&final_name));
                std::fs::rename(dir.join(&new_name), dir.join(&final_name))?;
            }
            if old != &final_name && dir.join(old).exists() {
                content::delete(dir, std::slice::from_ref(old))?;
            }
        }
        installed.push(final_name);
    }
    Ok(installed)
}

/// Ставит версию проекта Modrinth (и обязательные зависимости). Возвращает id задачи.
#[tauri::command]
pub fn install_modrinth(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    kind: ContentKind,
    version_id: String,
    title: String,
    with_deps: bool,
) -> AppResult<u64> {
    let inst = state.instances.get(&id)?;
    let target = target_for(&inst, kind)?;
    let job_title = format!("{title} → {}", inst.name);
    Ok(state.jobs.spawn(&app, "installContent", job_title, Some(id.clone()), move |ctx| async move {
        let state = ctx.app.state::<AppState>();
        ctx.stage("resolve");
        let client = state.client().await;
        let m = Modrinth::new(&client);
        let root = m.version(&version_id).await?.ok_or_else(|| AppError::NotFound(format!("версия {version_id}")))?;
        if !target.accepts(&root) {
            return Err(AppError::Content(format!(
                "версия {} не подходит для Minecraft {}{}",
                root.version_number,
                target.minecraft,
                target.loaders.as_ref().map(|l| format!(" ({})", l.join("/"))).unwrap_or_default()
            )));
        }
        // Что уже стоит: проект → (файл, выключен ли).
        let files = list_files(&ctx.app, &id, kind).await?;
        let known = identify(&state, &files).await?;
        let mut by_project: HashMap<String, (String, bool)> = HashMap::new();
        for f in &files {
            if let Some(info) = known.get(&f.file_name) {
                by_project.insert(info.project_id.clone(), (f.file_name.clone(), !f.enabled));
            }
        }
        let installed: HashSet<String> = by_project.keys().cloned().collect();
        let plan = m.plan(root, &target, &installed, with_deps && kind == ContentKind::Mods).await?;
        let replace: HashMap<String, (String, bool)> =
            plan.iter().filter_map(|v| by_project.get(&v.project_id).map(|o| (v.project_id.clone(), o.clone()))).collect();
        let dir = kind_dir(&state, &id, kind)?;
        let names = download_versions(&ctx, &dir, &plan, &replace).await?;
        tracing::info!("{}: установлено {}", id, names.join(", "));
        Ok(())
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentUpdate {
    pub file_name: String,
    pub project_id: String,
    pub title: String,
    pub current_version: String,
    pub version_id: String,
    pub version_number: String,
}

#[tauri::command]
pub async fn check_content_updates(app: AppHandle, id: String, kind: ContentKind) -> AppResult<Vec<ContentUpdate>> {
    let state = app.state::<AppState>();
    let inst = state.instances.get(&id)?;
    let target = target_for(&inst, kind)?;
    let files = list_files(&app, &id, kind).await?;
    let known = identify(&state, &files).await?;
    let hashes: Vec<String> = files.iter().filter(|f| known.contains_key(&f.file_name)).filter_map(|f| f.sha1.clone()).collect();
    let client = state.client().await;
    let latest = Modrinth::new(&client).latest_by_hashes(&hashes, &target).await?;
    let mut out = Vec::new();
    for f in &files {
        let (Some(sha1), Some(info)) = (&f.sha1, known.get(&f.file_name)) else { continue };
        let Some(new) = latest.get(sha1) else { continue };
        let same = new.files.iter().any(|nf| nf.hashes.sha1.as_deref() == Some(sha1.as_str()));
        if !same && new.id != info.version_id {
            out.push(ContentUpdate {
                file_name: f.file_name.clone(),
                project_id: info.project_id.clone(),
                title: info.title.clone(),
                current_version: info.version_number.clone(),
                version_id: new.id.clone(),
                version_number: new.version_number.clone(),
            });
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateItem {
    pub file_name: String,
    pub version_id: String,
}

/// Обновляет выбранные файлы до указанных версий (старые — в корзину).
#[tauri::command]
pub fn update_content(app: AppHandle, state: State<'_, AppState>, id: String, kind: ContentKind, items: Vec<UpdateItem>) -> AppResult<u64> {
    let inst = state.instances.get(&id)?;
    let title = format!("{} — обновление ({})", inst.name, items.len());
    Ok(state.jobs.spawn(&app, "updateContent", title, Some(id.clone()), move |ctx| async move {
        let state = ctx.app.state::<AppState>();
        ctx.stage("resolve");
        let client = state.client().await;
        let m = Modrinth::new(&client);
        let mut versions = Vec::new();
        let mut replace = HashMap::new();
        for (i, it) in items.iter().enumerate() {
            ctx.progress(Progress { files_done: i, files_total: items.len(), bytes_done: 0, bytes_total: 0 });
            let v = m.version(&it.version_id).await?.ok_or_else(|| AppError::NotFound(format!("версия {}", it.version_id)))?;
            replace.insert(v.project_id.clone(), (it.file_name.clone(), it.file_name.ends_with(DISABLED)));
            versions.push(v);
        }
        let dir = kind_dir(&state, &id, kind)?;
        download_versions(&ctx, &dir, &versions, &replace).await?;
        Ok(())
    }))
}

// ---------- CurseForge ----------

#[tauri::command]
pub async fn curseforge_search(state: State<'_, AppState>, id: String, kind: ContentKind, query: String, sort: String, offset: u64) -> AppResult<SearchResult> {
    let inst = state.instances.get(&id)?;
    let target = target_for(&inst, kind)?;
    let key = state.curseforge_key()?;
    let client = state.client().await;
    CurseForge::new(&client, &key).search(query.trim(), kind, &target, &sort, offset).await
}

#[tauri::command]
pub async fn curseforge_versions(state: State<'_, AppState>, id: String, kind: ContentKind, project: String) -> AppResult<Vec<Version>> {
    let inst = state.instances.get(&id)?;
    let target = target_for(&inst, kind)?;
    let key = state.curseforge_key()?;
    let client = state.client().await;
    let files = CurseForge::new(&client, &key).files(&project, Some(&target.minecraft)).await?;
    Ok(files.into_iter().map(CfFile::into_version).filter(|v| curseforge::accepts(&target, kind, v)).collect())
}

/// Проекты CurseForge, уже лежащие в папке (по отпечаткам): проект → (файл, выключен ли).
async fn curseforge_installed(app: &AppHandle, cf: &CurseForge<'_>, id: &str, kind: ContentKind) -> AppResult<HashMap<String, (String, bool)>> {
    let dir = kind_dir(&app.state::<AppState>(), id, kind)?;
    let files = list_files(app, id, kind).await?;
    let entries: Vec<(String, bool)> = files.iter().filter(|f| !f.is_dir).map(|f| (f.file_name.clone(), !f.enabled)).collect();
    let names = entries.clone();
    let prints = blocking(move || names.iter().map(|(n, _)| Ok(fingerprint(&std::fs::read(dir.join(n))?))).collect::<AppResult<Vec<u32>>>()).await?;
    let found = cf.by_fingerprints(&prints).await?;
    Ok(entries
        .into_iter()
        .zip(prints)
        .filter_map(|(e, p)| found.get(&p).map(|m| (m.id.to_string(), e)))
        .collect())
}

/// Ставит файл проекта CurseForge (и обязательные зависимости). Возвращает id задачи.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn install_curseforge(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    kind: ContentKind,
    project_id: String,
    file_id: String,
    title: String,
    with_deps: bool,
) -> AppResult<u64> {
    let inst = state.instances.get(&id)?;
    let target = target_for(&inst, kind)?;
    let key = state.curseforge_key()?;
    let job_title = format!("{title} → {}", inst.name);
    Ok(state.jobs.spawn(&app, "installContent", job_title, Some(id.clone()), move |ctx| async move {
        let state = ctx.app.state::<AppState>();
        ctx.stage("resolve");
        let client = state.client().await;
        let cf = CurseForge::new(&client, &key);
        let root = cf.file(&project_id, &file_id).await?.ok_or_else(|| AppError::NotFound(format!("файл {file_id}")))?.into_version();
        if !curseforge::accepts(&target, kind, &root) {
            return Err(AppError::Content(format!("файл {} не подходит для Minecraft {}", root.version_number, target.minecraft)));
        }
        let installed = curseforge_installed(&ctx.app, &cf, &id, kind).await?;
        // Обязательные зависимости (рекурсивно), уже стоящие проекты пропускаем.
        let mut plan = Vec::new();
        let mut seen: HashSet<String> = HashSet::from([root.project_id.clone()]);
        let mut queue = std::collections::VecDeque::from([root]);
        while let Some(v) = queue.pop_front() {
            if with_deps && kind == ContentKind::Mods {
                for dep in v.dependencies.iter().filter(|d| d.dependency_type == "required") {
                    let Some(p) = dep.project_id.clone() else { continue };
                    if installed.contains_key(&p) || !seen.insert(p.clone()) {
                        continue;
                    }
                    match cf.best_file(&p, kind, &target).await? {
                        Some(f) => queue.push_back(f),
                        None => {
                            let name = cf.mod_info(&p).await.ok().flatten().map(|m| m.name).unwrap_or(p);
                            return Err(AppError::Content(format!("нет версии обязательной зависимости «{name}» для Minecraft {}", target.minecraft)));
                        }
                    }
                }
            }
            plan.push(v);
        }
        for v in &plan {
            if v.files.iter().all(|f| f.url.is_empty()) {
                let page = cf.mod_info(&v.project_id).await.ok().flatten().map(|m| format!("{}/files/{}", m.page_url(), v.id)).unwrap_or_default();
                return Err(AppError::Content(format!(
                    "автор «{}» запретил скачивание из сторонних лаунчеров — скачайте файл вручную и перетащите в окно: {page}",
                    v.version_number
                )));
            }
        }
        let replace: HashMap<String, (String, bool)> =
            plan.iter().filter_map(|v| installed.get(&v.project_id).map(|o| (v.project_id.clone(), o.clone()))).collect();
        let dir = kind_dir(&state, &id, kind)?;
        let names = download_versions(&ctx, &dir, &plan, &replace).await?;
        tracing::info!("{}: установлено с CurseForge {}", id, names.join(", "));
        Ok(())
    }))
}

// ---------- Миры ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldsInfo {
    pub worlds: Vec<World>,
    pub backups: Vec<Backup>,
}

#[tauri::command]
pub async fn list_worlds(state: State<'_, AppState>, id: String) -> AppResult<WorldsInfo> {
    let saves = state.instances.game_dir(&id)?.join("saves");
    let backups = state.instances.dir(&id)?.join("backups");
    blocking(move || Ok(WorldsInfo { worlds: worlds::list_worlds(&saves), backups: worlds::list_backups(&backups) })).await
}

#[tauri::command]
pub fn backup_world(app: AppHandle, state: State<'_, AppState>, id: String, folder: String) -> AppResult<u64> {
    let saves = state.instances.game_dir(&id)?.join("saves");
    let world = content::entry_path(&saves, &folder)?;
    let backups = state.instances.dir(&id)?.join("backups");
    let title = format!("Резервная копия «{}»", base_name(&folder));
    Ok(state.jobs.spawn(&app, "backupWorld", title, Some(id), move |ctx| async move {
        ctx.stage("archive");
        let c = ctx.clone();
        let path = blocking(move || {
            worlds::backup_world(&world, &backups, &c.cancel, |done, total| {
                c.progress(Progress { files_done: done, files_total: total, bytes_done: 0, bytes_total: 0 })
            })
        })
        .await?;
        tracing::info!("резервная копия мира: {}", path.display());
        Ok(())
    }))
}

#[tauri::command]
pub fn delete_world(state: State<'_, AppState>, id: String, folder: String) -> AppResult<()> {
    if state.games.is_running(&id) {
        return Err(AppError::InvalidInput("сначала закройте игру".into()));
    }
    content::delete(&state.instances.game_dir(&id)?.join("saves"), &[folder])
}
