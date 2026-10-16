use tauri::State;
use zeiler_engine::{ResolvedStream, TorrentStreamRequest};

use crate::error::IpcError;
use crate::state::AppState;

/// Resolve a torrent stream (infoHash + fileIdx + sources) into a playable loopback URL.
/// Starts the engine on first use.
#[tauri::command]
pub async fn engine_resolve(
    state: State<'_, AppState>,
    request: TorrentStreamRequest,
) -> Result<ResolvedStream, IpcError> {
    let engine = state.engine().await?;
    engine
        .resolve(request)
        .await
        .map_err(|e| IpcError::Engine(format!("{e:#}")))
}

/// Live stats for a resolved torrent (librqbit stats JSON passthrough).
#[tauri::command]
pub async fn engine_stats(
    state: State<'_, AppState>,
    info_hash: String,
) -> Result<serde_json::Value, IpcError> {
    let engine = state
        .engine_if_running()
        .ok_or_else(|| IpcError::Engine("engine is not running".into()))?;
    engine
        .stats(&info_hash)
        .map_err(|e| IpcError::Engine(format!("{e:#}")))
}

/// Stop a torrent; optionally delete its cached data.
#[tauri::command]
pub async fn engine_remove(
    state: State<'_, AppState>,
    info_hash: String,
    delete_files: bool,
) -> Result<(), IpcError> {
    let Some(engine) = state.engine_if_running() else {
        return Ok(());
    };
    engine
        .remove(&info_hash, delete_files)
        .await
        .map_err(|e| IpcError::Engine(format!("{e:#}")))
}
