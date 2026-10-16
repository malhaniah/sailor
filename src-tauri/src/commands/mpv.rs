//! Embedded libmpv playback (Windows-first, cargo feature `embedded`).
//!
//! The video is rendered by mpv into a child window of the main Tauri window,
//! pinned to the bottom of the z-order, while the WebView2 on top is given a
//! fully transparent default background — so the React UI draws over the video
//! wherever it paints, and the video shows through wherever it doesn't.
//! Windows wiring adapted from Harbor (github.com/harborstremio/harbor, MIT); see
//! THIRD-PARTY-NOTICES.md.
//!
//! libmpv-2.dll is linked DELAYLOAD (see build.rs): when the feature is off or
//! the DLL is absent at runtime, `player_embedded_available` reports false and
//! the UI falls back to the external-mpv backend (`player_play`).
//!
//! Events flow UI-ward as the `player://event` Tauri event (time-pos throttled
//! to 200 ms in zeiler-player). Property writes and commands from the UI go
//! through allow-lists — the webview is our own code, but addon-controlled
//! strings pass through it, and mpv properties reach the filesystem and
//! network.

use serde::Deserialize;
use tauri::AppHandle;

use crate::error::IpcError;
use zeiler_player::PlaybackRequest;

/// Target rectangle for the video child window, in physical pixels relative to
/// the main window's client area (the UI computes getBoundingClientRect() ×
/// devicePixelRatio).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
// the stub backend deserializes but never reads the fields
#[cfg_attr(not(all(windows, feature = "embedded")), allow(dead_code))]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub use imp::PlayerState;

/// Properties the UI may set. Everything else is refused.
const PROPERTY_ALLOW: &[&str] = &[
    "pause",
    "volume",
    "mute",
    "speed",
    "aid",
    "sid",
    "vid",
    "sub-delay",
    "audio-delay",
    "sub-scale",
    "sub-pos",
];

/// Commands the UI may run (first argv token). Everything else is refused.
const COMMAND_ALLOW: &[&str] = &["seek", "cycle", "stop", "frame-step", "frame-back-step"];

fn check_scheme(url: &str) -> Result<(), IpcError> {
    let scheme = url.split("://").next().unwrap_or("").to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(IpcError::InvalidInput(format!(
            "refusing to play a '{scheme}' URL (only http/https streams are supported)"
        )));
    }
    Ok(())
}

/// True when embedded playback can actually be used right now (feature compiled
/// in AND libmpv-2.dll loadable). The UI uses this to pick a backend.
#[tauri::command]
pub async fn player_embedded_available(app: AppHandle) -> Result<bool, IpcError> {
    Ok(imp::available(&app))
}

/// Start (or replace) embedded playback of an http(s) stream.
#[tauri::command]
pub async fn player_embedded_start(
    app: AppHandle,
    state: tauri::State<'_, PlayerState>,
    request: PlaybackRequest,
) -> Result<(), IpcError> {
    check_scheme(&request.url)?;
    imp::start(&app, &state, request)
}

/// Stop embedded playback and tear the mpv instance down.
#[tauri::command]
pub async fn player_embedded_stop(state: tauri::State<'_, PlayerState>) -> Result<(), IpcError> {
    imp::stop(&state);
    Ok(())
}

/// Position the video child window (physical px, client-area relative).
#[tauri::command]
pub async fn player_set_geometry(
    app: AppHandle,
    state: tauri::State<'_, PlayerState>,
    geometry: Geometry,
) -> Result<(), IpcError> {
    imp::set_geometry(&app, &state, geometry)
}

/// Set an allow-listed mpv property (pause, volume, track selection, …).
#[tauri::command]
pub async fn player_embedded_set_property(
    state: tauri::State<'_, PlayerState>,
    name: String,
    value: serde_json::Value,
) -> Result<(), IpcError> {
    if !PROPERTY_ALLOW.contains(&name.as_str()) {
        return Err(IpcError::InvalidInput(format!(
            "property '{name}' is not settable from the UI"
        )));
    }
    imp::set_property(&state, name, value)
}

/// Run an allow-listed mpv command (seek, cycle, …).
#[tauri::command]
pub async fn player_embedded_command(
    state: tauri::State<'_, PlayerState>,
    args: Vec<String>,
) -> Result<(), IpcError> {
    match args.first() {
        Some(head) if COMMAND_ALLOW.contains(&head.as_str()) => imp::command(&state, args),
        Some(head) => Err(IpcError::InvalidInput(format!(
            "command '{head}' is not runnable from the UI"
        ))),
        None => Err(IpcError::InvalidInput("empty command".into())),
    }
}

// --- real implementation (Windows + `embedded`) -----------------------------------------

#[cfg(all(windows, feature = "embedded"))]
mod imp {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tauri::{AppHandle, Emitter, Manager};
    use tracing::{info, warn};
    use zeiler_player::PlaybackRequest;
    use zeiler_player::embedded::EmbeddedPlayer;

    use super::Geometry;
    use crate::error::IpcError;

    #[derive(Default)]
    pub struct PlayerState {
        inner: Arc<Inner>,
    }

    #[derive(Default)]
    struct Inner {
        session: Mutex<Option<EmbeddedPlayer>>,
        /// Last rect requested by the UI (physical px). Re-applied whenever the
        /// mpv child window (re)appears.
        geometry: Mutex<Option<(i32, i32, i32, i32)>>,
        /// The mpv child HWND currently pinned + subclassed.
        pinned: Mutex<Option<isize>>,
        /// Bumped per start; outstanding pin threads exit when stale.
        epoch: AtomicU64,
    }

    pub(super) fn available(_app: &AppHandle) -> bool {
        win32::ensure_libmpv().is_ok()
    }

    pub(super) fn start(
        app: &AppHandle,
        state: &PlayerState,
        request: PlaybackRequest,
    ) -> Result<(), IpcError> {
        win32::ensure_libmpv().map_err(IpcError::Player)?;
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| IpcError::Player("main window missing".into()))?;
        win32::set_webview_transparent(&window);
        let parent = window
            .hwnd()
            .map_err(|e| IpcError::Player(format!("window handle: {e}")))?
            .0 as isize;

        // Replace any previous instance (its child window dies with its core).
        if let Some(previous) = state.inner.session.lock().unwrap().take() {
            previous.quit();
        }
        *state.inner.pinned.lock().unwrap() = None;

        let emitter = app.clone();
        let player = EmbeddedPlayer::spawn(Some(parent as i64), move |event| {
            let _ = emitter.emit("player://event", &event);
        })
        .map_err(|e| IpcError::Player(e.to_string()))?;
        player
            .load(request)
            .map_err(|e| IpcError::Player(e.to_string()))?;
        *state.inner.session.lock().unwrap() = Some(player);

        // mpv creates its child window asynchronously (force-window=immediate);
        // poll briefly so it gets pinned below the webview before first paint.
        let inner = Arc::clone(&state.inner);
        let epoch = inner.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::spawn(move || {
            for _ in 0..60 {
                if inner.epoch.load(Ordering::SeqCst) != epoch {
                    return;
                }
                if win32::sync_child(parent, &inner) {
                    info!("mpv child window pinned under the webview");
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            warn!("mpv child window did not appear within 3s");
        });
        Ok(())
    }

    pub(super) fn stop(state: &PlayerState) {
        state.inner.epoch.fetch_add(1, Ordering::SeqCst);
        if let Some(session) = state.inner.session.lock().unwrap().take() {
            session.quit();
        }
        *state.inner.pinned.lock().unwrap() = None;
        *state.inner.geometry.lock().unwrap() = None;
    }

    pub(super) fn set_geometry(
        app: &AppHandle,
        state: &PlayerState,
        geometry: Geometry,
    ) -> Result<(), IpcError> {
        let rect = (
            geometry.x.round() as i32,
            geometry.y.round() as i32,
            (geometry.width.round() as i32).max(1),
            (geometry.height.round() as i32).max(1),
        );
        *state.inner.geometry.lock().unwrap() = Some(rect);
        if state.inner.session.lock().unwrap().is_none() {
            return Ok(()); // remembered for the next start
        }
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| IpcError::Player("main window missing".into()))?;
        let parent = window
            .hwnd()
            .map_err(|e| IpcError::Player(format!("window handle: {e}")))?
            .0 as isize;
        win32::sync_child(parent, &state.inner);
        Ok(())
    }

    pub(super) fn set_property(
        state: &PlayerState,
        name: String,
        value: serde_json::Value,
    ) -> Result<(), IpcError> {
        session(state)?
            .set_property(name, value)
            .map_err(|e| IpcError::Player(e.to_string()))
    }

    pub(super) fn command(state: &PlayerState, args: Vec<String>) -> Result<(), IpcError> {
        session(state)?
            .command(args)
            .map_err(|e| IpcError::Player(e.to_string()))
    }

    /// Clone-free access is impossible (the lock must not be held across mpv
    /// calls that could block), so commands run on a brief lock + channel send.
    fn session(
        state: &PlayerState,
    ) -> Result<impl std::ops::Deref<Target = EmbeddedPlayer>, IpcError> {
        let guard = state.inner.session.lock().unwrap();
        if guard.is_none() {
            return Err(IpcError::Player("embedded player is not running".into()));
        }
        struct SessionRef<'a>(std::sync::MutexGuard<'a, Option<EmbeddedPlayer>>);
        impl std::ops::Deref for SessionRef<'_> {
            type Target = EmbeddedPlayer;
            fn deref(&self) -> &EmbeddedPlayer {
                self.0.as_ref().expect("checked above")
            }
        }
        Ok(SessionRef(guard))
    }

    mod win32 {
        use std::sync::OnceLock;

        use tracing::{debug, warn};
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::Graphics::Gdi::{
            RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
        };
        use windows::Win32::System::LibraryLoader::LoadLibraryW;
        use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumChildWindows, GWL_EXSTYLE, GetClassNameW, GetClientRect, GetWindowLongW,
            HTTRANSPARENT, HWND_BOTTOM, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_SHOWWINDOW, SetWindowLongW, SetWindowPos, WM_NCHITTEST, WS_EX_TRANSPARENT,
        };
        use windows::core::{BOOL, HSTRING};

        use super::Inner;

        /// Load libmpv-2.dll once (exe dir → dev vendor dir → default search
        /// order). The DLL import is DELAYLOAD, so pre-loading it here is what
        /// makes the first mpv call resolve; the module is never freed.
        pub fn ensure_libmpv() -> Result<(), String> {
            static LOADED: OnceLock<Result<(), String>> = OnceLock::new();
            LOADED
                .get_or_init(|| {
                    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
                    if let Ok(exe) = std::env::current_exe()
                        && let Some(dir) = exe.parent()
                    {
                        candidates.push(dir.join("libmpv-2.dll"));
                    }
                    // dev builds: the vendored copy next to this crate
                    candidates.push(
                        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("libmpv")
                            .join("libmpv-2.dll"),
                    );
                    for candidate in &candidates {
                        if candidate.is_file()
                            && unsafe { LoadLibraryW(&HSTRING::from(candidate.as_os_str())) }
                                .is_ok()
                        {
                            debug!(path = %candidate.display(), "libmpv-2.dll loaded");
                            return Ok(());
                        }
                    }
                    unsafe { LoadLibraryW(&HSTRING::from("libmpv-2.dll")) }
                        .map(|_| ())
                        .map_err(|e| {
                            format!(
                                "libmpv-2.dll could not be loaded ({e}); embedded playback is \
                                 unavailable. Run `node scripts/fetch-libmpv.mjs` and restart."
                            )
                        })
                })
                .clone()
        }

        /// Make the WebView2 compositor background fully transparent so the
        /// video child window shows through wherever the page doesn't paint.
        pub fn set_webview_transparent(window: &tauri::WebviewWindow) {
            let result = window.with_webview(|webview| unsafe {
                use webview2_com::Microsoft::Web::WebView2::Win32::{
                    COREWEBVIEW2_COLOR, ICoreWebView2Controller2,
                };
                use windows::core::Interface;
                match webview.controller().cast::<ICoreWebView2Controller2>() {
                    Ok(controller2) => {
                        let transparent = COREWEBVIEW2_COLOR {
                            A: 0,
                            R: 0,
                            G: 0,
                            B: 0,
                        };
                        if let Err(e) = controller2.SetDefaultBackgroundColor(transparent) {
                            warn!("SetDefaultBackgroundColor failed: {e:?}");
                        }
                    }
                    Err(e) => warn!("cast to ICoreWebView2Controller2 failed: {e:?}"),
                }
            });
            if let Err(e) = result {
                warn!("with_webview failed: {e}");
            }
        }

        const SUBCLASS_ID: usize = 0x5A1_10B; // arbitrary, stable

        /// The mpv child must never eat mouse input meant for the UI above it.
        unsafe extern "system" fn subclass_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
            _id: usize,
            _data: usize,
        ) -> LRESULT {
            if msg == WM_NCHITTEST {
                return LRESULT(HTTRANSPARENT as i32 as isize);
            }
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }

        /// Find mpv's child window(s) under `parent`, pin the active one at the
        /// bottom of the z-order with the last requested geometry, and park any
        /// leftovers from a dying instance off-screen. Returns true once a
        /// window was pinned.
        pub fn sync_child(parent: isize, inner: &Inner) -> bool {
            let children = enumerate_mpv_children(parent);
            let Some(&first) = children.first() else {
                return false;
            };

            let (target, newly_pinned) = {
                let mut pinned = inner.pinned.lock().unwrap();
                let target = match *pinned {
                    Some(hwnd) if children.contains(&hwnd) => hwnd,
                    _ => first,
                };
                let newly = *pinned != Some(target);
                *pinned = Some(target);
                (target, newly)
            };
            let rect = *inner.geometry.lock().unwrap();

            unsafe {
                let hwnd = HWND(target as *mut _);
                if newly_pinned {
                    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE);
                    SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_TRANSPARENT.0 as i32);
                    // Best effort: comctl32 subclassing is same-thread only in
                    // theory; in practice this works for mpv's child (and the
                    // webview above it receives all input regardless).
                    let _ = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
                }
                let (x, y, w, h) = rect.unwrap_or_else(|| {
                    let mut client = windows::Win32::Foundation::RECT::default();
                    let _ = GetClientRect(HWND(parent as *mut _), &mut client);
                    (0, 0, client.right.max(1), client.bottom.max(1))
                });
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_BOTTOM),
                    x,
                    y,
                    w,
                    h,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                // Park children of a previous, still-dying instance off-screen.
                for &leftover in children.iter().filter(|&&c| c != target) {
                    let _ = SetWindowPos(
                        HWND(leftover as *mut _),
                        Some(HWND_BOTTOM),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_HIDEWINDOW,
                    );
                }
                let _ = RedrawWindow(
                    Some(hwnd),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
                );
            }
            true
        }

        fn enumerate_mpv_children(parent: isize) -> Vec<isize> {
            let mut found: Vec<isize> = Vec::new();

            unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
                let mut class_buf = [0u16; 64];
                let len = unsafe { GetClassNameW(hwnd, &mut class_buf) };
                let class_name = String::from_utf16_lossy(&class_buf[..len.max(0) as usize]);
                if class_name == "mpv" || class_name.starts_with("mpv ") {
                    let found = unsafe { &mut *(lparam.0 as *mut Vec<isize>) };
                    found.push(hwnd.0 as isize);
                }
                BOOL(1)
            }

            unsafe {
                let _ = EnumChildWindows(
                    Some(HWND(parent as *mut _)),
                    Some(enum_proc),
                    LPARAM(&mut found as *mut Vec<isize> as isize),
                );
            }
            found
        }
    }
}

// --- stub (feature off, or not Windows) -------------------------------------------------

#[cfg(not(all(windows, feature = "embedded")))]
mod imp {
    use tauri::AppHandle;
    use zeiler_player::PlaybackRequest;

    use super::Geometry;
    use crate::error::IpcError;

    #[derive(Default)]
    pub struct PlayerState;

    const UNAVAILABLE: &str =
        "embedded playback is not available in this build (compile with --features embedded)";

    pub(super) fn available(_app: &AppHandle) -> bool {
        false
    }

    pub(super) fn start(
        _app: &AppHandle,
        _state: &PlayerState,
        _request: PlaybackRequest,
    ) -> Result<(), IpcError> {
        Err(IpcError::Player(UNAVAILABLE.into()))
    }

    pub(super) fn stop(_state: &PlayerState) {}

    pub(super) fn set_geometry(
        _app: &AppHandle,
        _state: &PlayerState,
        _geometry: Geometry,
    ) -> Result<(), IpcError> {
        Ok(())
    }

    pub(super) fn set_property(
        _state: &PlayerState,
        _name: String,
        _value: serde_json::Value,
    ) -> Result<(), IpcError> {
        Err(IpcError::Player(UNAVAILABLE.into()))
    }

    pub(super) fn command(_state: &PlayerState, _args: Vec<String>) -> Result<(), IpcError> {
        Err(IpcError::Player(UNAVAILABLE.into()))
    }
}
