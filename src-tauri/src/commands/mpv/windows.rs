//! Windows embedded surface (`embedded` feature): libmpv renders into a child
//! window of the main Tauri window, pinned to the bottom of the z-order, while the
//! WebView2 on top gets a fully transparent default background — so the React UI
//! draws over the video wherever it paints, and the video shows through wherever it
//! doesn't.
//!
//! Adapted from Harbor (github.com/harborstremio/harbor), Copyright (c) 2026
//! Harbor, MIT License — the child-window pinning (`mpv_force_below`), the WebView2
//! transparency re-apply (`webview_reapply_transparency`) and the refresh-after-
//! fullscreen idea; the MIT notice is in THIRD-PARTY-NOTICES.md.
//!
//! **Transparency policy.** The webview is transparent only while built-in video
//! plays: [`spawn_embedded`] (and every [`refresh_surface`]) makes the WebView2
//! default background transparent, and [`on_stop`] restores WebView2's opaque
//! default (white, as at startup). That white never shows: the page always paints its
//! own background except while a built-in session runs — the player screen turns
//! transparent only once `player_start` has answered "embedded" (`videoBehind` in
//! `usePlaybackSession.ts`), and paints the app's canvas again when the session ends.
//! Harbor instead makes the main webview transparent once at setup; Zeiler keeps the
//! rest of the app on the plain, well-trodden opaque path and confines the
//! composition trick to playback.
//!
//! **Keeping the surface right.** WebView2 can lose the transparency and the child
//! can lose its place at the bottom of the z-order after fullscreen, DPI or monitor
//! changes and minimize/restore. The UI calls `player_surface_refresh` on a short
//! kick schedule after those ([`refresh_surface`]: transparency again, child re-pinned
//! with the last geometry, keyboard focus back on the webview). mpv's child window
//! can also take the keyboard focus when it appears, so the first successful pin
//! hands the focus back once ([`after_start`]). The hand-back focuses the WebView2
//! itself (`MoveFocus`), and only while Zeiler is the foreground window ([`refocus`]).
//!
//! libmpv-2.dll is linked DELAYLOAD (see build.rs): when the DLL is absent at
//! runtime, [`embedded_status`] reports it unavailable and `player_start` falls back
//! to external mpv. Every libmpv call stays behind [`win32::ensure_libmpv`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tracing::{info, warn};
use zeiler_player::embedded::{EmbeddedOptions, EmbeddedPlayer, Surface as VideoSurface};
use zeiler_player::{PlayerEvent, Session};

use super::{EmbeddedStatus, Geometry};
use crate::error::IpcError;

/// Native surface state of the embedded player.
#[derive(Default)]
pub struct Surface {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Last rect requested by the UI (physical px). Re-applied whenever the
    /// mpv child window (re)appears.
    geometry: Mutex<Option<(i32, i32, i32, i32)>>,
    /// The mpv child HWND currently pinned + subclassed.
    pinned: Mutex<Option<isize>>,
    /// Bumped per start; outstanding pin threads exit when stale.
    epoch: AtomicU64,
}

pub(super) fn embedded_status() -> EmbeddedStatus {
    match win32::ensure_libmpv() {
        Ok(()) => EmbeddedStatus {
            available: true,
            reason: None,
            libmpv_version: zeiler_player::embedded::mpv_version(),
        },
        Err(reason) => EmbeddedStatus::unavailable(reason),
    }
}

fn main_window_handle(app: &AppHandle) -> Result<(tauri::WebviewWindow, isize), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window missing".to_string())?;
    let parent = window.hwnd().map_err(|e| format!("window handle: {e}"))?.0 as isize;
    Ok((window, parent))
}

/// Create the libmpv core embedded into the main window. Init and VO failures are
/// synchronous and come back as the error text.
pub(super) fn spawn_embedded(
    app: &AppHandle,
    surface: &Surface,
    events: impl Fn(PlayerEvent) + Send + Sync + 'static,
) -> Result<Session, String> {
    win32::ensure_libmpv()?;
    let (window, parent) = main_window_handle(app)?;
    win32::set_webview_background(&window, Background::Transparent);
    // the previous core's child window dies with it
    *surface.inner.pinned.lock().unwrap() = None;
    let options = EmbeddedOptions::new(VideoSurface::Wid(parent as i64));
    match EmbeddedPlayer::spawn(options, events) {
        Ok(player) => Ok(Session::Embedded(player)),
        Err(e) => {
            // nothing plays here after all (the caller falls back or fails)
            win32::set_webview_background(&window, Background::Opaque);
            Err(e.to_string())
        }
    }
}

/// The WebView2 default background (see the transparency policy in the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Background {
    Transparent,
    /// WebView2's own default, as at startup: opaque white.
    Opaque,
}

/// Give the keyboard focus back to the webview — mpv's child window (or a
/// fullscreen transition) may have taken it, which silences the player's shortcuts.
/// This focuses the WebView2 itself (`Webview::set_focus` → `MoveFocus`); the
/// window-level `set_focus` would only activate the top-level window, which is a
/// no-op for the foreground window and never moves focus off a child. Only while
/// Zeiler is the foreground window: a late pin or refresh must never pull focus from
/// another app the user has moved to.
fn refocus(window: &tauri::WebviewWindow, parent: isize) {
    if !win32::is_foreground(parent) {
        return;
    }
    let webview: &tauri::Webview = window.as_ref();
    if let Err(e) = webview.set_focus() {
        warn!("cannot focus the webview: {e}");
    }
}

/// After the first load: mpv creates its child window asynchronously
/// (force-window=immediate); poll briefly so it gets pinned below the webview before
/// first paint, then hand the keyboard focus back to the webview once (the new
/// child window may have taken it, which would silence the player's shortcuts).
pub(super) fn after_start(app: &AppHandle, surface: &Surface) {
    let (window, parent) = match main_window_handle(app) {
        Ok(found) => found,
        Err(e) => {
            warn!("cannot pin the mpv window: {e}");
            return;
        }
    };
    let inner = Arc::clone(&surface.inner);
    let epoch = inner.epoch.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        for _ in 0..60 {
            if inner.epoch.load(Ordering::SeqCst) != epoch {
                return;
            }
            if win32::sync_child(parent, &inner) {
                info!("mpv child window pinned under the webview");
                refocus(&window, parent);
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        warn!("mpv child window did not appear within 3s");
    });
}

/// Make fullscreen fill the screen when it starts from a maximized window.
///
/// tao 0.37.1 trims a maximized window without OS decorations (Zeiler's) to the
/// monitor's work area, also while it is fullscreen, and a window made fullscreen from
/// maximized stays maximized underneath. The window then covers the whole monitor but
/// its client area, the webview and so the video stop short by the taskbar's height,
/// with mpv's letterbox bars beside the video. tao fixed this upstream
/// (tauri-apps/tao#1353, after 0.37.1); this subclass does the same for the main window
/// and goes away with a tao that has it. Called once at setup.
pub(super) fn fix_fullscreen_client_area(app: &AppHandle) {
    let (window, parent) = match main_window_handle(app) {
        Ok(found) => found,
        Err(e) => {
            warn!("cannot subclass the main window: {e}");
            return;
        }
    };
    // a window can only be subclassed from the thread that owns it
    if let Err(e) = window.run_on_main_thread(move || win32::subclass_main_window(parent)) {
        warn!("cannot subclass the main window: {e}");
    }
}

/// No embedded session is left: forget the window and the geometry, and give the
/// webview its opaque default background back.
pub(super) fn on_stop(app: &AppHandle, surface: &Surface) {
    surface.inner.epoch.fetch_add(1, Ordering::SeqCst);
    *surface.inner.pinned.lock().unwrap() = None;
    *surface.inner.geometry.lock().unwrap() = None;
    match main_window_handle(app) {
        Ok((window, _)) => win32::set_webview_background(&window, Background::Opaque),
        Err(e) => warn!("cannot restore the webview background: {e}"),
    }
}

/// Re-assert the surface after a window change (fullscreen, resize, move, DPI):
/// the transparent background, the child pinned at the bottom with the last
/// geometry, and the keyboard focus on the webview. `active`: an embedded session
/// is running (otherwise there is nothing to refresh, and the opaque background
/// restored at stop must stay).
pub(super) fn refresh_surface(
    app: &AppHandle,
    surface: &Surface,
    active: bool,
) -> Result<(), IpcError> {
    if !active {
        return Ok(());
    }
    let (window, parent) = main_window_handle(app).map_err(IpcError::Player)?;
    win32::set_webview_background(&window, Background::Transparent);
    win32::sync_child(parent, &surface.inner);
    refocus(&window, parent);
    Ok(())
}

/// Position the video child window. `active`: an embedded session is running
/// (otherwise the rect is only remembered for the next start).
pub(super) fn set_geometry(
    app: &AppHandle,
    surface: &Surface,
    geometry: Geometry,
    active: bool,
) -> Result<(), IpcError> {
    let rect = (
        geometry.x.round() as i32,
        geometry.y.round() as i32,
        (geometry.width.round() as i32).max(1),
        (geometry.height.round() as i32).max(1),
    );
    *surface.inner.geometry.lock().unwrap() = Some(rect);
    if !active {
        return Ok(()); // remembered for the next start
    }
    let (_, parent) = main_window_handle(app).map_err(IpcError::Player)?;
    win32::sync_child(parent, &surface.inner);
    Ok(())
}

mod win32 {
    use std::sync::OnceLock;

    use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use ::windows::Win32::Graphics::Gdi::{
        RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
    };
    use ::windows::Win32::System::LibraryLoader::LoadLibraryW;
    use ::windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use ::windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetClientRect,
        GetForegroundWindow, GetWindowLongW, HTTRANSPARENT, HWND_BOTTOM, SWP_FRAMECHANGED,
        SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
        SetWindowLongW, SetWindowPos, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WS_CAPTION,
        WS_EX_TRANSPARENT,
    };
    use ::windows::core::{BOOL, HSTRING};
    use tracing::{debug, warn};

    use super::{Background, Inner};

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
                        && unsafe { LoadLibraryW(&HSTRING::from(candidate.as_os_str())) }.is_ok()
                    {
                        debug!(path = %candidate.display(), "libmpv-2.dll loaded");
                        return Ok(());
                    }
                }
                unsafe { LoadLibraryW(&HSTRING::from("libmpv-2.dll")) }
                    .map(|_| ())
                    .map_err(|e| {
                        if cfg!(debug_assertions) {
                            // development builds: point at the fix
                            format!(
                                "Built-in playback needs libmpv-2.dll, which could not be \
                                 loaded ({e}). Run `node scripts/fetch-libmpv.mjs` and \
                                 restart."
                            )
                        } else {
                            "Built-in playback can't start because one of its files is \
                             missing. Reinstalling Zeiler should fix it; until then videos \
                             play in an mpv window."
                                .to_string()
                        }
                    })
            })
            .clone()
    }

    /// Set the WebView2 compositor's default background: fully transparent so the
    /// video child window shows through wherever the page doesn't paint, or back to
    /// WebView2's opaque white default.
    pub fn set_webview_background(window: &tauri::WebviewWindow, background: Background) {
        let result = window.with_webview(move |webview| unsafe {
            use ::windows::core::Interface;
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                COREWEBVIEW2_COLOR, ICoreWebView2Controller2,
            };
            match webview.controller().cast::<ICoreWebView2Controller2>() {
                Ok(controller2) => {
                    let color = match background {
                        Background::Transparent => COREWEBVIEW2_COLOR {
                            A: 0,
                            R: 0,
                            G: 0,
                            B: 0,
                        },
                        Background::Opaque => COREWEBVIEW2_COLOR {
                            A: 255,
                            R: 255,
                            G: 255,
                            B: 255,
                        },
                    };
                    if let Err(e) = controller2.SetDefaultBackgroundColor(color) {
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

    /// `hwnd` (the main window) is the foreground window, i.e. Zeiler is the app
    /// the user is in.
    pub fn is_foreground(hwnd: isize) -> bool {
        unsafe { GetForegroundWindow() }.0 as isize == hwnd
    }

    const SUBCLASS_ID: usize = 0x5A1_10B; // arbitrary, stable
    const MAIN_SUBCLASS_ID: usize = 0x5A1_10C; // arbitrary, stable

    /// Subclass the main window so that a fullscreen window's client area is the whole
    /// window (see [`super::fix_fullscreen_client_area`]). On the window's own thread.
    pub fn subclass_main_window(parent: isize) {
        let hwnd = HWND(parent as *mut _);
        if !unsafe { SetWindowSubclass(hwnd, Some(main_subclass_proc), MAIN_SUBCLASS_ID, 0) }
            .as_bool()
        {
            warn!("cannot subclass the main window: fullscreen from maximized may stop short");
            return;
        }
        // recalculate the frame once, in case the window is fullscreen already
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    /// tao takes WS_CAPTION away from the window only while it is fullscreen; then the
    /// client area is the whole proposed window rect (returning 0 leaves `rgrc[0]`).
    /// Everything else, and every message but NCCALCSIZE, goes on to tao. Assumes the
    /// undecorated window Zeiler has (a menu bar would need its height taken off). One
    /// gap: tao skips writing the style while the window is minimized, so a fullscreen
    /// change made then is seen here only at tao's next style write.
    unsafe extern "system" fn main_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == WM_NCCALCSIZE && wparam.0 != 0 {
            let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
            if (style & WS_CAPTION.0) == 0 {
                return LRESULT(0);
            }
        }
        if msg == WM_NCDESTROY {
            let _ =
                unsafe { RemoveWindowSubclass(hwnd, Some(main_subclass_proc), MAIN_SUBCLASS_ID) };
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

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
            return LRESULT(HTTRANSPARENT as isize);
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
                let mut client = ::windows::Win32::Foundation::RECT::default();
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
