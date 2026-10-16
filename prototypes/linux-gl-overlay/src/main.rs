//! M14 prototype: libmpv's OpenGL render API into a `GtkGLArea`, with a transparent
//! `WebKitWebView` stacked above it in a `GtkOverlay` (the layout Zeiler would need on
//! Linux: Tauri's webview over the video). It plays a solid green lavfi source; the
//! page paints a red box at the top left and leaves everything else transparent. A
//! screenshot then tells whether the video shows through the page.
//!
//! Prints `PROTO:` lines (GL renderer, WebKitGTK version, frames rendered) and quits
//! after `PROTO_SECONDS` (default 12).

use std::cell::RefCell;
use std::ffi::{CString, c_void};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gtk::prelude::*;
use libmpv2::Mpv;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};
use webkit2gtk::{WebViewExt, SettingsExt};

static FRAMES: AtomicU64 = AtomicU64::new(0);
static START: std::sync::LazyLock<std::time::Instant> = std::sync::LazyLock::new(std::time::Instant::now);

/// GL entry points from the loader of the context that is current: GDK's GL context
/// is GLX on X11 and EGL on Wayland. Asking EGL for a GLX context's functions (WebKit
/// has initialized EGL in this process) gives entry points that draw nothing.
fn get_proc_address(_: &(), name: &str) -> *mut c_void {
    type Loader = unsafe extern "C" fn(*const libc::c_char) -> *mut c_void;
    type Current = unsafe extern "C" fn() -> *mut c_void;
    let name = CString::new(name).unwrap();
    unsafe {
        let sym = |lib: &std::ffi::CStr, sym: &std::ffi::CStr| {
            let handle = libc::dlopen(lib.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
            if handle.is_null() { std::ptr::null_mut() } else { libc::dlsym(handle, sym.as_ptr()) }
        };
        for (lib, current, loader) in [
            (c"libGL.so.1", c"glXGetCurrentContext", c"glXGetProcAddressARB"),
            (c"libEGL.so.1", c"eglGetCurrentContext", c"eglGetProcAddress"),
        ] {
            let current = sym(lib, current);
            let loader = sym(lib, loader);
            if current.is_null() || loader.is_null() {
                continue;
            }
            if std::mem::transmute::<*mut c_void, Current>(current)().is_null() {
                continue;
            }
            if !LOADER_LOGGED.swap(true, Ordering::Relaxed) {
                println!("PROTO: GL loader {}", lib.to_string_lossy());
            }
            return std::mem::transmute::<*mut c_void, Loader>(loader)(name.as_ptr());
        }
        libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr())
    }
}

static LOADER_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn current_glx() -> usize {
    type Current = unsafe extern "C" fn() -> *mut c_void;
    unsafe {
        let h = libc::dlopen(c"libGL.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        let f = libc::dlsym(h, c"glXGetCurrentContext".as_ptr());
        if f.is_null() { 0 } else { std::mem::transmute::<*mut c_void, Current>(f)() as usize }
    }
}

fn gl_is_enabled(cap: u32) -> u8 {
    type IsEnabled = unsafe extern "C" fn(u32) -> u8;
    unsafe { std::mem::transmute::<*mut c_void, IsEnabled>(get_proc_address(&(), "glIsEnabled"))(cap) }
}

fn read_center(w: i32, h: i32) -> [u8; 4] {
    type ReadPixels = unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut u8);
    let mut px = [0u8; 4];
    unsafe {
        std::mem::transmute::<*mut c_void, ReadPixels>(get_proc_address(&(), "glReadPixels"))(w / 2, h / 2, 1, 1, 0x1908, 0x1401, px.as_mut_ptr());
    }
    px
}

fn gl_get_integer(pname: u32) -> i32 {
    type GetIntegerv = unsafe extern "C" fn(u32, *mut i32);
    let f = get_proc_address(&(), "glGetIntegerv");
    let mut v = 0;
    if !f.is_null() {
        unsafe { std::mem::transmute::<*mut c_void, GetIntegerv>(f)(pname, &mut v) };
    }
    v
}

fn gl_string(name: u32) -> String {
    type GetString = unsafe extern "C" fn(u32) -> *const libc::c_char;
    let f = get_proc_address(&(), "glGetString");
    if f.is_null() {
        return "?".into();
    }
    unsafe {
        let s = std::mem::transmute::<*mut c_void, GetString>(f)(name);
        if s.is_null() { "?".into() } else { std::ffi::CStr::from_ptr(s).to_string_lossy().into() }
    }
}

const PAGE: &str = r#"<!doctype html><html><head><style>
html, body { margin: 0; background: transparent; }
#box { position: absolute; left: 0; top: 0; width: 200px; height: 100px; background: rgb(255, 0, 0); }
#label { position: absolute; left: 220px; top: 20px; font: 24px sans-serif; color: white; }
</style></head><body><div id="box"></div><div id="label">webview above the video</div></body></html>"#;

fn main() {
    std::sync::LazyLock::force(&START);
    gtk::init().expect("GTK initializes");
    // GTK sets the user's locale; libmpv needs "C" numerics
    unsafe { libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr()) };

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(800, 450);
    window.move_(0, 0);
    let overlay = gtk::Overlay::new();
    let area = gtk::GLArea::new();
    area.set_auto_render(false);
    overlay.add(&area);

    let webview = webkit2gtk::WebView::new();
    webview.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
    if let Some(settings) = WebViewExt::settings(&webview) {
        println!("PROTO: hardware acceleration policy {:?}", settings.hardware_acceleration_policy());
    }
    webview.load_html(PAGE, None);
    // PROTO_NO_WEBVIEW: the GL area alone (does the video draw at all?)
    if std::env::var_os("PROTO_NO_WEBVIEW").is_none() {
        overlay.add_overlay(&webview);
    }
    // clicks go to the page, as in Zeiler
    if webview.parent().is_some() {
        overlay.set_overlay_pass_through(&webview, false);
    }
    window.add(&overlay);

    let mpv = Rc::new(
        Mpv::with_initializer(|init| {
            init.set_option("config", "no")?;
            init.set_option("vo", "libmpv")?;
            init.set_option("ao", "null")?;
            init.set_option("idle", "yes")?;
            init.set_option("keep-open", "yes")?;
            // PROTO_VTO0: hand frames over at their display time (as macOS does); by
            // default mpv's own offset, so render() waits for the display time instead
            if std::env::var_os("PROTO_VTO0").is_some() {
                init.set_option("video-timing-offset", "0")?;
            }
            // the prototype's diagnostics: mpv's own log on stdout
            init.set_option("terminal", "yes")?;
            init.set_option("msg-level", "all=info,vo=debug,libmpv_render=v,vd=v")?;
            Ok(())
        })
        .expect("libmpv initializes"),
    );
    let render: Rc<RefCell<Option<RenderContext<'static>>>> = Rc::new(RefCell::new(None));

    {
        let mpv = Rc::clone(&mpv);
        let render = Rc::clone(&render);
        area.connect_realize(move |area| {
            area.make_current();
            if let Some(e) = area.error() {
                println!("PROTO: GLArea error {e}");
                return;
            }
            println!("PROTO: GL {} / {}", gl_string(0x1F01), gl_string(0x1F02));
            println!("PROTO: realize context {:#x}", current_glx());
            let ctx = mpv
                .create_render_context([
                    RenderParam::ApiType(RenderParamApiType::OpenGl),
                    RenderParam::InitParams(OpenGLInitParams { get_proc_address, ctx: () }),
                ])
                .expect("render context");
            // prototype only: the Rc<Mpv> outlives the window's main loop
            let mut ctx: RenderContext<'static> = unsafe { std::mem::transmute(ctx) };
            let (tx, rx) = glib::MainContext::channel::<()>(glib::Priority::DEFAULT);
            ctx.set_update_callback(move || {
                let _ = tx.send(());
            });
            let area2 = area.clone();
            rx.attach(None, move |()| {
                area2.queue_render();
                glib::ControlFlow::Continue
            });
            *render.borrow_mut() = Some(ctx);
            println!("PROTO: loadfile at {:?}", START.elapsed());
            mpv.command("loadfile", &["av://lavfi:color=c=0x00ff00:s=640x360:r=25", "replace"])
                .expect("loadfile");
            println!("PROTO: render context created, playing");
        });
    }
    {
        let render = Rc::clone(&render);
        area.connect_render(move |area, _| {
            // PROTO_CLEAR: plain blue instead of mpv (does any GL content show?)
            if std::env::var_os("PROTO_CLEAR").is_some() {
                type ClearColor = unsafe extern "C" fn(f32, f32, f32, f32);
                type Clear = unsafe extern "C" fn(u32);
                unsafe {
                    std::mem::transmute::<*mut c_void, ClearColor>(get_proc_address(&(), "glClearColor"))(0.0, 0.0, 1.0, 1.0);
                    std::mem::transmute::<*mut c_void, Clear>(get_proc_address(&(), "glClear"))(0x4000);
                }
                FRAMES.fetch_add(1, Ordering::Relaxed);
                return glib::Propagation::Stop;
            }
            if let Some(ctx) = render.borrow().as_ref() {
                let fbo = gl_get_integer(0x8CA6); // GL_FRAMEBUFFER_BINDING
                if FRAMES.load(Ordering::Relaxed) < 3 {
                    let scale = area.scale_factor();
                    let (w, h) = (area.allocated_width() * scale, area.allocated_height() * scale);
                    let mut mask = [0u8; 4];
                    type GetBooleanv = unsafe extern "C" fn(u32, *mut u8);
                    unsafe { std::mem::transmute::<*mut c_void, GetBooleanv>(get_proc_address(&(), "glGetBooleanv"))(0x0C23, mask.as_mut_ptr()) };
                    println!("PROTO: before render context {:#x} scissor={} blend={} depth={} stencil={} colormask={mask:?} center_before={:?} viewport-fb={}",
                        current_glx(), gl_is_enabled(0x0C11), gl_is_enabled(0x0BE2), gl_is_enabled(0x0B71), gl_is_enabled(0x0B90), read_center(w, h), fbo);
                }
                let scale = area.scale_factor();
                let (w, h) = (area.allocated_width() * scale, area.allocated_height() * scale);
                if let Err(e) = ctx.render::<()>(fbo, w, h, true) {
                    println!("PROTO: render error {e:?}");
                }
                // mpv leaves framebuffer 0 bound; GTK's stays the target
                type BindFramebuffer = unsafe extern "C" fn(u32, u32);
                unsafe {
                    std::mem::transmute::<*mut c_void, BindFramebuffer>(get_proc_address(&(), "glBindFramebuffer"))(0x8D40, fbo as u32);
                }
                let n = FRAMES.fetch_add(1, Ordering::Relaxed);
                if n < 3 || n % 100 == 0 {
                    type GetError = unsafe extern "C" fn() -> u32;
                    type ReadPixels = unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut u8);
                    let err = unsafe { std::mem::transmute::<*mut c_void, GetError>(get_proc_address(&(), "glGetError"))() };
                    let mut px = [0u8; 4];
                    unsafe {
                        std::mem::transmute::<*mut c_void, ReadPixels>(get_proc_address(&(), "glReadPixels"))(w / 2, h / 2, 1, 1, 0x1908, 0x1401, px.as_mut_ptr());
                    }
                    println!("PROTO: render #{n} at {:?} fbo={fbo} {w}x{h} glError={err:#x} center={px:?} draw_fbo_after={}", START.elapsed(), gl_get_integer(0x8CA6));
                }
            }
            glib::Propagation::Stop
        });
    }

    window.connect_destroy(|_| gtk::main_quit());
    window.show_all();

    let seconds: u64 = std::env::var("PROTO_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    let render_q = Rc::clone(&render);
    let mpv_q = Rc::clone(&mpv);
    glib::timeout_add_local_once(Duration::from_secs(seconds), move || {
        println!("PROTO: frames rendered {}", FRAMES.load(Ordering::Relaxed));
        for p in ["vo-configured", "hwdec-current", "time-pos", "frame-drop-count", "decoder-frame-drop-count", "vo-delayed-frame-count", "estimated-vf-fps", "display-fps"] {
            println!("PROTO: {p} = {:?}", mpv_q.get_property::<String>(p));
        }
        // free the render context before the core (GL current)
        render_q.borrow_mut().take();
        gtk::main_quit();
    });
    gtk::main();
}
