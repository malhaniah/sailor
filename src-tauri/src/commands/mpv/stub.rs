//! No embedded surface in this build: the `embedded` feature is off, or the OS has
//! no embedded implementation yet (macOS: next; Linux: mpv keeps its own window).
//! `player_start` uses external mpv, remote-controlled from Zeiler's player screen.
//! The macOS and Linux surface modules re-export these no-ops.

use tauri::AppHandle;
use zeiler_player::{PlayerEvent, Session};

use super::{EmbeddedStatus, Geometry};
use crate::error::IpcError;

#[derive(Default)]
pub struct Surface;

/// Why built-in playback is unavailable, in the words Settings → Player shows.
const REASON: &str = if cfg!(target_os = "macos") {
    "Coming soon to macOS. Until then, videos play in an mpv window, with full controls \
     in Zeiler."
} else if cfg!(target_os = "linux") {
    "On Linux, videos play in an mpv window, with full controls in Zeiler."
} else if cfg!(windows) {
    "This build of Zeiler doesn't include the built-in player, so videos play in an mpv \
     window, with full controls in Zeiler."
} else {
    "Not available on this system. Videos play in an mpv window, with full controls in \
     Zeiler."
};

pub(super) fn embedded_status() -> EmbeddedStatus {
    EmbeddedStatus::unavailable(REASON.into())
}

pub(super) fn spawn_embedded(
    _app: &AppHandle,
    _surface: &Surface,
    _events: impl Fn(PlayerEvent) + Send + Sync + 'static,
) -> Result<Session, String> {
    Err(REASON.into())
}

pub(super) fn after_start(_app: &AppHandle, _surface: &Surface) {}

pub(super) fn on_stop(_app: &AppHandle, _surface: &Surface) {}

pub(super) fn refresh_surface(
    _app: &AppHandle,
    _surface: &Surface,
    _active: bool,
) -> Result<(), IpcError> {
    Ok(())
}

pub(super) fn set_geometry(
    _app: &AppHandle,
    _surface: &Surface,
    _geometry: Geometry,
    _active: bool,
) -> Result<(), IpcError> {
    Ok(())
}
