//! macOS embedded surface: not implemented yet (planned: libmpv's render API into an
//! NSOpenGLView under the webview). Until then `player_start` uses external mpv,
//! remote-controlled over JSON IPC.

use tauri::AppHandle;
use zeiler_player::{PlayerEvent, Session};

use super::EmbeddedStatus;
pub(super) use super::stub::{Surface, after_start, on_stop, set_geometry};

const REASON: &str = "Built-in playback is not available on macOS yet; Zeiler plays in an \
                      mpv window and controls it from the player screen.";

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
