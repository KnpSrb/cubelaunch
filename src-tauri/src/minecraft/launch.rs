//! Запуск инстанса: Java → проверка/докачка файлов → natives и assets →
//! командная строка → pre-launch → процесс → мониторинг.

use std::path::Path;
use std::process::Stdio;

use tauri::{Emitter, Manager};

use super::arguments::{build_classpath, build_command, redact, split_args, LaunchOptions, LaunchVars};
use super::install::{client_jar_path, install_version, version_json_path, InstallParams};
use super::libraries::resolve_libraries;
use super::prepare::{extract_natives, files_present, natives_dir, prepare_legacy_assets};
use super::rules::{Arch, Env};
use super::session::{info_line, monitor, ExitHooks};
use super::version::VersionJson;
use crate::auth::{ensure_token, AccountKind, MsaContext};
use crate::downloads::jobs::JobCtx;
use crate::error::{AppError, AppResult};
use crate::events;
use crate::instances::Instance;
use crate::java::download::install_temurin;
use crate::java::{no_console, pick_best, probe, JavaInstall};
use crate::loaders::{install_loader, load_profile};
use crate::settings::{LauncherVisibility, Settings};
use crate::state::AppState;
use crate::util::read_json_opt;

/// Подбирает Java: явный путь (инстанс → глобальный) → найденная подходящая →
/// автоскачивание Temurin.
pub async fn resolve_java(
    state: &AppState,
    inst: &Instance,
    settings: &Settings,
    required: u32,
    ctx: &JobCtx,
) -> AppResult<JavaInstall> {
    let root = state.paths.java_dir();
    let explicit = inst
        .overrides
        .java_path
        .as_ref()
        .or(settings.java.path.as_ref())
        .filter(|p| !p.trim().is_empty());
    if let Some(p) = explicit {
        let j = probe(Path::new(p), Some(&root)).await?;
        if j.major != required {
            tracing::warn!("выбрана Java {} для версии, которой нужна Java {required}", j.major);
        }
        return Ok(j);
    }
    let list = state.java_installs(false).await;
    if let Some(j) = pick_best(&list, required, Arch::current()) {
        return Ok(j.clone());
    }
    if !settings.java.auto_download {
        return Err(AppError::JavaMissing(required));
    }
    let j = install_temurin(
        &state.client().await,
        &root,
        &state.paths.cache_dir(),
        required,
        Arch::current(),
        state.download_options().await,
        &ctx.cancel,
        ctx,
    )
    .await?;
    state.invalidate_java_cache().await;
    Ok(j)
}

/// Полная установка инстанса: ванильная версия, затем загрузчик модов (если выбран).
/// Возвращает итоговое (слитое) описание версии. Инстанс помечается установленным.
pub async fn install_instance_files(state: &AppState, ctx: &JobCtx, inst: &Instance, env: Env) -> AppResult<VersionJson> {
    // Установки делят общие файлы (библиотеки, assets) и идут по одной.
    let _lock = match state.install_lock.try_lock() {
        Ok(g) => g,
        Err(_) => {
            ctx.stage("waitInstall");
            state.install_lock.lock().await
        }
    };
    let client = state.client().await;
    let params = InstallParams {
        client: &client,
        paths: &state.paths,
        opts: state.download_options().await,
        cancel: &ctx.cancel,
        env,
        skip_asset_objects: false,
    };
    let vanilla = install_version(&params, &inst.minecraft, ctx).await?;
    let version = match &inst.loader {
        None => vanilla,
        Some(spec) => {
            // Процессорам установщика Forge/NeoForge нужна Java под эту версию игры.
            let java = if spec.kind.uses_installer() {
                ctx.stage("java");
                let settings = state.settings.read().await.clone();
                Some(resolve_java(state, inst, &settings, vanilla.required_java(), ctx).await?)
            } else {
                None
            };
            let java_path = java.as_ref().map(|j| std::path::PathBuf::from(&j.path));
            let profile = install_loader(&params, &inst.minecraft, spec, &vanilla, java_path.as_deref(), ctx).await?;
            profile.merge_onto(vanilla)
        }
    };
    // Инстанс могли удалить во время установки — это не ошибка.
    match state.instances.update(&inst.id, |i| i.installed = true) {
        Ok(_) | Err(AppError::NotFound(_)) => {}
        Err(e) => return Err(e),
    }
    crate::commands::instances::notify_changed(&ctx.app);
    Ok(version)
}

/// Описание версии инстанса с диска (ванильное + профиль загрузчика).
/// `None` — чего-то не хватает, нужна установка.
pub fn load_instance_version(paths: &crate::paths::AppPaths, inst: &Instance) -> AppResult<Option<VersionJson>> {
    let Some(vanilla) = read_json_opt::<VersionJson>(&version_json_path(paths, &inst.minecraft))? else {
        return Ok(None);
    };
    match &inst.loader {
        None => Ok(Some(vanilla)),
        Some(spec) => Ok(load_profile(paths, &inst.minecraft, spec)?.map(|p| p.merge_onto(vanilla))),
    }
}

/// Вывод JVM в UTF-8: иначе на Windows сообщения JVM приходят в системной
/// кодировке (cp1251) и в логе превращаются в «кракозябры». Явные настройки
/// пользователя не трогаем.
fn with_utf8_output(user: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for (prop, value) in [("file.encoding", "UTF-8"), ("stdout.encoding", "UTF-8"), ("stderr.encoding", "UTF-8")] {
        if !user.iter().any(|a| a.starts_with(&format!("-D{prop}="))) {
            out.push(format!("-D{prop}={value}"));
        }
    }
    out.extend(user);
    out
}

/// Команда оболочки (pre-launch / post-exit) с переменными инстанса.
fn shell_command(cmd: &str, inst: &Instance, inst_dir: &Path, game_dir: &Path, java: &str) -> tokio::process::Command {
    let mut c = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };
    c.current_dir(inst_dir)
        .env("INST_ID", &inst.id)
        .env("INST_NAME", &inst.name)
        .env("INST_DIR", inst_dir)
        .env("INST_MC_DIR", game_dir)
        .env("INST_MC_VERSION", &inst.minecraft)
        .env("INST_JAVA", java);
    no_console(&mut c);
    c
}

pub async fn launch(ctx: &JobCtx, instance_id: &str) -> AppResult<()> {
    let app = ctx.app.clone();
    let state = app.state::<AppState>();
    if state.games.is_running(instance_id) {
        return Err(AppError::AlreadyRunning);
    }
    let inst = state.instances.get(instance_id)?;
    let account = state.accounts.default_account()?;
    let settings = state.settings.read().await.clone();
    let paths = &state.paths;

    // 1. Описание версии (если его ещё нет — ставим версию целиком).
    ctx.stage("prepare");
    let version = match load_instance_version(paths, &inst)? {
        Some(v) if inst.installed => v,
        _ => install_instance_files(&state, ctx, &inst, Env::current()).await?,
    };

    // 2. Java.
    ctx.stage("java");
    let java = resolve_java(&state, &inst, &settings, version.required_java(), ctx).await?;
    // Natives выбираются под архитектуру JVM, а не лаунчера.
    let env = Env { arch: java.arch, ..Env::current() };

    // 3. Файлы версии.
    ctx.stage("checkFiles");
    let mut libs = resolve_libraries(&version.libraries, &env);
    let present = {
        let (p, v, l) = (paths.clone(), version.clone(), libs.clone());
        let jar = client_jar_path(&p, &inst.minecraft);
        tokio::task::spawn_blocking(move || files_present(&p, &v, &l, &jar)).await.unwrap_or(false)
    };
    let version = if !present {
        let v = install_instance_files(&state, ctx, &inst, env.clone()).await?;
        libs = resolve_libraries(&v.libraries, &env);
        v
    } else {
        version
    };
    if ctx.cancel.is_cancelled() {
        return Err(AppError::Cancelled);
    }

    // 4. Natives и assets.
    ctx.stage("natives");
    let inst_dir = state.instances.dir(&inst.id)?;
    let game_dir = state.instances.game_dir(&inst.id)?;
    std::fs::create_dir_all(&game_dir)?;
    let natives_dir = natives_dir(&inst_dir, &inst.id);
    let game_assets = {
        let (p, v, l, nd, gd) = (paths.clone(), version.clone(), libs.clone(), natives_dir.clone(), game_dir.clone());
        tokio::task::spawn_blocking(move || -> AppResult<_> {
            extract_natives(&l, &p.libraries_dir(), &nd)?;
            prepare_legacy_assets(&p, &v, &gd)
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))??
    };

    // 5. Командная строка.
    let ov = &inst.overrides;
    let (token, user_type, client_id, account) = match account.kind {
        AccountKind::Offline => ("0".to_string(), "legacy".to_string(), String::new(), account),
        AccountKind::Microsoft => {
            ctx.stage("account");
            let client_id = state.msa_client_id().await?;
            let client = state.client().await;
            let msa = MsaContext {
                client: &client,
                ep: &state.msa_endpoints,
                client_id: &client_id,
                secrets: state.secrets.as_ref(),
            };
            // Токен из кэша или обновлённый; заодно подтягиваются новый ник и скин.
            let (token, acc) = ensure_token(&msa, &state.accounts, &account, false).await?;
            (token, "msa".to_string(), client_id, acc)
        }
    };
    let vars = LaunchVars {
        username: account.username.clone(),
        uuid: account.uuid.clone(),
        access_token: token.clone(),
        user_type,
        xuid: "0".into(),
        client_id,
        // Имя версии = версия Minecraft: Forge/NeoForge по нему ищут клиентский jar
        // (`-DignoreList=…${version_name}.jar`), а он лежит в versions/<mc>/<mc>.jar.
        version_name: inst.minecraft.clone(),
        version_type: version.kind.clone().unwrap_or_else(|| "release".into()),
        game_dir: game_dir.clone(),
        assets_root: paths.assets_dir(),
        game_assets,
        assets_index: version.assets_id().to_string(),
        natives_dir: natives_dir.clone(),
        libraries_dir: paths.libraries_dir(),
        classpath: build_classpath(&libs, &paths.libraries_dir(), &client_jar_path(paths, &inst.minecraft), &env),
        width: ov.window_width.unwrap_or(settings.game.window_width),
        height: ov.window_height.unwrap_or(settings.game.window_height),
    };
    let logging_arg = version.logging.as_ref().and_then(|l| l.client.as_ref()).map(|c| {
        let path = paths.assets_dir().join("log_configs").join(&c.file.id);
        c.argument.replace("${path}", &path.display().to_string())
    });
    let opts = LaunchOptions {
        min_memory_mb: ov.min_memory_mb.unwrap_or(settings.java.min_memory_mb),
        max_memory_mb: ov.max_memory_mb.unwrap_or(settings.java.max_memory_mb),
        extra_jvm_args: with_utf8_output(split_args(ov.jvm_args.as_deref().unwrap_or(&settings.java.jvm_args))),
        logging_arg,
        fullscreen: ov.fullscreen.unwrap_or(settings.game.fullscreen),
    };
    let args = build_command(&version, &env, &vars, &opts)?;

    // 6. Pre-launch.
    if let Some(cmd) = ov.pre_launch_command.as_deref().filter(|c| !c.trim().is_empty()) {
        ctx.stage("preLaunch");
        let status = shell_command(cmd, &inst, &inst_dir, &game_dir, &java.path).status().await?;
        if !status.success() {
            return Err(AppError::Launch(format!("pre-launch команда завершилась с кодом {:?}", status.code())));
        }
    }

    // 7. Процесс.
    let mut cmd = tokio::process::Command::new(&java.path);
    cmd.args(&args)
        .current_dir(&game_dir)
        .envs(&ov.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_console(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| AppError::Launch(format!("не удалось запустить {}: {e}", java.path)))?;

    let preface = vec![
        info_line(format!(
            "Запуск «{}» (Minecraft {}{}), аккаунт {}",
            inst.name,
            inst.minecraft,
            inst.loader.as_ref().map(|l| format!(", {} {}", l.kind.display(), crate::loaders::short_version(&inst.minecraft, &l.version))).unwrap_or_default(),
            account.username
        )),
        info_line(format!("Java {} ({:?}, {}): {}", java.version, java.arch, java.vendor, java.path)),
        info_line(format!("Командная строка: {}", redact(&args, &token).join(" "))),
    ];
    tracing::info!("запуск {}: java {}", inst.id, java.path);
    let session = state.games.start(&inst.id, child.id(), preface);
    let status = session.status.lock().unwrap().clone();
    let _ = app.emit(events::GAME_STATE, status);
    state.instances.update(&inst.id, |i| i.last_played = Some(chrono::Utc::now()))?;

    // 8. Поведение окна лаунчера.
    let window = app.get_webview_window("main");
    let on_launch = settings.game.on_launch;
    if let Some(w) = &window {
        match on_launch {
            LauncherVisibility::Minimize => {
                let _ = w.minimize();
            }
            LauncherVisibility::Hide => {
                let _ = w.hide();
            }
            LauncherVisibility::Keep | LauncherVisibility::Close => {}
        }
    }

    let hooks = {
        let app = app.clone();
        let inst = inst.clone();
        let inst_dir = inst_dir.clone();
        let game_dir = game_dir.clone();
        let java_path = java.path.clone();
        let record = settings.game.record_playtime;
        let post_exit = ov.post_exit_command.clone().filter(|c| !c.trim().is_empty());
        ExitHooks {
            on_exit: Box::new(move |_status, played| {
                let state = app.state::<AppState>();
                if record {
                    let secs = played.as_secs();
                    let _ = state.instances.update(&inst.id, |i| i.playtime_secs += secs);
                    crate::commands::instances::notify_changed(&app);
                }
                if on_launch == LauncherVisibility::Hide {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                }
                if let Some(cmd) = post_exit {
                    let mut c = shell_command(&cmd, &inst, &inst_dir, &game_dir, &java_path);
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = c.status().await {
                            tracing::warn!("post-exit команда не выполнилась: {e}");
                        }
                    });
                }
            }),
        }
    };
    tauri::async_runtime::spawn(monitor(app.clone(), session, child, game_dir, hooks));

    if on_launch == LauncherVisibility::Close {
        // Даём событиям уйти в UI и закрываем лаунчер; игра продолжает работать.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            app.exit(0);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_flags_respect_user_choice() {
        let a = with_utf8_output(vec!["-Dfile.encoding=Cp1251".into(), "-Xss2M".into()]);
        assert_eq!(a, vec!["-Dstdout.encoding=UTF-8", "-Dstderr.encoding=UTF-8", "-Dfile.encoding=Cp1251", "-Xss2M"]);
        assert_eq!(with_utf8_output(vec![]).len(), 3);
    }
}
