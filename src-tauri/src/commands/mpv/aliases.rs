//! Pre-M2 player command names, kept as thin aliases for one release (the UI moved to
//! the unified commands in M2b and no longer calls them). New code uses `player_start`,
//! `player_stop`, `player_set_property`, `player_command`, `player_get_property` and
//! `player_backends`.
//!
//! The aliases keep their old meaning where it differs from the new commands:
//! `player_play` starts external mpv (now remote-controlled over JSON IPC),
//! `player_embedded_start` starts embedded only (no fallback), and
//! `player_embedded_stop` stops only an embedded session, so leaving the "Playing in
//! mpv" screen still leaves external mpv playing.

use tauri::{AppHandle, State};
use zeiler_player::{Backend, PlaybackRequest, PlayerBackend};

use super::{ExternalStatus, PlayerState, check_request, external_status, imp, start};
use crate::error::IpcError;
use crate::state::AppState;

/// Deprecated: `player_backends().external`.
#[tauri::command]
pub async fn player_status(state: State<'_, AppState>) -> Result<ExternalStatus, IpcError> {
    Ok(external_status(&state))
}

/// Deprecated: `player_backends().embedded.available`.
#[tauri::command]
pub async fn player_embedded_available() -> Result<bool, IpcError> {
    Ok(imp::embedded_status().available)
}

/// Deprecated: `player_start { request, backend: "external" }`.
#[tauri::command]
pub async fn player_play(
    app: AppHandle,
    state: State<'_, AppState>,
    player: State<'_, PlayerState>,
    mut request: PlaybackRequest,
) -> Result<(), IpcError> {
    check_request(&mut request)?;
    start(
        &app,
        &state,
        &player,
        request,
        PlayerBackend::External,
        None,
    )
    .await
    .map(drop)
}

/// Deprecated: `player_start { request, backend: "embedded" }`.
#[tauri::command]
pub async fn player_embedded_start(
    app: AppHandle,
    state: State<'_, AppState>,
    player: State<'_, PlayerState>,
    mut request: PlaybackRequest,
) -> Result<(), IpcError> {
    check_request(&mut request)?;
    start(
        &app,
        &state,
        &player,
        request,
        PlayerBackend::Embedded,
        None,
    )
    .await
    .map(drop)
}

/// Deprecated: stops only an embedded session (see the module docs).
#[tauri::command]
pub async fn player_embedded_stop(
    app: AppHandle,
    player: State<'_, PlayerState>,
) -> Result<(), IpcError> {
    let _op = player.op.lock().await;
    if player.stop_session(Some(Backend::Embedded)).is_some() {
        imp::on_stop(&app, &player.surface);
    }
    Ok(())
}

/// Deprecated: `player_set_property`.
#[tauri::command]
pub async fn player_embedded_set_property(
    player: State<'_, PlayerState>,
    name: String,
    value: serde_json::Value,
) -> Result<(), IpcError> {
    super::player_set_property(player, name, value).await
}

/// Deprecated: `player_command`.
#[tauri::command]
pub async fn player_embedded_command(
    player: State<'_, PlayerState>,
    args: Vec<String>,
) -> Result<(), IpcError> {
    super::player_command(player, args).await
}
