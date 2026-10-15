use tauri::State;

use crate::error::IpcError;
use crate::state::{AppState, Settings};

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings, IpcError> {
    Ok(state.settings.read().await.clone())
}

/// Replace settings. Player path applies immediately; engine settings apply the next
/// time the engine starts (i.e. after an app restart if it is already running).
#[tauri::command]
pub async fn settings_set(
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, IpcError> {
    state
        .player
        .relocate(settings.mpv_path.as_deref().map(std::path::Path::new));
    {
        let mut current = state.settings.write().await;
        *current = settings;
    }
    state.persist_settings().await?;
    Ok(state.settings.read().await.clone())
}
