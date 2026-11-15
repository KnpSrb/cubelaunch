//! Cubelaunch — бэкенд лаунчера Minecraft: Java Edition.

mod commands;
mod error;
mod events;
mod paths;
mod settings;
mod state;
mod util;

// Модули следующих этапов (пока заготовки).
mod auth;
mod content;
mod downloads;
mod instances;
mod java;
mod loaders;
mod minecraft;
mod modpacks;

use tauri::Manager;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::paths::AppPaths;
use crate::state::AppState;

/// Держит фоновый поток записи логов живым, пока живо приложение.
struct LogGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

fn init_logging(paths: &AppPaths) -> LogGuard {
    let appender = tracing_appender::rolling::daily(paths.logs_dir(), "launcher.log");
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,cubelaunch_lib=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stdout))
        .with(fmt::layer().with_ansi(false).with_writer(file_writer))
        .init();
    LogGuard(guard)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::resolve().expect("не удалось определить папку данных");
    std::fs::create_dir_all(paths.logs_dir()).expect("не удалось создать папку логов");
    let log_guard = init_logging(&paths);
    tracing::info!(
        "Cubelaunch {} запускается, данные: {}",
        env!("CARGO_PKG_VERSION"),
        paths.data_dir.display()
    );

    let state = AppState::init(paths).expect("не удалось инициализировать лаунчер");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(commands::updates::PendingUpdate::default())
        .manage(state)
        .setup(move |app| {
            app.manage(log_guard);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::app_info,
            commands::app::open_launcher_folder,
            commands::updates::check_update,
            commands::updates::install_update,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::reset_settings,
            commands::settings::set_data_dir,
            commands::versions::get_version_manifest,
            commands::versions::list_jobs,
            commands::versions::retry_job,
            commands::transfer::list_groups,
            commands::transfer::set_group_order,
            commands::transfer::create_group,
            commands::transfer::rename_group,
            commands::transfer::delete_group,
            commands::transfer::clone_instance,
            commands::transfer::instance_export_candidates,
            commands::transfer::export_instance,
            commands::versions::cancel_job,
            commands::versions::dismiss_job,
            commands::instances::list_instances,
            commands::instances::create_instance,
            commands::instances::install_instance,
            commands::instances::rename_instance,
            commands::instances::set_instance_group,
            commands::instances::delete_instance,
            commands::instances::open_instance_folder,
            commands::instances::get_instance,
            commands::instances::launch_instance,
            commands::instances::kill_game,
            commands::instances::get_game_log,
            commands::instances::running_games,
            commands::instances::save_game_log,
            commands::instances::update_instance_overrides,
            commands::instances::loader_versions,
            commands::instances::set_instance_version,
            commands::content::list_content,
            commands::content::set_content_enabled,
            commands::content::delete_content,
            commands::content::add_content_files,
            commands::content::open_content_folder,
            commands::content::content_modrinth_info,
            commands::content::modrinth_search,
            commands::content::modrinth_versions,
            commands::content::install_modrinth,
            commands::content::check_content_updates,
            commands::content::update_content,
            commands::content::curseforge_search,
            commands::content::curseforge_versions,
            commands::content::install_curseforge,
            commands::modpacks::curseforge_status,
            commands::modpacks::set_curseforge_key,
            commands::modpacks::modpack_search,
            commands::modpacks::modpack_versions,
            commands::modpacks::inspect_modpack,
            commands::modpacks::import_modpack,
            commands::modpacks::install_modpack,
            commands::modpacks::update_modpack,
            commands::modpacks::modpack_state,
            commands::modpacks::modpack_add_missing,
            commands::modpacks::export_candidates,
            commands::modpacks::export_modpack,
            commands::content::list_worlds,
            commands::content::backup_world,
            commands::content::delete_world,
            commands::java::detect_java,
            commands::java::test_java,
            commands::java::download_java,
            commands::java::system_info,
            commands::accounts::list_accounts,
            commands::accounts::add_offline_account,
            commands::accounts::remove_account,
            commands::accounts::set_default_account,
            commands::accounts::msa_status,
            commands::accounts::msa_begin,
            commands::accounts::msa_finish,
            commands::accounts::msa_cancel,
            commands::accounts::refresh_account,
            commands::accounts::account_skin,
        ])
        .run(tauri::generate_context!())
        .expect("ошибка при запуске приложения Tauri");
}
