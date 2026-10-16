mod builtin;
mod commands;
mod error;
mod library;
mod persist;
mod settings_groups;
mod state;
mod subtitles;
mod updater;

use std::time::Duration;

use tauri::{Emitter, Manager};
use tracing::{error, info};

use crate::library::{LIBRARY_CHANGED, LibraryChanged, LibraryState};
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
                // the subtitle file picker, opened from Rust only: no capability grants
                // the webview its commands
                app.handle().plugin(tauri_plugin_dialog::init())?;
            }

            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let state =
                tauri::async_runtime::block_on(AppState::load(data_dir, cache_dir.clone()))?;
            let handle = app.handle().clone();
            let library = tauri::async_runtime::block_on(LibraryState::load(
                &state.profile_dir(),
                move |ids| {
                    let _ = handle.emit(LIBRARY_CHANGED, LibraryChanged { ids });
                },
            ))?;
            let prefs = tauri::async_runtime::block_on(subtitles::prefs::PlayerPrefs::load(
                &state.profile_dir(),
            ))?;
            app.manage(subtitles::SubtitleStore::new(&cache_dir));
            app.manage(state);
            app.manage(library);
            app.manage(prefs);
            app.manage(commands::mpv::PlayerState::default());
            updater::start(app.handle().clone());
            info!("zeiler started");
            Ok(())
        })
        .on_window_event(|window, event| {
            // a subtitle file dropped on the window goes to the running player
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                commands::mpv::subtitles::on_drop(window.app_handle(), paths);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::addons::addon_preview,
            commands::addons::addon_add,
            commands::addons::addon_remove,
            commands::addons::addon_list,
            commands::content::catalog_load,
            commands::content::meta_get,
            commands::content::streams_get,
            commands::content::subtitles_get,
            commands::engine::engine_resolve,
            commands::engine::engine_stats,
            commands::engine::engine_remove,
            // library
            commands::library::library_list,
            commands::library::library_get,
            commands::library::library_set,
            commands::library::library_mark_watched,
            commands::library::library_clear_progress,
            commands::settings::settings_get,
            commands::settings::settings_patch,
            commands::settings::settings_set,
            updater::update_status,
            updater::update_check,
            updater::update_install_now,
            commands::mpv::player_backends,
            commands::mpv::player_start,
            commands::mpv::player_load,
            commands::mpv::player_stop,
            commands::mpv::player_set_property,
            commands::mpv::player_command,
            commands::mpv::player_get_property,
            commands::mpv::player_set_geometry,
            commands::mpv::player_surface_refresh,
            commands::mpv::subtitles::player_subtitle_add,
            commands::mpv::subtitles::player_subtitle_reencode,
            commands::mpv::subtitles::player_subtitle_pick_local,
            commands::mpv::subtitles::player_set_sub_style,
            commands::mpv::subtitles::player_prefs_get,
            commands::mpv::subtitles::player_prefs_set,
            // deprecated aliases, kept for one release (commands/mpv/aliases.rs)
            commands::mpv::aliases::player_status,
            commands::mpv::aliases::player_play,
            commands::mpv::aliases::player_embedded_available,
            commands::mpv::aliases::player_embedded_start,
            commands::mpv::aliases::player_embedded_stop,
            commands::mpv::aliases::player_embedded_set_property,
            commands::mpv::aliases::player_embedded_command,
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            error!("failed to build the app: {e}");
            std::process::exit(1);
        })
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // write the progress of whatever is still playing (bounded wait)
                if let Some(player) = app.try_state::<commands::mpv::PlayerState>()
                    && let Some(written) = player.close_progress()
                {
                    let _ = tauri::async_runtime::block_on(tokio::time::timeout(
                        Duration::from_secs(3),
                        written,
                    ));
                }
                // flush engine persistence before the process dies
                if let Some(state) = app.try_state::<AppState>()
                    && let Some(engine) = state.engine_if_running()
                {
                    tauri::async_runtime::block_on(engine.shutdown());
                }
            }
        });
}
