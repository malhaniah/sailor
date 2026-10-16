//! Linux embedded surface: not implemented (a GtkGLArea under a transparent
//! WebKitGTK is unproven). Linux keeps mpv in its own window, remote-controlled over
//! JSON IPC, with the full controls on Zeiler's player screen.

use tauri::AppHandle;
use zeiler_player::{PlayerEvent, Session};

use super::EmbeddedStatus;
pub(super) use super::stub::{Surface, after_start, on_stop, set_geometry};

const REASON: &str = "Built-in playback is not available on Linux; Zeiler plays in an mpv \
                      window and controls it from the player screen.";

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
