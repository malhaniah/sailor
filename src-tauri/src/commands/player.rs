use serde::Serialize;
use tauri::State;
use zeiler_player::PlaybackRequest;

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStatus {
    pub available: bool,
    pub path: Option<String>,
}

#[tauri::command]
pub async fn player_status(state: State<'_, AppState>) -> Result<PlayerStatus, IpcError> {
    let path = state.player.binary();
    Ok(PlayerStatus {
        available: path.is_some(),
        path: path.map(|p| p.display().to_string()),
    })
}

/// Launch playback of a direct or engine-resolved URL in the external player.
/// Addon-supplied URLs are untrusted: only http(s) is ever handed to the player.
#[tauri::command]
pub async fn player_play(
    state: State<'_, AppState>,
    request: PlaybackRequest,
) -> Result<(), IpcError> {
    let scheme = request
        .url
        .split("://")
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(IpcError::InvalidInput(format!(
            "refusing to play a '{scheme}' URL (only http/https streams are supported)"
        )));
    }
    state.player.play(&request).await?;
    Ok(())
}

#[tauri::command]
pub async fn player_stop(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.player.stop();
    Ok(())
}
