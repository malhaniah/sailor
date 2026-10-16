//! No embedded surface in this build: the `embedded` feature is off (or the OS has
//! no embedded implementation). `player_start` uses external mpv; macOS and Linux
//! builds with the feature reuse these no-ops with their own reason text.

use tauri::AppHandle;
use zeiler_player::{PlayerEvent, Session};

use super::{EmbeddedStatus, Geometry};
use crate::error::IpcError;

#[derive(Default)]
pub struct Surface;

#[cfg_attr(feature = "embedded", allow(dead_code))]
const REASON: &str = "Built-in playback is not part of this build (it needs the `embedded` \
                      feature).";

#[cfg_attr(feature = "embedded", allow(dead_code))]
pub(super) fn embedded_status() -> EmbeddedStatus {
    EmbeddedStatus::unavailable(REASON.into())
}

#[cfg_attr(feature = "embedded", allow(dead_code))]
pub(super) fn spawn_embedded(
    _app: &AppHandle,
    _surface: &Surface,
    _events: impl Fn(PlayerEvent) + Send + Sync + 'static,
) -> Result<Session, String> {
    Err(REASON.into())
}

pub(super) fn after_start(_app: &AppHandle, _surface: &Surface) {}

pub(super) fn on_stop(_surface: &Surface) {}

pub(super) fn set_geometry(
    _app: &AppHandle,
    _surface: &Surface,
    _geometry: Geometry,
    _active: bool,
) -> Result<(), IpcError> {
    Ok(())
}
