//! Linux embedded surface: not implemented (a GtkGLArea under a transparent
//! WebKitGTK is unproven). Linux keeps mpv in its own window, remote-controlled over
//! JSON IPC, with the full controls on Zeiler's player screen; the stub's no-ops (and
//! its Linux reason text) stand in.

pub(super) use super::stub::{
    Surface, after_start, embedded_status, on_stop, refresh_surface, set_geometry, spawn_embedded,
};
