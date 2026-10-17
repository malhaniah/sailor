//! The main window's size, position and maximized state, kept across launches in the
//! device's `window.json` (app data dir).
//!
//! Zeiler keeps it itself instead of through tauri-plugin-window-state, which saves the
//! window as it is when the app quits: quit in the player's fullscreen and the next launch
//! opened at the fullscreen frame (on macOS also maximized, as tao reports a fullscreen
//! borderless window). On macOS it also took a maximized borderless window's frame as its
//! size, so a window brought back maximized had nothing smaller to go back to. Here the
//! size and position are those the window last had while normal (neither fullscreen,
//! maximized nor minimized), read once it has settled: maximizing animates on macOS, and
//! on Windows a maximize's move arrives before tao knows the window is maximized.
//! Fullscreen is never kept: only the player has a way out of it.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, Window, WindowEvent};
use tracing::warn;

use crate::persist::JsonStore;

const FILE: &str = "window.json";
const FILE_VERSION: u32 = 1;
/// The window this keeps (tauri.conf.json).
const MAIN: &str = "main";
/// How long the window must stay put before it is read.
const SETTLE: Duration = Duration::from_millis(500);
/// The window goes back to a place only if a monitor shows the point this far below the
/// middle of its top edge (its top bar, where it is dragged from), in physical pixels.
const TOP_INSET: i32 = 8;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowFile {
    version: u32,
    /// The window when it was last normal.
    normal: Option<Bounds>,
    maximized: bool,
}

/// Outer position and inner size, in physical pixels (what tao reports and takes).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// The window as read: maximized, or normal with its bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Seen {
    Maximized,
    Normal(Bounds),
}

impl WindowFile {
    /// This file after seeing the window: a maximized window keeps the bounds it had
    /// before.
    fn after(&self, seen: Seen) -> Self {
        let (normal, maximized) = match seen {
            Seen::Maximized => (self.normal, true),
            Seen::Normal(bounds) => (Some(bounds), false),
        };
        Self {
            version: FILE_VERSION,
            normal,
            maximized,
        }
    }
}

pub struct WindowState {
    file: JsonStore<WindowFile>,
    /// Bumped by every move and resize: a pending read goes ahead only if no later one
    /// replaced it.
    generation: AtomicU64,
}

impl WindowState {
    pub async fn load(data_dir: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            file: JsonStore::load(data_dir.join(FILE)).await?,
            generation: AtomicU64::new(0),
        })
    }

    /// Put the main window back as it was (in setup: the config's windows exist by then).
    /// A place no monitor shows any more is left to the config (centered).
    pub async fn restore<R: Runtime>(&self, app: &AppHandle<R>) {
        let Some(window) = app.get_webview_window(MAIN) else {
            return;
        };
        let saved = self.file.get().await;
        let monitors: Vec<_> = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .map(|m| (*m.position(), *m.size()))
            .collect();
        if let Some(bounds) = saved.normal.filter(|b| shown(b, &monitors)) {
            let _ = window.set_position(PhysicalPosition::new(bounds.x, bounds.y));
            let _ = window.set_size(PhysicalSize::new(bounds.width, bounds.height));
        }
        if saved.maximized {
            let _ = window.maximize();
        }
    }

    /// Read `window` and keep what it shows; nothing while it is fullscreen or minimized.
    async fn save<R: Runtime>(&self, window: &Window<R>) {
        let Some(seen) = see(window) else {
            return;
        };
        let saved = self
            .file
            .try_update(|file| {
                let next = file.after(seen);
                if next == *file {
                    return Ok(None);
                }
                *file = next;
                Ok(Some(()))
            })
            .await;
        if let Err(e) = saved {
            warn!("could not keep the window's size and position: {e}");
        }
    }
}

/// A window event (`on_window_event`): keep the main window's state once a move or
/// resize has settled, and as it closes (on Windows and Linux it is gone by the app's
/// exit).
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if window.label() != MAIN {
        return;
    }
    let Some(state) = window.try_state::<WindowState>() else {
        return;
    };
    match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
            let window = window.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(SETTLE).await;
                let state = window.state::<WindowState>();
                if state.generation.load(Ordering::SeqCst) == generation {
                    state.save(&window).await;
                }
            });
        }
        WindowEvent::CloseRequested { .. } => tauri::async_runtime::block_on(state.save(window)),
        _ => {}
    }
}

/// The app is exiting (RunEvent::Exit): keep the main window's last state if it is still
/// open (macOS Quit), without waiting for a move to settle.
pub fn on_exit<R: Runtime>(app: &AppHandle<R>) {
    let (Some(state), Some(window)) =
        (app.try_state::<WindowState>(), app.get_webview_window(MAIN))
    else {
        return;
    };
    tauri::async_runtime::block_on(state.save(&window.as_ref().window()));
}

/// The window now; `None` while it is fullscreen or minimized, or can't be read.
fn see<R: Runtime>(window: &Window<R>) -> Option<Seen> {
    if window.is_fullscreen().ok()? || window.is_minimized().ok()? {
        return None;
    }
    if window.is_maximized().ok()? {
        return Some(Seen::Maximized);
    }
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    (size.width > 0 && size.height > 0).then_some(Seen::Normal(Bounds {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }))
}

/// Whether a monitor shows the window's top bar ([`TOP_INSET`]).
fn shown(bounds: &Bounds, monitors: &[(PhysicalPosition<i32>, PhysicalSize<u32>)]) -> bool {
    let x = i64::from(bounds.x) + i64::from(bounds.width / 2);
    let y = i64::from(bounds.y) + i64::from(TOP_INSET);
    monitors.iter().any(|(position, size)| {
        let (left, top) = (i64::from(position.x), i64::from(position.y));
        (left..left + i64::from(size.width)).contains(&x)
            && (top..top + i64::from(size.height)).contains(&y)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::tests::temp_dir;

    const SMALL: Bounds = Bounds {
        x: 400,
        y: 240,
        width: 2000,
        height: 1360,
    };

    #[test]
    fn a_normal_window_keeps_its_bounds() {
        let file = WindowFile::default().after(Seen::Normal(SMALL));
        assert_eq!(
            file,
            WindowFile {
                version: FILE_VERSION,
                normal: Some(SMALL),
                maximized: false,
            }
        );
    }

    #[test]
    fn a_maximized_window_keeps_the_bounds_it_had_before() {
        let file = WindowFile::default()
            .after(Seen::Normal(SMALL))
            .after(Seen::Maximized);
        assert_eq!(file.normal, Some(SMALL));
        assert!(file.maximized);
        // and goes back to normal at its new bounds
        let moved = Bounds {
            x: 0,
            y: 0,
            ..SMALL
        };
        let file = file.after(Seen::Normal(moved));
        assert_eq!(file.normal, Some(moved));
        assert!(!file.maximized);
    }

    #[test]
    fn a_window_maximized_at_first_launch_has_no_bounds_yet() {
        let file = WindowFile::default().after(Seen::Maximized);
        assert_eq!(file.normal, None);
        assert!(file.maximized);
    }

    #[test]
    fn a_place_goes_back_only_where_a_monitor_shows_its_top() {
        // a 2880×1864 monitor at the origin and a 1920×1080 one to its right
        let monitors = [
            (PhysicalPosition::new(0, 0), PhysicalSize::new(2880, 1864)),
            (
                PhysicalPosition::new(2880, 0),
                PhysicalSize::new(1920, 1080),
            ),
        ];
        assert!(shown(&SMALL, &monitors));
        let on_second = Bounds { x: 3000, ..SMALL };
        assert!(shown(&on_second, &monitors));
        // the second monitor unplugged
        assert!(!shown(&on_second, &monitors[..1]));
        // its top above every monitor, the rest of it on one
        assert!(!shown(&Bounds { y: -200, ..SMALL }, &monitors));
        // half off the left edge still shows the middle of its top
        assert!(shown(&Bounds { x: -900, ..SMALL }, &monitors));
        assert!(!shown(&SMALL, &[]));
    }

    #[tokio::test]
    async fn the_file_round_trips() {
        let dir = temp_dir("window-state");
        let state = WindowState::load(&dir).await.unwrap();
        state
            .file
            .update(|file| *file = file.after(Seen::Normal(SMALL)).after(Seen::Maximized))
            .await
            .unwrap();
        let again = WindowState::load(&dir).await.unwrap();
        assert_eq!(again.file.get().await, state.file.get().await);
        let raw = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(raw.contains("\"version\": 1"), "{raw}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
