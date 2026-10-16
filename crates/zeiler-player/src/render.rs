//! libmpv's OpenGL render API, for a host that draws the video into its own GL view
//! ([`Surface::RenderApi`](crate::embedded::Surface::RenderApi), macOS).
//!
//! The core never opens a window here (`vo=libmpv`). Instead the host creates a
//! render context for it ([`RenderCore::create_gl`]) on the thread that owns its GL
//! context, and draws a frame ([`GlRenderContext::render`]) whenever mpv says one is
//! ready. Where that happens is the host's business ([`RenderHost`]); this module
//! only owns the mpv side, and the order mpv requires:
//!
//! - the render context is created after the core is initialized and before the
//!   first file loads (a file loaded without one gets no video output);
//! - it is freed before the core quits — while the core still runs, so mpv can tear
//!   the video output down — and never after the core is destroyed. The actor calls
//!   [`RenderHost::detach`] before it sends `quit`, and every [`GlRenderContext`]
//!   also holds a reference to the core, so the core cannot be destroyed under a
//!   render context the host failed to free in time.

use std::ffi::c_void;
use std::fmt;
use std::sync::Arc;

use libmpv2::Mpv;
use libmpv2::render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType};

/// The host side of the render-API surface. Both methods are called on the player's
/// actor thread, which they may block; neither may call into the player.
pub trait RenderHost: Send + Sync {
    /// The core is initialized and nothing has loaded yet: create its render context
    /// (where the host's GL context lives) and keep it until [`detach`](Self::detach).
    /// An error fails the spawn, and the core is quit.
    fn attach(&self, core: RenderCore) -> Result<(), String>;

    /// The core is about to quit: drop the render context, and return once it is
    /// freed (or the host gave up waiting). Called once, also after a failed attach.
    fn detach(&self);
}

/// A core the host may create a render context for (see [`RenderHost::attach`]).
pub struct RenderCore {
    mpv: Arc<Mpv>,
}

impl fmt::Debug for RenderCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RenderCore")
    }
}

impl RenderCore {
    pub(crate) fn new(mpv: Arc<Mpv>) -> Self {
        Self { mpv }
    }

    /// Create an OpenGL render context. The host's GL context must be current on this
    /// thread, and every later call on the returned context must happen with it current
    /// on the same thread. `get_proc_address` resolves GL function names (it is only
    /// used during this call). `on_update` is called, from any mpv thread, whenever a
    /// new frame is ready or the video must be redrawn: it must not call into mpv, only
    /// schedule a [`GlRenderContext::render`] on the GL thread.
    pub fn create_gl(
        self,
        get_proc_address: fn(&str) -> *mut c_void,
        on_update: fn(),
    ) -> Result<GlRenderContext, String> {
        fn resolve(get: &fn(&str) -> *mut c_void, name: &str) -> *mut c_void {
            get(name)
        }
        let params = [
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address: resolve,
                ctx: get_proc_address,
            }),
        ];
        let context = self
            .mpv
            .create_render_context(params)
            .map_err(|e| format!("mpv could not create a video renderer ({e:?})"))?;
        // SAFETY: the context borrows the core; extending the borrow is sound because
        // the core lives in the same struct, behind an Arc that is dropped after the
        // context (field order), so the core outlives the context.
        let mut context: RenderContext<'static> = unsafe { std::mem::transmute(context) };
        context.set_update_callback(on_update);
        Ok(GlRenderContext {
            context: Some(context),
            _mpv: self.mpv,
        })
    }
}

/// An mpv OpenGL render context. Use and drop it only on the GL thread it was
/// created on, with that GL context current (it is not `Send`).
pub struct GlRenderContext {
    // dropped before `_mpv` (declaration order): mpv requires the render context to be
    // freed before the core is destroyed
    context: Option<RenderContext<'static>>,
    _mpv: Arc<Mpv>,
}

impl fmt::Debug for GlRenderContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GlRenderContext")
    }
}

impl GlRenderContext {
    /// Draw the current video frame into framebuffer `fbo` (0: the default
    /// framebuffer) of `width` × `height` pixels, flipped for OpenGL's bottom-up
    /// coordinates. With no new frame, the previous one is drawn again.
    pub fn render(&self, fbo: i32, width: i32, height: i32) -> Result<(), String> {
        let Some(context) = &self.context else {
            return Ok(());
        };
        context
            .render::<fn(&str) -> *mut c_void>(fbo, width, height, true)
            .map_err(|e| format!("mpv render failed ({e:?})"))
    }
}

impl Drop for GlRenderContext {
    fn drop(&mut self) {
        if let Some(mut context) = self.context.take() {
            // libmpv2 frees the update callback before the context, so mpv could still
            // call it in between. Swap in a capture-less one first (mpv serializes the
            // swap with its calls): calling it touches no freed memory.
            context.set_update_callback(|| {});
            drop(context);
        }
    }
}
