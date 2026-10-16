mod builtin;
mod commands;
mod error;
mod persist;
mod state;
mod updater;

use tauri::Manager;
use tracing::{error, info};

use crate::state::AppState;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,librqbit=warn".into()),
        )
        .init();

    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        // single-instance must be the first registered plugin
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    }

    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(desktop)]
            {
                app.handle()
                    .plugin(tauri_plugin_window_state::Builder::default().build())?;
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
            }

            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let state = tauri::async_runtime::block_on(AppState::load(data_dir, cache_dir))?;
            app.manage(state);
            // a unit struct without the `embedded` feature, a real state holder with it
            #[allow(clippy::default_constructed_unit_structs)]
            app.manage(commands::mpv::PlayerState::default());
            updater::start(app.handle().clone());
            info!("zeiler started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::addons::addon_preview,
            commands::addons::addon_add,
            commands::addons::addon_remove,
            commands::addons::addon_list,
            commands::content::catalog_load,
            commands::content::meta_get,
            commands::content::streams_get,
            commands::engine::engine_resolve,
            commands::engine::engine_stats,
            commands::engine::engine_remove,
            commands::player::player_status,
            commands::player::player_play,
            commands::player::player_stop,
            commands::settings::settings_get,
            commands::settings::settings_patch,
            commands::settings::settings_set,
            updater::update_status,
            updater::update_check,
            updater::update_install_now,
            commands::mpv::player_embedded_available,
            commands::mpv::player_embedded_start,
            commands::mpv::player_embedded_stop,
            commands::mpv::player_embedded_set_property,
            commands::mpv::player_embedded_command,
            commands::mpv::player_set_geometry,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            error!("failed to build the app: {e}");
            std::process::exit(1);
        })
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // flush engine persistence before the process dies
                if let Some(state) = app.try_state::<AppState>()
                    && let Some(engine) = state.engine_if_running()
                {
                    tauri::async_runtime::block_on(engine.shutdown());
                }
            }
        });
}
