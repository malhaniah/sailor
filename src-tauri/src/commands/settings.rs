use tauri::{AppHandle, Emitter, State};

use crate::error::IpcError;
use crate::state::{AppState, Settings};

/// Emitted with the full new `Settings` after every successful change, so every part
/// of the UI (and later, every window) sees the latest values.
pub const SETTINGS_CHANGED: &str = "settings://changed";

/// The current settings.
#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings, IpcError> {
    Ok(state.settings.read().await.clone())
}

/// Merge a partial update into the current settings, server-side, and return the
/// result. `patch` is an object of top-level camelCase setting names to new values;
/// unknown names and wrongly typed values reject the whole patch. Merging here (rather
/// than the UI sending a whole object) means two writers changing different keys can
/// never undo each other. The player path applies immediately; engine settings apply
/// the next time the engine starts (i.e. after an app restart if it is already running).
#[tauri::command]
pub async fn settings_patch(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<Settings, IpcError> {
    // the OS registration has to change with it: deeplink_set_stremio does both
    if patch.get("stremioDeepLink").is_some() {
        return Err(IpcError::InvalidInput(
            "Switch stremio:// links on or off with deeplink_set_stremio.".into(),
        ));
    }
    // naming mpvPath re-locates mpv even when unchanged (picks up a fresh install on PATH)
    let relocate = patch.get("mpvPath").is_some();
    update(&app, &state, relocate, |current| current.patched(patch)).await
}

/// Replace all settings. Deprecated alias kept for one release; use `settings_patch`.
#[tauri::command]
pub async fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, IpcError> {
    // stremioDeepLink changes only with the OS registration (deeplink_set_stremio)
    update(&app, &state, true, |current| {
        Ok(Settings {
            stremio_deep_link: current.stremio_deep_link,
            ..settings
        })
    })
    .await
}

/// Compute the next settings from the current ones, persist them, and only after the
/// write succeeds publish them, apply side effects and announce the change (all
/// serialized with other updates). A failed write changes nothing.
async fn update(
    app: &AppHandle,
    state: &AppState,
    relocate: bool,
    next: impl FnOnce(&Settings) -> Result<Settings, IpcError>,
) -> Result<Settings, IpcError> {
    state
        .update_settings(next, |after| {
            if relocate {
                state
                    .player
                    .relocate(after.mpv_path.as_deref().map(std::path::Path::new));
            }
            let _ = app.emit(SETTINGS_CHANGED, after);
        })
        .await
}
