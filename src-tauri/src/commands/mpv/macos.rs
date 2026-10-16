//! macOS embedded surface (`embedded` feature): libmpv's render API draws into an
//! `NSOpenGLView` placed under the WKWebView, inside the window's content view, while
//! the webview stops drawing its own background — so the React UI draws over the
//! video wherever it paints, and the video shows through wherever it doesn't.
//!
//! Adapted from Harbor's `src-tauri/src/mpv_render_mac.rs` (github.com/harborstremio/harbor
//! v0.8.5), Copyright (c) 2026 Harbor, MIT License — the GL view below the webview,
//! `drawsBackground` off, the OpenGL render context with `dlsym` for GL functions, and
//! redraws scheduled on the main queue; the MIT notice is in THIRD-PARTY-NOTICES.md.
//! Changed here: the geometry is converted to points (Harbor passed the page's
//! physical pixels as NSView coordinates, which misplaces the video on Retina
//! screens), the render context is owned through `zeiler_player::render` and freed
//! before its core quits, a new session takes over the surface of the one it
//! replaces, and the view is cleared to black until the first frame.
//!
//! **Threads.** Everything AppKit and GL happens on the main thread: the install and
//! the uninstall run through `with_webview` (the player's actor thread waits for
//! them, see [`Host`]), and mpv's update callback only schedules a redraw on the main
//! queue. The installed surface lives in a main-thread-only slot ([`EMBED`]).
//!
//! **Transparency policy** (as on Windows, see `windows.rs`): the webview stops drawing
//! its background only while built-in video plays, and draws it again when the surface
//! is removed. The page paints its own background except while that video runs.
//!
//! **Geometry.** The UI sends the video rectangle in physical pixels (CSS px ×
//! devicePixelRatio, as on Windows); here it becomes points (÷ the window's scale
//! factor) in the content view's bottom-up coordinates. The GL drawable is sized in
//! pixels from the view's backing store.
//!
//! libmpv is linked normally (bundled under `Contents/Frameworks`, see
//! `scripts/fetch-libmpv.mjs`), so when this code runs, libmpv is there.

// NSOpenGLView and NSOpenGLPixelFormat are deprecated (OpenGL runs on Metal since
// macOS 10.14) but still work; libmpv's render API has no Metal backend to move to.
#![allow(deprecated)]

use std::cell::{Cell, RefCell};
use std::ffi::{CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSOpenGLPixelFormat, NSOpenGLView, NSView, NSWindowOrderingMode,
};
use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, NSString};
use tauri::{AppHandle, Manager};
use tracing::{info, warn};
use zeiler_player::embedded::{EmbeddedOptions, EmbeddedPlayer, Surface as VideoSurface};
use zeiler_player::render::{GlRenderContext, RenderCore, RenderHost};
use zeiler_player::{PlayerEvent, Session};

use super::{EmbeddedStatus, Geometry};
use crate::error::IpcError;

/// How long the actor thread waits for the main thread to install or remove the
/// surface. The main thread only waits on mpv while it frees a render context.
const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(5);

/// Native surface state of the embedded player.
#[derive(Default)]
pub struct Surface {
    /// Last rect requested by the UI, in points (top-left origin); applied when the
    /// GL view is installed.
    geometry: Arc<Mutex<Option<Rect>>>,
}

/// A rectangle in points, top-left origin (as the page lays out).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

pub(super) fn embedded_status() -> EmbeddedStatus {
    EmbeddedStatus {
        available: true,
        reason: None,
        libmpv_version: zeiler_player::embedded::mpv_version(),
    }
}

fn main_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "main window missing".to_string())
}

/// Run `f` on the main thread with the WKWebView, and wait for its result.
fn on_main<T: Send + 'static>(
    window: &tauri::WebviewWindow,
    f: impl FnOnce(&NSView, MainThreadMarker) -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    window
        .with_webview(move |webview| {
            let Some(mtm) = MainThreadMarker::new() else {
                return; // with_webview always runs on the main thread
            };
            // SAFETY: wry's `inner()` on macOS is the window's live WKWebView, an
            // NSView subclass.
            let web_view = unsafe { &*(webview.inner() as *const NSView) };
            let _ = tx.send(f(web_view, mtm));
        })
        .map_err(|e| format!("cannot reach the webview: {e}"))?;
    rx.recv_timeout(MAIN_THREAD_TIMEOUT)
        .map_err(|_| "the window did not respond in time".to_string())
}

/// Create the libmpv core with the render API: its host installs the GL view and the
/// render context before the core reports ready, so a failure here is synchronous.
pub(super) fn spawn_embedded(
    app: &AppHandle,
    surface: &Surface,
    events: impl Fn(PlayerEvent) + Send + Sync + 'static,
) -> Result<Session, String> {
    let host = Arc::new(Host {
        window: main_window(app)?,
        id: NEXT_EMBED.fetch_add(1, Ordering::Relaxed) + 1,
        geometry: Arc::clone(&surface.geometry),
    });
    let options = EmbeddedOptions::new(VideoSurface::RenderApi(host));
    EmbeddedPlayer::spawn(options, events)
        .map(Session::Embedded)
        .map_err(|e| e.to_string())
}

pub(super) fn after_start(_app: &AppHandle, _surface: &Surface) {}

/// No embedded session is left: remove the surface (if its core has not done so
/// already) and give the webview its background back. Forget the geometry.
pub(super) fn on_stop(app: &AppHandle, surface: &Surface) {
    *surface.geometry.lock().unwrap() = None;
    let Ok(window) = main_window(app) else {
        return;
    };
    // Not waited for: the render context may take a moment to free, and nothing
    // here depends on it being gone.
    let _ = window.with_webview(|webview| {
        if let Some(mtm) = MainThreadMarker::new() {
            // SAFETY: as in `on_main`
            let web_view = unsafe { &*(webview.inner() as *const NSView) };
            uninstall(None, web_view, mtm);
        }
    });
}

/// Re-assert the surface after a window change: the transparent webview, the view's
/// frame from the last geometry, and a fresh frame.
pub(super) fn refresh_surface(
    app: &AppHandle,
    surface: &Surface,
    active: bool,
) -> Result<(), IpcError> {
    if !active {
        return Ok(());
    }
    let rect = *surface.geometry.lock().unwrap();
    let window = main_window(app).map_err(IpcError::Player)?;
    window
        .with_webview(move |webview| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            // SAFETY: as in `on_main`
            let web_view = unsafe { &*(webview.inner() as *const NSView) };
            EMBED.with_borrow(|embed| {
                if let Some(embed) = embed {
                    make_web_view_transparent(web_view);
                    embed.place(rect, mtm);
                }
            });
            render_now();
        })
        .map_err(|e| IpcError::Player(format!("cannot reach the webview: {e}")))
}

/// Position the video view (`geometry` in physical px, client-area relative).
/// `active`: an embedded session is running (otherwise the rect is only remembered
/// for the next start).
pub(super) fn set_geometry(
    app: &AppHandle,
    surface: &Surface,
    geometry: Geometry,
    active: bool,
) -> Result<(), IpcError> {
    let window = main_window(app).map_err(IpcError::Player)?;
    let scale = window
        .scale_factor()
        .ok()
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(1.0);
    let rect = to_points(geometry, scale);
    *surface.geometry.lock().unwrap() = Some(rect);
    if !active {
        return Ok(());
    }
    window
        .with_webview(move |_| {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            EMBED.with_borrow(|embed| {
                if let Some(embed) = embed {
                    embed.place(Some(rect), mtm);
                }
            });
            render_now();
        })
        .map_err(|e| IpcError::Player(format!("cannot reach the webview: {e}")))
}

/// The page's physical pixels as points (NSView coordinates are points; Harbor's
/// version skipped this and misplaced the video on Retina screens).
fn to_points(geometry: Geometry, scale: f64) -> Rect {
    Rect {
        x: geometry.x / scale,
        y: geometry.y / scale,
        width: (geometry.width / scale).max(1.0),
        height: (geometry.height / scale).max(1.0),
    }
}

/// `rect` (top-left origin) as a frame in a superview of `parent_height` points with
/// AppKit's bottom-up coordinates.
fn flipped(rect: Rect, parent_height: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.x, parent_height - rect.y - rect.height),
        NSSize::new(rect.width, rect.height),
    )
}

// --- the render host --------------------------------------------------------------------------

static NEXT_EMBED: AtomicU64 = AtomicU64::new(0);

/// One core's side of the surface: installs it on attach, removes it on detach. A
/// newer core's install takes the surface over (its view replaces this one), after
/// which this one's detach finds nothing of its own and leaves the newer one alone.
struct Host {
    window: tauri::WebviewWindow,
    id: u64,
    geometry: Arc<Mutex<Option<Rect>>>,
}

impl RenderHost for Host {
    fn attach(&self, core: RenderCore) -> Result<(), String> {
        let id = self.id;
        let rect = *self.geometry.lock().unwrap();
        let installed = on_main(&self.window, move |web_view, mtm| {
            install(id, core, web_view, rect, mtm)
        })?;
        if installed.is_ok() {
            info!("built-in video surface installed under the webview");
        }
        installed
    }

    fn detach(&self) {
        let id = self.id;
        if let Err(e) = on_main(&self.window, move |web_view, mtm| {
            uninstall(Some(id), web_view, mtm)
        }) {
            warn!("could not remove the built-in video surface: {e}");
        }
    }
}

// --- main thread ------------------------------------------------------------------------------

thread_local! {
    /// The installed surface. Only ever touched on the main thread.
    static EMBED: RefCell<Option<Embed>> = const { RefCell::new(None) };
    /// The webview layer's opacity before Zeiler made the webview transparent, while it
    /// is (restored by `restore_web_view`). Main thread only.
    static WEB_LAYER_WAS_OPAQUE: Cell<Option<bool>> = const { Cell::new(None) };
}

struct Embed {
    // dropped first (declaration order): mpv frees the render context with the view's
    // GL context, so the view must still be alive
    render: GlRenderContext,
    /// The core this surface belongs to (`Host::id`).
    id: u64,
    view: Retained<NSOpenGLView>,
}

impl Embed {
    /// Free the render context (with its GL context current, as mpv requires) and take
    /// the view out. The webview's transparency is left as it is.
    fn tear_down(self) {
        let Embed { render, view, .. } = self;
        // kept alive until the render context is gone
        let gl = view.openGLContext();
        if let Some(gl) = &gl {
            gl.makeCurrentContext();
        }
        drop(render);
        view.removeFromSuperview();
        drop(gl);
    }

    /// Frame the view at `rect`, or fill the content view when there is none yet. A
    /// rect that fills the content view keeps filling it while the window resizes;
    /// any other keeps its distance to the top until the page sends the next one.
    fn place(&self, rect: Option<Rect>, mtm: MainThreadMarker) {
        let view: &NSView = &self.view;
        // SAFETY: plain view hierarchy reads on the main thread
        let Some(parent) = (unsafe { view.superview() }) else {
            return;
        };
        let bounds = parent.bounds();
        let fills = |r: Rect| {
            r.x.abs() < 1.0
                && r.y.abs() < 1.0
                && (r.width - bounds.size.width).abs() < 1.0
                && (r.height - bounds.size.height).abs() < 1.0
        };
        match rect {
            Some(rect) if !fills(rect) => {
                view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
                view.setFrame(flipped(rect, bounds.size.height));
            }
            _ => {
                view.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                view.setFrame(bounds);
            }
        }
        if let Some(gl) = self.view.openGLContext() {
            gl.update(mtm);
        }
    }
}

// NSOpenGLPixelFormatAttribute values (NSOpenGL.h)
const NSOPENGLPFA_DOUBLEBUFFER: u32 = 5;
const NSOPENGLPFA_COLOR_SIZE: u32 = 8;
const NSOPENGLPFA_ALPHA_SIZE: u32 = 11;
const NSOPENGLPFA_DEPTH_SIZE: u32 = 12;
const NSOPENGLPFA_ACCELERATED: u32 = 73;
const NSOPENGLPFA_OPENGL_PROFILE: u32 = 99;
const NSOPENGL_PROFILE_VERSION_3_2_CORE: u32 = 0x3200;
// NSOpenGLContextParameterSurfaceOpacity
const NSOPENGL_CP_SURFACE_OPACITY: isize = 236;

fn install(
    id: u64,
    core: RenderCore,
    web_view: &NSView,
    rect: Option<Rect>,
    mtm: MainThreadMarker,
) -> Result<(), String> {
    // A surface left by the core this one replaces: this core takes the place over.
    // Freeing its render context now is the order mpv wants (that core still runs).
    // The webview stays transparent meanwhile, so nothing flashes.
    if let Some(stale) = EMBED.take() {
        stale.tear_down();
    }
    // SAFETY: AppKit calls on the main thread (`mtm`) with valid, retained objects;
    // the msg_send selectors and argument types are NSOpenGLPixelFormat's and
    // NSOpenGLView's documented initializers and NSOpenGLContext's setValues.
    unsafe {
        let parent = web_view
            .superview()
            .ok_or_else(|| "the webview is not in a window".to_string())?;
        let attributes: [u32; 12] = [
            NSOPENGLPFA_OPENGL_PROFILE,
            NSOPENGL_PROFILE_VERSION_3_2_CORE,
            NSOPENGLPFA_DOUBLEBUFFER,
            NSOPENGLPFA_ACCELERATED,
            NSOPENGLPFA_COLOR_SIZE,
            24,
            NSOPENGLPFA_ALPHA_SIZE,
            8,
            NSOPENGLPFA_DEPTH_SIZE,
            0,
            0,
            0,
        ];
        let format: Option<Retained<NSOpenGLPixelFormat>> = msg_send![
            NSOpenGLPixelFormat::alloc(),
            initWithAttributes: attributes.as_ptr()
        ];
        let format = format.ok_or_else(|| "no OpenGL 3.2 pixel format".to_string())?;
        let view: Option<Retained<NSOpenGLView>> = msg_send![
            NSOpenGLView::alloc(mtm),
            initWithFrame: parent.bounds(),
            pixelFormat: &*format
        ];
        let view = view.ok_or_else(|| "cannot create the OpenGL view".to_string())?;
        // draw at the screen's full resolution, not at 1x scaled up
        let _: () = msg_send![&*view, setWantsBestResolutionOpenGLSurface: true];
        parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(web_view));

        let Some(gl) = view.openGLContext() else {
            view.removeFromSuperview();
            return Err("the OpenGL view has no context".to_string());
        };
        gl.makeCurrentContext();
        // setValues:(const GLint *) forParameter:(NSOpenGLContextParameter, an NSInteger)
        let opaque: i32 = 1;
        let _: () = msg_send![
            &*gl,
            setValues: &opaque as *const i32,
            forParameter: NSOPENGL_CP_SURFACE_OPACITY
        ];
        let render = match core.create_gl(get_proc_address, schedule_redraw) {
            Ok(render) => render,
            Err(e) => {
                view.removeFromSuperview();
                return Err(e);
            }
        };

        make_web_view_transparent(web_view);
        let embed = Embed { render, id, view };
        embed.place(rect, mtm);
        // black until mpv's first frame
        clear_black(&embed.view);
        EMBED.set(Some(embed));
    }
    Ok(())
}

/// Remove the surface of core `only` (any surface if `None`): free its render context,
/// take the view out and let the webview draw its background again. A surface of
/// another (newer) core stays as it is. With no surface left, the webview's background
/// is restored all the same (a start that failed after making it transparent).
fn uninstall(only: Option<u64>, web_view: &NSView, _mtm: MainThreadMarker) {
    if let Some(embed) = EMBED.take() {
        if only.is_some_and(|id| id != embed.id) {
            EMBED.set(Some(embed)); // a newer core's surface
            return;
        }
        embed.tear_down();
        info!("built-in video surface removed");
    }
    restore_web_view(web_view);
}

/// Stop the webview drawing its background, so the GL view below shows through,
/// remembering how its layer was. `drawsBackground` is a private WKWebView key, the
/// same one wry uses for transparent webviews.
fn make_web_view_transparent(web_view: &NSView) {
    if WEB_LAYER_WAS_OPAQUE.get().is_none() {
        WEB_LAYER_WAS_OPAQUE.set(Some(layer_opaque(web_view).unwrap_or(true)));
    }
    set_draws_background(web_view, false);
    set_layer_opaque(web_view, false);
}

/// Let the webview draw its own background again (WebKit's default), if Zeiler made it
/// transparent.
fn restore_web_view(web_view: &NSView) {
    if let Some(was_opaque) = WEB_LAYER_WAS_OPAQUE.take() {
        set_draws_background(web_view, true);
        set_layer_opaque(web_view, was_opaque);
    }
}

fn set_draws_background(web_view: &NSView, draws: bool) {
    // SAFETY: KVC on the main thread with a key WKWebView accepts (wry relies on it)
    unsafe {
        let value = NSNumber::new_bool(draws);
        let key = NSString::from_str("drawsBackground");
        let _: () = msg_send![web_view, setValue: &*value, forKey: &*key];
        let _: () = msg_send![web_view, setWantsLayer: true];
    }
}

fn layer_opaque(view: &NSView) -> Option<bool> {
    // SAFETY: `layer` and `isOpaque` are plain CALayer reads on the main thread
    unsafe {
        let layer: *mut AnyObject = msg_send![view, layer];
        let layer = layer.as_ref()?;
        let opaque: bool = msg_send![layer, isOpaque];
        Some(opaque)
    }
}

fn set_layer_opaque(view: &NSView, opaque: bool) {
    // SAFETY: as in `layer_opaque`
    unsafe {
        let layer: *mut AnyObject = msg_send![view, layer];
        if let Some(layer) = layer.as_ref() {
            let _: () = msg_send![layer, setOpaque: opaque];
        }
    }
}

/// Draw the current frame into the installed view (main thread).
fn render_now() {
    EMBED.with_borrow(|embed| {
        let Some(embed) = embed else {
            return;
        };
        let Some(gl) = embed.view.openGLContext() else {
            return;
        };
        gl.makeCurrentContext();
        let view: &NSView = &embed.view;
        let pixels = view.convertRectToBacking(view.bounds());
        let (width, height) = (pixels.size.width as i32, pixels.size.height as i32);
        if width <= 0 || height <= 0 {
            return;
        }
        if let Err(e) = embed.render.render(0, width, height) {
            warn!("{e}");
            return;
        }
        gl.flushBuffer();
    });
}

/// Fill the view with black (before the first video frame).
fn clear_black(view: &NSOpenGLView) {
    type ClearColor = unsafe extern "C" fn(f32, f32, f32, f32);
    type Clear = unsafe extern "C" fn(u32);
    const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
    let (clear_color, clear) = (
        get_proc_address("glClearColor"),
        get_proc_address("glClear"),
    );
    if clear_color.is_null() || clear.is_null() {
        return;
    }
    let Some(gl) = view.openGLContext() else {
        return;
    };
    gl.makeCurrentContext();
    // SAFETY: the GL entry points resolved from the loaded OpenGL framework, called
    // with the view's context current
    unsafe {
        let clear_color: ClearColor = std::mem::transmute(clear_color);
        let clear: Clear = std::mem::transmute(clear);
        clear_color(0.0, 0.0, 0.0, 1.0);
        clear(GL_COLOR_BUFFER_BIT);
    }
    gl.flushBuffer();
}

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dispatch_async_f(
        queue: *mut c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
    static _dispatch_main_q: c_void;
}

const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

/// GL functions come from the OpenGL framework AppKit has loaded.
fn get_proc_address(name: &str) -> *mut c_void {
    let Ok(name) = CString::new(name) else {
        return std::ptr::null_mut();
    };
    // SAFETY: dlsym with RTLD_DEFAULT and a NUL-terminated name
    unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) }
}

/// A redraw is queued on the main queue and has not run yet.
static REDRAW_PENDING: AtomicBool = AtomicBool::new(false);

/// mpv's update callback (any thread, must not call mpv): queue one redraw.
fn schedule_redraw() {
    if REDRAW_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe extern "C" fn redraw(_context: *mut c_void) {
        REDRAW_PENDING.store(false, Ordering::Release);
        render_now();
    }
    // SAFETY: the main dispatch queue is a static object; the work function takes no
    // context
    unsafe {
        let main_queue = &raw const _dispatch_main_q as *mut c_void;
        dispatch_async_f(main_queue, std::ptr::null_mut(), redraw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page sends physical pixels; NSView frames are points, bottom-up.
    #[test]
    fn retina_pixels_become_bottom_up_points() {
        let geometry = Geometry {
            x: 200.0,
            y: 100.0,
            width: 1600.0,
            height: 900.0,
        };
        let rect = to_points(geometry, 2.0);
        assert_eq!(
            rect,
            Rect {
                x: 100.0,
                y: 50.0,
                width: 800.0,
                height: 450.0
            }
        );
        let frame = flipped(rect, 800.0);
        assert_eq!(frame.origin, NSPoint::new(100.0, 300.0));
        assert_eq!(frame.size, NSSize::new(800.0, 450.0));
        // a 1x screen keeps the numbers
        assert_eq!(to_points(geometry, 1.0).width, 1600.0);
    }
}
