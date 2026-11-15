//! Команды модпаков: поиск (Modrinth, CurseForge), установка как нового инстанса,
//! импорт из файла, обновление, экспорт; ключ CurseForge.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::content::blocking;
use super::instances::notify_changed;
use crate::content::curseforge::{folder_for_class, fingerprint, CfFile, CfMod, CurseForge, PackFilter};
use crate::content::modrinth::{Modrinth, SearchResult, Version};
use crate::downloads::jobs::JobCtx;
use crate::downloads::queue::{download_all, DownloadTask, Progress};
use crate::error::{AppError, AppResult};
use crate::instances::archive::{self as instance_archive, ArchiveKind};
use crate::instances::{Instance, LoaderKind, LoaderSpec};
use crate::loaders::meta::{list_versions, neoforge_minecraft};
use crate::loaders::{short_version, validate_token};
use crate::minecraft::launch::install_instance_files;
use crate::minecraft::rules::Env;
use crate::modpacks::apply::{apply, sha1_path, NewEntry};
use crate::modpacks::export::{self, ExportCandidate, ExportMeta, MrRemote};
use crate::modpacks::format::{count_overrides, extract_overrides, read_index, PackFile, PackFormat, PackIndex};
use crate::modpacks::{find_downloaded, join_rel, safe_rel, ManagedState, MissingFile, ModpackLink, PackSource, STAGING_DIR};
use crate::state::AppState;

// ---------- Ключ CurseForge ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseforgeStatus {
    /// CurseForge доступен (есть свой или вшитый ключ).
    pub configured: bool,
    pub user_key: bool,
    pub built_in: bool,
}

fn cf_status(state: &AppState) -> CurseforgeStatus {
    let user_key = state.curseforge_user_key().ok().flatten().is_some();
    let built_in = crate::content::curseforge::BUILT_IN_KEY.is_some_and(|k| !k.trim().is_empty());
    CurseforgeStatus { configured: user_key || built_in, user_key, built_in }
}

#[tauri::command]
pub fn curseforge_status(state: State<'_, AppState>) -> CurseforgeStatus {
    cf_status(&state)
}

/// Сохраняет ключ CurseForge в keyring (предварительно проверив его). `None` — удалить.
#[tauri::command]
pub async fn set_curseforge_key(state: State<'_, AppState>, key: Option<String>) -> AppResult<CurseforgeStatus> {
    match key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(k) => {
            let client = state.client().await;
            CurseForge::new(&client, &k).check_key().await?;
            state.set_curseforge_user_key(Some(&k))?;
        }
        None => state.set_curseforge_user_key(None)?,
    }
    Ok(cf_status(&state))
}

// ---------- Поиск ----------

#[tauri::command]
pub async fn modpack_search(
    state: State<'_, AppState>,
    source: PackSource,
    query: String,
    sort: String,
    offset: u64,
    minecraft: Option<String>,
    loader: Option<LoaderKind>,
) -> AppResult<SearchResult> {
    let client = state.client().await;
    let minecraft = minecraft.filter(|m| !m.trim().is_empty());
    match source {
        PackSource::Modrinth => {
            Modrinth::new(&client)
                .search_modpacks(query.trim(), minecraft.as_deref(), loader.map(|l| l.as_str()), &sort, offset)
                .await
        }
        PackSource::Curseforge => {
            let key = state.curseforge_key()?;
            let filter = PackFilter { minecraft, loader: loader.map(|l| l.as_str().to_string()) };
            CurseForge::new(&client, &key).search_modpacks(query.trim(), &filter, &sort, offset).await
        }
        PackSource::File => Err(AppError::InvalidInput("поиск по файлу невозможен".into())),
    }
}

/// Версии модпака (новые сверху).
#[tauri::command]
pub async fn modpack_versions(state: State<'_, AppState>, source: PackSource, project: String) -> AppResult<Vec<Version>> {
    let client = state.client().await;
    match source {
        PackSource::Modrinth => Modrinth::new(&client).all_versions(&project).await,
        PackSource::Curseforge => {
            let key = state.curseforge_key()?;
            Ok(CurseForge::new(&client, &key).files(&project, None).await?.into_iter().map(CfFile::into_version).collect())
        }
        PackSource::File => Ok(vec![]),
    }
}

// ---------- Предпросмотр и импорт файла ----------

/// Что за файл импортируется: модпак или инстанс целиком.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewFormat {
    Mrpack,
    Curseforge,
    Cubelaunch,
    Prism,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackPreview {
    pub format: PreviewFormat,
    pub name: String,
    pub version: String,
    pub summary: Option<String>,
    pub minecraft: String,
    pub loader: Option<LoaderKind>,
    pub loader_version: Option<String>,
    pub files: usize,
    pub overrides: usize,
    /// В архиве инстанса есть миры.
    pub has_saves: bool,
    /// Что из архива не переносится.
    pub warnings: Vec<String>,
}

#[tauri::command]
pub async fn inspect_modpack(path: String) -> AppResult<PackPreview> {
    blocking(move || {
        let p = PathBuf::from(&path);
        if let Some(a) = instance_archive::read(&p)? {
            return Ok(PackPreview {
                format: match a.kind {
                    ArchiveKind::Cubelaunch => PreviewFormat::Cubelaunch,
                    ArchiveKind::Prism => PreviewFormat::Prism,
                },
                name: a.name,
                version: String::new(),
                summary: None,
                minecraft: a.minecraft,
                loader: a.loader.as_ref().map(|l| l.0),
                loader_version: a.loader.map(|l| l.1),
                files: 0,
                overrides: a.files,
                has_saves: a.has_saves,
                warnings: a.warnings,
            });
        }
        let idx = read_index(&p)?;
        let overrides = count_overrides(&p, &idx.overrides)?;
        Ok(PackPreview {
            format: match idx.format {
                PackFormat::Mrpack => PreviewFormat::Mrpack,
                PackFormat::Curseforge => PreviewFormat::Curseforge,
            },
            has_saves: false,
            warnings: vec![],
            files: idx.files.len() + idx.cf_files.len(),
            overrides,
            loader: idx.loader.as_ref().map(|l| l.0),
            loader_version: idx.loader.as_ref().map(|l| l.1.clone()),
            name: idx.name,
            version: idx.version,
            summary: idx.summary,
            minecraft: idx.minecraft,
        })
    })
    .await
}

/// Где взять архив пака.
#[derive(Debug, Clone)]
enum Origin {
    File(PathBuf),
    Modrinth { version_id: String },
    Curseforge { project_id: String, file_id: String },
}

fn create_pack_instance(app: &AppHandle, name: &str, group: Option<String>, minecraft: &str, link: ModpackLink) -> AppResult<Instance> {
    let state = app.state::<AppState>();
    validate_token(minecraft, "Minecraft")?;
    let inst = state.instances.create(name, group, minecraft, None)?;
    let inst = state.instances.update(&inst.id, |i| i.modpack = Some(link))?;
    tracing::info!("создан инстанс модпака {} ({})", inst.id, inst.minecraft);
    notify_changed(app);
    Ok(inst)
}

fn spawn_pack_job(app: &AppHandle, kind: &str, inst: &Instance, origin: Origin, title: String) -> AppResult<u64> {
    let state = app.state::<AppState>();
    if state.jobs.running_for_instance(&inst.id) {
        return Err(AppError::InvalidInput("для этого инстанса уже выполняется задача".into()));
    }
    if state.games.is_running(&inst.id) {
        return Err(AppError::InvalidInput("сначала закройте игру".into()));
    }
    let id = inst.id.clone();
    Ok(state.jobs.spawn(app, kind, title, Some(id.clone()), move |ctx| async move { run_pack(&ctx, &id, origin).await }))
}

/// Создаёт инстанс из файла модпака и запускает установку.
#[tauri::command]
pub async fn import_modpack(app: AppHandle, path: String, name: Option<String>, group: Option<String>) -> AppResult<Instance> {
    let p = PathBuf::from(&path);
    let p2 = p.clone();
    if let Some(a) = blocking(move || instance_archive::read(&p2)).await? {
        return super::transfer::import_archive(&app, p, a, name, group).await;
    }
    let p2 = p.clone();
    let idx = blocking(move || read_index(&p2)).await?;
    let link = ModpackLink { source: PackSource::File, project_id: None, version_id: None, name: idx.name.clone(), version: idx.version.clone(), icon_url: None };
    let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| idx.name.clone());
    let inst = create_pack_instance(&app, &name, group, &idx.minecraft, link)?;
    spawn_pack_job(&app, "installModpack", &inst, Origin::File(p), format!("Модпак «{}»", inst.name))?;
    Ok(inst)
}

/// Ставит версию модпака с Modrinth/CurseForge как новый инстанс.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn install_modpack(
    app: AppHandle,
    source: PackSource,
    project_id: String,
    version_id: String,
    title: String,
    name: Option<String>,
    group: Option<String>,
    icon_url: Option<String>,
) -> AppResult<Instance> {
    let state = app.state::<AppState>();
    let client = state.client().await;
    let (minecraft, version_number, origin) = match source {
        PackSource::Modrinth => {
            let v = Modrinth::new(&client).version(&version_id).await?.ok_or_else(|| AppError::NotFound(format!("версия {version_id}")))?;
            let mc = v.game_versions.first().cloned().ok_or_else(|| AppError::Content("у версии модпака не указана версия Minecraft".into()))?;
            (mc, v.version_number, Origin::Modrinth { version_id: version_id.clone() })
        }
        PackSource::Curseforge => {
            let key = state.curseforge_key()?;
            let f = CurseForge::new(&client, &key)
                .file(&project_id, &version_id)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("файл {version_id}")))?;
            let mc = f.minecraft_versions().first().cloned().ok_or_else(|| AppError::Content("у файла модпака не указана версия Minecraft".into()))?;
            let v = f.into_version();
            (mc, v.version_number, Origin::Curseforge { project_id: project_id.clone(), file_id: version_id.clone() })
        }
        PackSource::File => return Err(AppError::InvalidInput("для файла используйте импорт".into())),
    };
    let link = ModpackLink {
        source,
        project_id: Some(project_id),
        version_id: Some(version_id),
        name: title.clone(),
        version: version_number,
        icon_url: icon_url.filter(|u| u.starts_with("https://")),
    };
    let name = name.filter(|n| !n.trim().is_empty()).unwrap_or(title);
    let inst = create_pack_instance(&app, &name, group, &minecraft, link)?;
    spawn_pack_job(&app, "installModpack", &inst, origin, format!("Модпак «{}»", inst.name))?;
    Ok(inst)
}

/// Обновляет (или откатывает) модпак инстанса: до версии из источника или из файла.
#[tauri::command]
pub fn update_modpack(app: AppHandle, state: State<'_, AppState>, id: String, version_id: Option<String>, path: Option<String>) -> AppResult<u64> {
    let inst = state.instances.get(&id)?;
    let link = inst.modpack.clone().ok_or_else(|| AppError::InvalidInput("инстанс не создан из модпака".into()))?;
    let origin = match (path, version_id) {
        (Some(p), _) => Origin::File(PathBuf::from(p)),
        (None, Some(v)) => match link.source {
            PackSource::Modrinth => Origin::Modrinth { version_id: v },
            PackSource::Curseforge => Origin::Curseforge {
                project_id: link.project_id.clone().ok_or_else(|| AppError::InvalidInput("нет id проекта".into()))?,
                file_id: v,
            },
            PackSource::File => return Err(AppError::InvalidInput("модпак из файла обновляется только из файла".into())),
        },
        (None, None) => return Err(AppError::InvalidInput("не выбрана версия модпака".into())),
    };
    spawn_pack_job(&app, "updateModpack", &inst, origin, format!("Обновление «{}»", inst.name))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackStateInfo {
    pub files: usize,
    pub missing: Vec<MissingFile>,
}

#[tauri::command]
pub fn modpack_state(state: State<'_, AppState>, id: String) -> AppResult<ModpackStateInfo> {
    let s = ManagedState::load(&state.instances.dir(&id)?)?;
    Ok(ModpackStateInfo { files: s.files.len(), missing: s.missing })
}

/// Кладёт на место файлы, скачанные вручную (ищет в «Загрузках» и среди `paths`).
/// Возвращает, сколько файлов ещё не хватает.
#[tauri::command]
pub async fn modpack_add_missing(state: State<'_, AppState>, id: String, paths: Vec<String>) -> AppResult<usize> {
    let inst_dir = state.instances.dir(&id)?;
    let game = state.instances.game_dir(&id)?;
    blocking(move || {
        let mut s = ManagedState::load(&inst_dir)?;
        let extra: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let found = find_downloaded(&s.missing, &extra);
        let mut done = Vec::new();
        for (i, src) in found {
            let m = &s.missing[i];
            let rel = safe_rel(&m.path)?;
            let dest = join_rel(&game, &rel);
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&src, &dest)?;
            let sha = match &m.sha1 {
                Some(s) => s.clone(),
                None => sha1_path(&dest)?,
            };
            s.files.insert(rel, sha);
            done.push(i);
        }
        let mut i = 0;
        s.missing.retain(|_| {
            let keep = !done.contains(&i);
            i += 1;
            keep
        });
        s.save(&inst_dir)?;
        Ok(s.missing.len())
    })
    .await
}

// ---------- Установка/обновление (тело задачи) ----------

async fn fetch_pack(ctx: &JobCtx, origin: &Origin) -> AppResult<PathBuf> {
    let state = ctx.app.state::<AppState>();
    let client = state.client().await;
    let dir = state.paths.cache_dir().join("modpacks");
    let task = match origin {
        Origin::File(p) => return Ok(p.clone()),
        Origin::Modrinth { version_id } => {
            let v = Modrinth::new(&client).version(version_id).await?.ok_or_else(|| AppError::NotFound(format!("версия {version_id}")))?;
            let f = v
                .files
                .iter()
                .find(|f| f.filename.ends_with(".mrpack"))
                .or(v.primary_file())
                .ok_or_else(|| AppError::Content("у версии модпака нет файла".into()))?;
            DownloadTask { url: f.url.clone(), dest: dir.join(format!("mr-{version_id}.mrpack")), sha1: f.hashes.sha1.clone(), size: Some(f.size).filter(|s| *s > 0) }
        }
        Origin::Curseforge { project_id, file_id } => {
            let key = state.curseforge_key()?;
            let f = CurseForge::new(&client, &key)
                .file(project_id, file_id)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("файл {file_id}")))?;
            let url = f.download_url.clone().ok_or_else(|| {
                AppError::Content("автор запретил скачивание этого модпака из сторонних лаунчеров — скачайте zip на сайте CurseForge и импортируйте файл".into())
            })?;
            DownloadTask { url, dest: dir.join(format!("cf-{file_id}.zip")), sha1: f.sha1(), size: Some(f.file_length).filter(|s| *s > 0) }
        }
    };
    let dest = task.dest.clone();
    download_all(&client, vec![task], state.download_options().await, &ctx.cancel, |p| ctx.progress(p)).await?;
    Ok(dest)
}

/// Файлы CurseForge: что скачать и что придётся скачать вручную.
async fn resolve_curseforge(state: &AppState, idx: &PackIndex) -> AppResult<(Vec<PackFile>, Vec<MissingFile>)> {
    if idx.cf_files.is_empty() {
        return Ok((vec![], vec![]));
    }
    let key = state.curseforge_key().map_err(|_| AppError::CurseforgeKey("для модпаков CurseForge нужен ключ API — укажите его в настройках".into()))?;
    let client = state.client().await;
    let cf = CurseForge::new(&client, &key);
    let file_ids: Vec<u64> = idx.cf_files.iter().map(|r| r.file_id).collect();
    let mut mod_ids: Vec<u64> = idx.cf_files.iter().map(|r| r.project_id).collect();
    mod_ids.sort_unstable();
    mod_ids.dedup();
    let files: HashMap<u64, CfFile> = cf.files_by_ids(&file_ids).await?.into_iter().map(|f| (f.id, f)).collect();
    let mods: HashMap<u64, CfMod> = cf.mods_by_ids(&mod_ids).await?.into_iter().map(|m| (m.id, m)).collect();
    let mut out = Vec::new();
    let mut missing = Vec::new();
    for r in &idx.cf_files {
        let f = files
            .get(&r.file_id)
            .ok_or_else(|| AppError::Content(format!("файл {} проекта {} не найден на CurseForge", r.file_id, r.project_id)))?;
        let m = mods.get(&r.project_id);
        let class = m.and_then(|m| m.class_id).unwrap_or(crate::content::curseforge::CLASS_MODS);
        let Some(folder) = folder_for_class(class) else {
            tracing::warn!("пропущен файл {} (класс CurseForge {class} не поддерживается)", f.file_name);
            continue;
        };
        let path = safe_rel(&format!("{folder}/{}", f.file_name))?;
        match f.download_url.as_ref().filter(|u| u.starts_with("https://")) {
            Some(url) => out.push(PackFile { path, sha1: f.sha1(), size: Some(f.file_length).filter(|s| *s > 0), urls: vec![url.clone()] }),
            None => missing.push(MissingFile {
                name: m.map(|m| m.name.clone()).unwrap_or_else(|| f.display_name.clone()),
                file_name: f.file_name.clone(),
                page_url: m.map(|m| format!("{}/files/{}", m.page_url(), f.id)).unwrap_or_else(|| format!("https://www.curseforge.com/projects/{}", r.project_id)),
                path,
                sha1: f.sha1(),
                size: f.file_length,
            }),
        }
    }
    Ok((out, missing))
}

/// Версия загрузчика из пака → наш формат (у Forge — полная версия maven).
pub(crate) async fn resolve_loader(state: &AppState, mc: &str, loader: &Option<(LoaderKind, String)>) -> AppResult<Option<LoaderSpec>> {
    let Some((kind, raw)) = loader else { return Ok(None) };
    let version = match kind {
        LoaderKind::Fabric | LoaderKind::Quilt => raw.clone(),
        LoaderKind::Forge | LoaderKind::NeoForge => {
            let client = state.client().await;
            let list = list_versions(&client, &state.paths.meta_dir(), *kind, mc).await.unwrap_or_default();
            match list.iter().find(|v| &v.version == raw || &short_version(mc, &v.version) == raw) {
                Some(v) => v.version.clone(),
                None if *kind == LoaderKind::NeoForge && neoforge_minecraft(raw).as_deref() == Some(mc) => raw.clone(),
                None if raw.starts_with(&format!("{mc}-")) => raw.clone(),
                None => format!("{mc}-{raw}"),
            }
        }
    };
    validate_token(&version, kind.display())?;
    Ok(Some(LoaderSpec { kind: *kind, version }))
}

async fn run_pack(ctx: &JobCtx, id: &str, origin: Origin) -> AppResult<()> {
    let state = ctx.app.state::<AppState>();
    ctx.stage("modpackDownload");
    let pack = fetch_pack(ctx, &origin).await?;

    ctx.stage("modpackResolve");
    let p = pack.clone();
    let idx = blocking(move || read_index(&p)).await?;
    validate_token(&idx.minecraft, "Minecraft")?;
    let (mut files, missing) = match idx.format {
        PackFormat::Mrpack => (idx.files.clone(), vec![]),
        PackFormat::Curseforge => resolve_curseforge(&state, &idx).await?,
    };
    let loader = resolve_loader(&state, &idx.minecraft, &idx.loader).await?;

    let inst_dir = state.instances.dir(id)?;
    let game = state.instances.game_dir(id)?;
    let staging = inst_dir.join(STAGING_DIR);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;

    ctx.stage("files");
    let tasks: Vec<DownloadTask> = files
        .iter()
        .map(|f| DownloadTask { url: f.urls[0].clone(), dest: join_rel(&staging, &f.path), sha1: f.sha1.clone(), size: f.size })
        .collect();
    let client = state.client().await;
    download_all(&client, tasks, state.download_options().await, &ctx.cancel, |p| ctx.progress(p)).await?;

    ctx.stage("modpackApply");
    let (pack2, prefixes, staging2, cancel) = (pack.clone(), idx.overrides.clone(), staging.clone(), ctx.cancel.clone());
    let overrides = blocking(move || extract_overrides(&pack2, &prefixes, &staging2, &cancel)).await?;
    // overrides перекрывают скачанные файлы с тем же путём.
    files.retain(|f| !overrides.contains_key(&f.path));

    let staging2 = staging.clone();
    let game2 = game.clone();
    let inst_dir2 = inst_dir.clone();
    let report = blocking(move || {
        let mut entries: Vec<NewEntry> = Vec::new();
        for f in &files {
            let sha1 = match &f.sha1 {
                Some(s) => s.clone(),
                None => sha1_path(&join_rel(&staging2, &f.path))?,
            };
            entries.push(NewEntry { rel: f.path.clone(), sha1, from_override: false });
        }
        // Файлы, которые пользователь уже скачал вручную.
        let mut still_missing = missing.clone();
        let found = find_downloaded(&missing, &[]);
        for (i, src) in &found {
            let m = &missing[*i];
            let staged = join_rel(&staging2, &m.path);
            if let Some(p) = staged.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(src, &staged)?;
            let sha1 = m.sha1.clone().map_or_else(|| sha1_path(&staged), Ok)?;
            entries.push(NewEntry { rel: m.path.clone(), sha1, from_override: false });
        }
        let found_idx: Vec<usize> = found.iter().map(|(i, _)| *i).collect();
        let mut i = 0;
        still_missing.retain(|_| {
            let keep = !found_idx.contains(&i);
            i += 1;
            keep
        });
        for (rel, sha1) in overrides {
            entries.push(NewEntry { rel, sha1, from_override: true });
        }
        let old = ManagedState::load(&inst_dir2)?;
        // Ещё не скачанные вручную файлы прошлой версии не считаем «своими» для удаления.
        let report = apply(&game2, &staging2, &entries, &old.files)?;
        ManagedState { files: report.managed.clone(), missing: still_missing }.save(&inst_dir2)?;
        Ok(report)
    })
    .await?;
    let _ = std::fs::remove_dir_all(&staging);
    tracing::info!(
        "модпак {}: записано {}, удалено {}, оставлено изменённых {} ({})",
        idx.name,
        report.written,
        report.removed,
        report.kept.len(),
        report.kept.join(", ")
    );

    let version = idx.version.clone();
    let inst = state.instances.update(id, |i| {
        if i.minecraft != idx.minecraft || i.loader != loader {
            i.minecraft = idx.minecraft.clone();
            i.loader = loader.clone();
            i.installed = false;
        }
        if let Some(link) = i.modpack.as_mut() {
            match &origin {
                Origin::File(_) => {
                    link.version = version.clone();
                    if link.source == PackSource::File {
                        link.name = idx.name.clone();
                    }
                }
                Origin::Modrinth { version_id } | Origin::Curseforge { file_id: version_id, .. } => link.version_id = Some(version_id.clone()),
            }
        }
    })?;
    // Номер версии из источника (у Modrinth/CF он точнее, чем в архиве).
    if !matches!(origin, Origin::File(_)) {
        if let Some(number) = remote_version_number(&state, &origin).await {
            state.instances.update(id, |i| {
                if let Some(l) = i.modpack.as_mut() {
                    l.version = number;
                }
            })?;
        }
    }
    notify_changed(&ctx.app);
    install_instance_files(&state, ctx, &inst, Env::current()).await?;
    Ok(())
}

async fn remote_version_number(state: &AppState, origin: &Origin) -> Option<String> {
    let client = state.client().await;
    match origin {
        Origin::Modrinth { version_id } => Modrinth::new(&client).version(version_id).await.ok().flatten().map(|v| v.version_number),
        Origin::Curseforge { project_id, file_id } => {
            let key = state.curseforge_key().ok()?;
            CurseForge::new(&client, &key).file(project_id, file_id).await.ok().flatten().map(|f| f.into_version().version_number)
        }
        Origin::File(_) => None,
    }
}

// ---------- Экспорт ----------

#[tauri::command]
pub fn export_candidates(state: State<'_, AppState>, id: String) -> AppResult<Vec<ExportCandidate>> {
    Ok(export::candidates(&state.instances.game_dir(&id)?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportArgs {
    pub format: PackFormat,
    pub path: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub summary: Option<String>,
    pub include: Vec<String>,
}

#[tauri::command]
pub fn export_modpack(app: AppHandle, state: State<'_, AppState>, id: String, args: ExportArgs) -> AppResult<u64> {
    let inst = state.instances.get(&id)?;
    let out = PathBuf::from(&args.path);
    if !out.is_absolute() {
        return Err(AppError::InvalidInput("путь должен быть абсолютным".into()));
    }
    if args.name.trim().is_empty() {
        return Err(AppError::InvalidInput("укажите название модпака".into()));
    }
    let game = state.instances.game_dir(&id)?;
    let title = format!("Экспорт «{}»", inst.name);
    Ok(state.jobs.spawn(&app, "exportModpack", title, Some(id), move |ctx| async move {
        let state = ctx.app.state::<AppState>();
        ctx.stage("modpackResolve");
        let include = args.include.clone();
        let g = game.clone();
        let files = blocking(move || export::collect(&g, &include)).await?;
        let meta = ExportMeta {
            name: args.name.trim().to_string(),
            version: args.version.trim().to_string(),
            summary: args.summary.clone(),
            minecraft: inst.minecraft.clone(),
            loader: inst.loader.clone(),
        };
        let linkable: Vec<usize> = files.iter().enumerate().filter(|(_, f)| export::is_linkable(&f.rel)).map(|(i, _)| i).collect();
        let client = state.client().await;
        let mut linked = std::collections::HashSet::new();
        let (index_name, index) = match args.format {
            PackFormat::Mrpack => {
                let paths: Vec<PathBuf> = linkable.iter().map(|i| files[*i].abs.clone()).collect();
                let hashes = blocking(move || paths.iter().map(|p| sha1_path(p)).collect::<AppResult<Vec<_>>>()).await?;
                let found = Modrinth::new(&client).versions_by_hashes(&hashes).await?;
                let mut remote = Vec::new();
                for (k, i) in linkable.iter().enumerate() {
                    let Some(v) = found.get(&hashes[k]) else { continue };
                    let Some(vf) = v.files.iter().find(|f| f.hashes.sha1.as_deref() == Some(hashes[k].as_str())) else { continue };
                    let Some(sha512) = vf.hashes.sha512.clone() else { continue };
                    remote.push(MrRemote { rel: files[*i].rel.clone(), url: vf.url.clone(), sha1: hashes[k].clone(), sha512, size: vf.size });
                    linked.insert(*i);
                }
                ("modrinth.index.json", export::mrpack_index(&meta, &remote))
            }
            PackFormat::Curseforge => {
                let mut refs = Vec::new();
                match state.curseforge_key() {
                    Ok(key) => {
                        let paths: Vec<PathBuf> = linkable.iter().map(|i| files[*i].abs.clone()).collect();
                        let prints = blocking(move || paths.iter().map(|p| Ok(fingerprint(&std::fs::read(p)?))).collect::<AppResult<Vec<u32>>>()).await?;
                        let found = CurseForge::new(&client, &key).by_fingerprints(&prints).await?;
                        for (k, i) in linkable.iter().enumerate() {
                            if let Some(m) = found.get(&prints[k]) {
                                refs.push((m.id, m.file.id));
                                linked.insert(*i);
                            }
                        }
                    }
                    Err(_) => tracing::warn!("ключ CurseForge не задан — все моды экспортируются в overrides"),
                }
                ("manifest.json", export::curseforge_manifest(&meta, &refs))
            }
        };
        let overrides: Vec<_> = files.iter().enumerate().filter(|(i, _)| !linked.contains(i)).map(|(_, f)| f.clone()).collect();
        tracing::info!("экспорт {}: ссылками {}, в overrides {}", meta.name, linked.len(), overrides.len());
        ctx.stage("archive");
        let c = ctx.clone();
        blocking(move || {
            export::write_archive(&out, index_name, &index, &overrides, &c.cancel, |done, total| {
                c.progress(Progress { files_done: done, files_total: total, bytes_done: 0, bytes_total: 0 })
            })
        })
        .await?;
        Ok(())
    }))
}
