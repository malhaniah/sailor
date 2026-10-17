mod account;
mod builtin;
mod catalog_cache;
mod commands;
mod deeplink;
mod error;
mod home_rows;
mod iptv;
mod library;
mod persist;
mod profiles;
mod secrets;
mod settings_groups;
mod state;
mod subtitles;
mod updater;
mod window_state;

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
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            // a stremio:// link opened while Zeiler runs (deeplink.rs)
            tauri::async_runtime::spawn(deeplink::handle_args(app.clone(), argv, false));
        }));
    }

    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(desktop)]
            {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
                // the subtitle file picker, opened from Rust only: no capability grants
                // the webview its commands
                app.handle().plugin(tauri_plugin_dialog::init())?;
            }
            commands::mpv::fix_fullscreen_client_area(app.handle());

            let data_dir = app.path().app_data_dir()?;
            // the main window's size, position and maximized state from the last run
            let window_state = tauri::async_runtime::block_on(async {
                let window_state = window_state::WindowState::load(&data_dir).await?;
                window_state.restore(app.handle()).await;
                anyhow::Ok(window_state)
            })?;
            app.manage(window_state);
            let cache_dir = app.path().app_cache_dir()?;
            let state =
                tauri::async_runtime::block_on(AppState::load(data_dir, cache_dir.clone()))?;
            let handle = app.handle().clone();
            let library = tauri::async_runtime::block_on(LibraryState::load(
                &state.profile_dir(),
                move |ids| {
                    let _ = handle.emit(LIBRARY_CHANGED, LibraryChanged { ids });
                    // a local change goes to the Stremio account (not one a sync made)
                    if let Some(account) = handle.try_state::<account::AccountState>()
                        && !account.syncing.load(std::sync::atomic::Ordering::SeqCst)
                    {
                        account.kick.notify_one();
                    }
                },
            ))?;
            let account_state = tauri::async_runtime::block_on(account::AccountState::load(
                &state.profile_dir(),
                zeiler_sync::StremioApi::new()?,
                account::secrets::Sessions::new(Box::new(account::secrets::KeyringStore)),
            ))?;
            let prefs = tauri::async_runtime::block_on(subtitles::prefs::PlayerPrefs::load(
                &state.profile_dir(),
            ))?;
            let profile_dir = state.profile_dir();
            app.manage(subtitles::SubtitleStore::new(&cache_dir));
            app.manage(zeiler_addon::VideoHasher::new()?);
            app.manage(commands::marketplace::MarketState::new(&cache_dir));
            app.manage(commands::addons::UpdateCache::default());
            app.manage(deeplink::DeepLinkState::default());
            app.manage(account_state);
            let deep_link_on =
                tauri::async_runtime::block_on(state.settings.read()).stremio_deep_link;
            app.manage(state);
            app.manage(catalog_cache::CatalogCache::default());
            app.manage(library);
            app.manage(prefs);
            app.manage(commands::mpv::PlayerState::default());
            // skip times (AniSkip, TheIntroDB): asked from Rust, the CSP keeps the
            // webview off those hosts
            app.manage(zeiler_skip::SkipClient::new(concat!(
                "Zeiler/",
                env!("CARGO_PKG_VERSION")
            ))?);
            deeplink::refresh_registration(app.handle(), deep_link_on);
            // a stremio:// link that started the app waits for the UI to ask for it
            tauri::async_runtime::block_on(deeplink::handle_args(
                app.handle().clone(),
                std::env::args().collect(),
                true,
            ));
            commands::account::start_background_sync(app.handle().clone());
            // IPTV accounts: the keychain store M12's sessions use, with IPTV's own
            // memory fallback (secrets.rs)
            let secrets = std::sync::Arc::new(secrets::Secrets::new(std::sync::Arc::new(
                account::secrets::KeyringStore,
            )));
            app.manage(tauri::async_runtime::block_on(iptv::IptvState::load(
                &profile_dir,
                secrets,
            ))?);
            updater::start(app.handle().clone());
            info!("zeiler started");
            Ok(())
        })
        .on_window_event(|window, event| {
            window_state::on_window_event(window, event);
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
            commands::addons::addon_reorder,
            commands::addons::addon_check_updates,
            commands::addons::addon_refresh,
            // marketplace
            commands::marketplace::marketplace_list,
            commands::marketplace::marketplace_categories,
            commands::marketplace::marketplace_detail,
            commands::marketplace::marketplace_refresh,
            deeplink::deeplink_take_pending,
            deeplink::deeplink_stremio_status,
            deeplink::deeplink_set_stremio,
            // profiles
            commands::profiles::profile_list,
            commands::profiles::profile_create,
            commands::profiles::profile_update,
            commands::profiles::profile_set_pin,
            commands::profiles::profile_set_avatar,
            commands::profiles::profile_select,
            commands::profiles::profile_delete,
            // Stremio account
            commands::account::account_status,
            commands::account::account_link_start,
            commands::account::account_link_poll,
            commands::account::account_login_password,
            commands::account::account_logout,
            commands::account::account_set_sync,
            commands::account::account_addons_preview,
            commands::account::account_addons_apply,
            commands::account::account_sync_now,
            commands::content::catalog_load,
            commands::content::meta_get,
            commands::content::streams_get,
            commands::content::subtitles_get,
            // live TV: the user's IPTV sources
            commands::iptv::iptv_sources,
            commands::iptv::iptv_source_add,
            commands::iptv::iptv_source_remove,
            commands::iptv::iptv_channels,
            commands::iptv::iptv_epg,
            commands::iptv::iptv_favorite_set,
            commands::iptv::iptv_resolve,
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
            commands::mpv::subtitles::player_video_hash,
            commands::mpv::audio::player_set_audio_normalize,
            // skip and next episode
            commands::skip::skip_segments_get,
            commands::mpv::osd::player_osd_prompt,
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
                window_state::on_exit(app);
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
