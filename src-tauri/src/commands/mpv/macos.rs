//! macOS embedded surface: not implemented yet (planned for M13: libmpv's render API
//! into an NSOpenGLView under the webview, `zeiler_player::embedded::Surface::RenderApi`).
//! Until then `player_start` uses external mpv, remote-controlled over JSON IPC, and
//! the stub's no-ops (and its macOS reason text) stand in.

pub(super) use super::stub::{
    Surface, after_start, embedded_status, on_stop, refresh_surface, set_geometry, spawn_embedded,
};
