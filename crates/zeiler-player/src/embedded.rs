//! Embedded libmpv backend (feature `embedded`).
//!
//! One mpv instance lives behind an actor: callers talk to it through a command
//! channel (so no caller ever blocks on libmpv), and a dedicated thread drains
//! `mpv_wait_event`, forwarding a filtered event stream to a sink the shell
//! provides (Zeiler wires it to a `player://event` Tauri event).
//!
//! Where the video goes is the caller's [`Surface`] choice, fixed at spawn: a
//! native parent window (`wid`, Windows), the render API drawn by the host into its
//! own GL view (macOS), mpv's own window, or no output at all (tests). The OS
//! wiring (mpv child HWND pinned under a transparent WebView2, the NSOpenGLView
//! under the WKWebView) is NOT here: this crate never touches the OS surface beyond
//! passing the parent `wid` to mpv pre-init, or handing the core to the host's
//! [`RenderHost`] (see [`crate::render`]). See `src-tauri/src/commands/mpv/`.
//!
//! The event type ([`PlayerEvent`]) and the observed property set
//! ([`crate::OBSERVED`]) are shared with the external JSON-IPC backend.
//!
//! The embedding approach is adapted from Harbor (github.com/harborstremio/harbor),
//! Copyright (c) 2026 Harbor, MIT License — see THIRD-PARTY-NOTICES.md.

use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use libmpv2::events::{Event, PropertyData};
use libmpv2::{Format, Mpv};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use crate::render::{RenderCore, RenderHost};
use crate::{
    OBSERVED, PlaybackRequest, PlayerEvent, PropKind, SessionError, TIME_POS_THROTTLE,
    header_lines, mpv_error, property_kind, redact_url,
};

/// How close to the end of the throttle window a held `time-pos` may be
/// flushed. Windows mpv rounds wait timeouts down to whole milliseconds, so a
/// wait for the rest of the window can wake just short of it; without slack the
/// loop would spin on zero-length waits until the window closes.
const FLUSH_SLACK: Duration = Duration::from_millis(2);

/// libmpv read format for a property kind. Node-shaped properties (lists) are
/// read as strings — mpv serializes them to JSON, and libmpv2 ≥ 5 has no
/// MpvNode — and re-parsed before they are forwarded.
fn format_of(kind: PropKind) -> Format {
    match kind {
        PropKind::Double => Format::Double,
        PropKind::Int => Format::Int64,
        PropKind::Flag => Format::Flag,
        PropKind::Text | PropKind::Json => Format::String,
    }
}

/// Video output list for a core that shows video. The trailing comma makes mpv fall
/// back through its default VO list when `gpu-next` cannot start (RDP sessions, old
/// GPUs, VMs) instead of failing initialization.
const VO_WITH_FALLBACK: &str = "gpu-next,";

/// Where an embedded core renders its video. Fixed for the life of the core: mpv
/// takes these options before initialization only.
#[derive(Clone)]
pub enum Surface {
    /// Embed into a native parent window (an HWND on Windows): mpv creates a child
    /// window there right away (`force-window=immediate`), which the shell pins
    /// under the webview.
    Wid(i64),
    /// libmpv's render API (`vo=libmpv`): the host draws the video into its own GL
    /// view (macOS). The host creates the render context once the core is up and
    /// frees it before the core quits (see [`crate::render`]).
    RenderApi(Arc<dyn RenderHost>),
    /// mpv opens its own top-level window once a video plays.
    Detached,
    /// No video and no audio output (`vo=null`, `ao=null`): for tests, which must
    /// never open a window or an audio device.
    Headless,
}

impl std::fmt::Debug for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Surface::Wid(wid) => f.debug_tuple("Wid").field(wid).finish(),
            Surface::RenderApi(_) => f.write_str("RenderApi"),
            Surface::Detached => f.write_str("Detached"),
            Surface::Headless => f.write_str("Headless"),
        }
    }
}

/// How to create an embedded core.
#[derive(Debug, Clone)]
pub struct EmbeddedOptions {
    pub surface: Surface,
}

impl EmbeddedOptions {
    pub fn new(surface: Surface) -> Self {
        Self { surface }
    }
}

/// libmpv refuses to create a core unless `LC_NUMERIC` is `"C"` (it parses and
/// prints floats with the C library). On Unix, GTK (and so Tauri/WebKitGTK) sets
/// the user's locale at startup, so a German or French locale would make
/// `mpv_create` fail; reset just the numeric category before every core. Windows
/// needs nothing: the CRT starts in the "C" locale and Rust never changes it.
#[cfg(unix)]
fn force_c_numeric_locale() {
    // SAFETY: setlocale with a valid category and a NUL-terminated string. It is
    // not thread-safe against concurrent locale *readers* in C code; it runs once
    // per core, before mpv (the only C code here that cares) is created.
    unsafe {
        libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
    }
}

#[cfg(not(unix))]
fn force_c_numeric_locale() {}

/// `mpv-version` of the last core this process initialized (for the backends report).
static MPV_VERSION: OnceLock<String> = OnceLock::new();

/// The libmpv version string (`mpv v0.40.0 …`), once a core has been initialized.
pub fn mpv_version() -> Option<String> {
    MPV_VERSION.get().cloned()
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddedError {
    #[error("embedded player failed to initialize: {0}")]
    Init(String),
    #[error("embedded player is shut down")]
    Closed,
}

/// Readable text for a libmpv2 error. Its own `Display` is just `Debug`
/// (`Raw(-13)`), and these messages reach the UI.
fn readable(e: &libmpv2::Error) -> String {
    match e {
        libmpv2::Error::Raw(code) => format!("{} (mpv error {code})", mpv_error::describe(*code)),
        libmpv2::Error::VersionMismatch { linked, loaded } => format!(
            "incompatible libmpv: it provides client API {}.{}, Zeiler needs {}.x",
            loaded >> 16,
            loaded & 0xffff,
            linked >> 16
        ),
        libmpv2::Error::InvalidUtf8 => "mpv returned text that is not valid UTF-8".into(),
        libmpv2::Error::Null => "a value passed to mpv contains a NUL character".into(),
    }
}

/// [`readable`] for `Mpv::with_initializer` failures. The initializer only sets
/// literal options, so there `Null` means mpv_create() failed, and -4 is
/// mpv_initialize()'s catch-all — with force-window set, a video output that
/// could not be created.
fn readable_init(e: &libmpv2::Error) -> String {
    match e {
        libmpv2::Error::Null => "libmpv could not create a player instance \
                                 (out of memory, or the LC_NUMERIC locale is not \"C\")"
            .into(),
        libmpv2::Error::Raw(code @ libmpv2::mpv_error::InvalidParameter) => format!(
            "mpv failed to start, most likely because no video output could be created \
             for the window (mpv error {code})"
        ),
        other => readable(other),
    }
}

fn end_file_reason(reason: libmpv2::EndFileReason) -> &'static str {
    use libmpv2::mpv_end_file_reason as r;
    match reason {
        r::Eof => "eof",
        r::Stop => "stop",
        r::Quit => "quit",
        r::Error => "error",
        r::Redirect => "redirect",
        _ => "unknown",
    }
}

type PropertyReply = oneshot::Sender<Result<Value, String>>;

enum Cmd {
    Load(PlaybackRequest),
    SetProperty(String, Value),
    /// Fire and forget: a failure is logged.
    Command(Vec<String>),
    /// A command whose outcome the caller waits for.
    CommandChecked(Vec<String>, oneshot::Sender<Result<(), String>>),
    GetProperty(String, PropertyReply),
    Quit,
}

/// Handle to the mpv actor. Cloning is cheap; dropping the last handle shuts
/// the instance down.
pub struct EmbeddedPlayer {
    tx: mpsc::Sender<Cmd>,
}

impl EmbeddedPlayer {
    /// Create and initialize an mpv core on a dedicated actor thread.
    ///
    /// `options.surface` says where the video goes (see [`Surface`]); it is a
    /// pre-init option, hence a constructor argument. `events` receives the event
    /// stream; it is called from the event thread (and from the actor thread for a
    /// load that fails synchronously) and must not block.
    pub fn spawn(
        options: EmbeddedOptions,
        events: impl Fn(PlayerEvent) + Send + Sync + 'static,
    ) -> Result<Self, EmbeddedError> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), EmbeddedError>>(1);

        std::thread::Builder::new()
            .name("mpv-actor".into())
            .spawn(move || actor_thread(options.surface, rx, init_tx, events))
            .map_err(|e| EmbeddedError::Init(format!("failed to spawn actor thread: {e}")))?;

        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self { tx }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(EmbeddedError::Init("actor thread died during init".into())),
        }
    }

    pub fn load(&self, request: PlaybackRequest) -> Result<(), EmbeddedError> {
        self.send(Cmd::Load(request))
    }

    pub fn set_property(&self, name: String, value: Value) -> Result<(), EmbeddedError> {
        self.send(Cmd::SetProperty(name, value))
    }

    pub fn command(&self, args: Vec<String>) -> Result<(), EmbeddedError> {
        self.send(Cmd::Command(args))
    }

    /// Run a command and wait for mpv's answer (in command order), for commands whose
    /// failure the caller must report, such as `sub-add`.
    pub async fn command_checked(&self, args: Vec<String>) -> Result<(), SessionError> {
        let (tx, rx) = oneshot::channel();
        self.send(Cmd::CommandChecked(args, tx))
            .map_err(|_| SessionError::Closed)?;
        rx.await
            .map_err(|_| SessionError::Closed)?
            .map_err(SessionError::Mpv)
    }

    /// Read a property, typed per [`property_kind`]. Answered by the actor thread
    /// in command order.
    pub async fn get_property(&self, name: String) -> Result<Value, SessionError> {
        let (tx, rx) = oneshot::channel();
        self.send(Cmd::GetProperty(name, tx))
            .map_err(|_| SessionError::Closed)?;
        rx.await
            .map_err(|_| SessionError::Closed)?
            .map_err(SessionError::Mpv)
    }

    /// Ask the core to quit. The actor and event threads wind down on their own.
    pub fn quit(&self) {
        let _ = self.tx.send(Cmd::Quit);
    }

    fn send(&self, cmd: Cmd) -> Result<(), EmbeddedError> {
        self.tx.send(cmd).map_err(|_| EmbeddedError::Closed)
    }
}

impl Drop for EmbeddedPlayer {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
    }
}

/// Pre-init options for a surface: `(name, value)` pairs, in order.
fn surface_options(surface: &Surface) -> Vec<(&'static str, String)> {
    match surface {
        Surface::Wid(wid) => vec![
            ("vo", VO_WITH_FALLBACK.into()),
            // Embed into the host window. The child window must exist before the
            // first frame so the shell can pin it below the webview.
            ("wid", wid.to_string()),
            ("force-window", "immediate".into()),
        ],
        Surface::RenderApi(_) => vec![
            ("vo", "libmpv".into()),
            ("force-window", "no".into()),
            // The host draws on its UI thread: let mpv hand over each frame at its
            // display time instead of blocking that thread inside render until then.
            ("video-timing-offset", "0".into()),
        ],
        Surface::Detached => vec![("vo", VO_WITH_FALLBACK.into())],
        Surface::Headless => vec![
            ("vo", "null".into()),
            ("ao", "null".into()),
            ("force-window", "no".into()),
        ],
    }
}

fn actor_thread(
    surface: Surface,
    rx: mpsc::Receiver<Cmd>,
    init_tx: mpsc::SyncSender<Result<(), EmbeddedError>>,
    events: impl Fn(PlayerEvent) + Send + Sync + 'static,
) {
    force_c_numeric_locale();
    let mpv = match Mpv::with_initializer(|init| {
        // No config files, no scripts, no mpv-owned input: Zeiler's UI is the
        // only control surface, and addon-supplied URLs must not be able to
        // pull in ytdl or user scripts.
        init.set_option("config", "no")?;
        init.set_option("load-scripts", "no")?;
        // `ytdl` and `osc` are options of mpv's built-in Lua scripts: a libmpv built
        // without Lua (the macOS bundle) has neither the options nor the scripts
        for (name, value) in [("ytdl", "no"), ("osc", "no")] {
            match init.set_option(name, value) {
                Err(libmpv2::Error::Raw(libmpv2::mpv_error::OptionNotFound)) => {}
                other => other?,
            }
        }
        init.set_option("terminal", "no")?;
        init.set_option("osd-level", "0")?;
        init.set_option("input-default-bindings", "no")?;
        init.set_option("input-vo-keyboard", "no")?;
        init.set_option("input-cursor", "no")?;
        init.set_option("cursor-autohide", "no")?;
        init.set_option("title", "Zeiler")?;
        init.set_option("audio-client-name", "Zeiler")?;
        // Keep the core alive at end-of-file so the UI can show a replay state
        // instead of the window tearing down mid-interaction.
        init.set_option("idle", "yes")?;
        init.set_option("keep-open", "yes")?;
        // Streaming behavior: generous timeouts and a large demuxer cache —
        // torrent streams stall while pieces download.
        init.set_option("network-timeout", "600")?;
        init.set_option("cache", "yes")?;
        init.set_option("demuxer-max-bytes", "128MiB")?;
        init.set_option("demuxer-max-back-bytes", "32MiB")?;
        init.set_option(
            "stream-lavf-o",
            "reconnect=1,reconnect_streamed=1,reconnect_delay_max=10,reconnect_on_network_error=1",
        )?;
        init.set_option("hwdec", "auto-safe")?;
        for (name, value) in surface_options(&surface) {
            init.set_option(name, value.as_str())?;
        }
        Ok(())
    }) {
        Ok(mpv) => Arc::new(mpv),
        Err(e) => {
            let _ = init_tx.send(Err(EmbeddedError::Init(readable_init(&e))));
            return;
        }
    };

    // The render-API host: its render context must exist before the first load
    // and be gone before the core quits (see crate::render).
    let host = match &surface {
        Surface::RenderApi(host) => Some(Arc::clone(host)),
        _ => None,
    };
    if let Some(host) = &host
        && let Err(e) = host.attach(RenderCore::new(Arc::clone(&mpv)))
    {
        host.detach();
        let _ = mpv.command("quit", &[]);
        let _ = init_tx.send(Err(EmbeddedError::Init(e)));
        return;
    }

    if let Ok(version) = mpv.get_property::<String>("mpv-version") {
        let _ = MPV_VERSION.set(version);
    }
    // libmpv's own user agent ("libmpv"), restored for streams that bring none —
    // an empty one would go out as an empty `User-Agent:` header.
    let default_user_agent = mpv
        .get_property::<String>("user-agent")
        .unwrap_or_else(|_| "libmpv".into());
    for (i, (name, kind)) in OBSERVED.iter().enumerate() {
        if let Err(e) = mpv.observe_property(name, format_of(*kind), i as u64) {
            warn!(property = name, error = %readable(&e), "mpv observe_property failed");
        }
    }

    let events = Arc::new(events);
    {
        let core = Arc::clone(&mpv);
        let events = Arc::clone(&events);
        if let Err(e) = std::thread::Builder::new()
            .name("mpv-events".into())
            .spawn(move || event_thread(core, &*events))
        {
            if let Some(host) = &host {
                host.detach();
            }
            let _ = mpv.command("quit", &[]);
            let _ = init_tx.send(Err(EmbeddedError::Init(format!(
                "failed to spawn event thread: {e}"
            ))));
            return;
        }
    }

    let _ = init_tx.send(Ok(()));
    info!("embedded mpv initialized");

    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Load(request) => {
                if let Err(e) = load(&mpv, &request, &default_user_agent) {
                    // Stream failures arrive later on the event stream, but a bad
                    // argument (e.g. a NUL byte in an addon-supplied header)
                    // fails synchronously here — forward it, or the UI waits
                    // for a file that never starts.
                    let message = readable(&e);
                    warn!(error = %message, "mpv load failed");
                    events(PlayerEvent::Error { message });
                }
            }
            Cmd::SetProperty(name, value) => {
                let result = match &value {
                    Value::Bool(b) => mpv.set_property(&name, *b),
                    Value::Number(n) if n.is_i64() => {
                        mpv.set_property(&name, n.as_i64().unwrap_or_default())
                    }
                    Value::Number(n) => mpv.set_property(&name, n.as_f64().unwrap_or_default()),
                    Value::String(s) => mpv.set_property(&name, s.as_str()),
                    other => {
                        warn!(property = %name, "unsupported property value: {other}");
                        Ok(())
                    }
                };
                if let Err(e) = result {
                    warn!(property = %name, error = %readable(&e), "mpv set_property failed");
                }
            }
            Cmd::Command(args) => {
                if let Err(e) = run_command(&mpv, &args) {
                    let command = args.first().map_or("", String::as_str);
                    warn!(%command, error = %e, "mpv command failed");
                }
            }
            Cmd::CommandChecked(args, reply) => {
                let _ = reply.send(run_command(&mpv, &args));
            }
            Cmd::GetProperty(name, reply) => {
                let _ = reply.send(read_property(&mpv, &name).map_err(|e| readable(&e)));
            }
            Cmd::Quit => break,
        }
    }

    // The render context goes first, while the core still runs (mpv tears the video
    // output down through it). Then quit the core; the event thread sees Shutdown,
    // forwards it, and exits. Whichever thread drops the last Arc destroys the handle.
    if let Some(host) = &host {
        host.detach();
    }
    let _ = mpv.command("quit", &[]);
    debug!("mpv actor thread exiting");
}

/// Run an argv-form command; the error is readable text.
fn run_command(mpv: &Mpv, args: &[String]) -> Result<(), String> {
    let Some((name, tail)) = args.split_first() else {
        return Err("empty command".into());
    };
    let tail: Vec<&str> = tail.iter().map(String::as_str).collect();
    mpv.command(name, &tail).map_err(|e| readable(&e))
}

fn read_property(mpv: &Mpv, name: &str) -> Result<Value, libmpv2::Error> {
    Ok(match property_kind(name) {
        PropKind::Double => json!(mpv.get_property::<f64>(name)?),
        PropKind::Int => json!(mpv.get_property::<i64>(name)?),
        PropKind::Flag => Value::Bool(mpv.get_property::<bool>(name)?),
        PropKind::Text => Value::String(mpv.get_property::<String>(name)?),
        PropKind::Json => {
            let text = mpv.get_property::<String>(name)?;
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        }
    })
}

fn load(
    mpv: &Mpv,
    request: &PlaybackRequest,
    default_user_agent: &str,
) -> Result<(), libmpv2::Error> {
    info!(url = %redact_url(&request.url), "embedded player loading");

    // Per-stream HTTP headers (`behaviorHints.proxyHeaders.request`) and the
    // referrer. Built via `change-list append` — argv-based, so commas in values
    // cannot corrupt the list option — and cleared first so headers never leak
    // between streams.
    mpv.command("change-list", &["http-header-fields", "clr", ""])?;
    for line in header_lines(request) {
        mpv.command("change-list", &["http-header-fields", "append", &line])?;
    }
    mpv.set_property(
        "user-agent",
        request.user_agent.as_deref().unwrap_or(default_user_agent),
    )?;

    match request.start_at_sec {
        Some(start) if start > 0.0 => mpv.set_property("start", format!("{start}"))?,
        _ => mpv.set_property("start", "none")?,
    }
    mpv.set_property("force-media-title", request.title.as_deref().unwrap_or(""))?;
    // shell-only pre-load options (never set by the webview; see PlaybackRequest): a
    // preference mpv refuses costs that preference, never the playback
    for (name, value) in &request.options {
        if let Err(e) = mpv.set_property(name, value.as_str()) {
            warn!(option = %name, error = %readable(&e), "mpv refused a pre-load option");
        }
    }

    mpv.command("loadfile", &[&request.url, "replace"])?;
    // `pause` persists across files (and keep-open leaves the core paused at
    // EOF) — a fresh load always starts playing.
    mpv.set_property("pause", false)
}

/// The observed properties last reported with a value, so the event thread can
/// report `null` when one becomes unavailable.
///
/// mpv announces "this property is now unavailable" (e.g. `duration` and `chapter`
/// once a file ends) as a property change with `MPV_FORMAT_NONE`, which the external
/// backend forwards as `null`. libmpv2's `wait_event` turns that change into `None`,
/// indistinguishable from a timeout, so the embedded backend never sees it. Instead,
/// at file boundaries and whenever a wait returns early, it re-reads the properties
/// it last reported with a value and reports `null` for those that are gone — the
/// same event stream the external backend produces.
#[derive(Default)]
struct Available(Vec<&'static str>);

impl Available {
    /// Record a reported value of an observed property.
    fn note(&mut self, name: &str, value: &Value) {
        let Some((name, _)) = OBSERVED.iter().find(|(n, _)| *n == name) else {
            return;
        };
        if value.is_null() {
            self.0.retain(|n| n != name);
        } else if !self.0.contains(name) {
            self.0.push(name);
        }
    }

    /// Report `null` for each tracked property that `unavailable` says is gone.
    /// Returns whether `time-pos` was among them.
    fn sweep(&mut self, unavailable: impl Fn(&str) -> bool, events: &impl Fn(PlayerEvent)) -> bool {
        let mut time_pos = false;
        self.0.retain(|name| {
            if !unavailable(name) {
                return true;
            }
            time_pos |= *name == "time-pos";
            events(PlayerEvent::Property {
                name: (*name).to_string(),
                value: Value::Null,
            });
            false
        });
        time_pos
    }
}

fn unavailable(mpv: &Mpv, name: &str) -> bool {
    // Any property reads as a string, so the only failure here is availability.
    matches!(
        mpv.get_property::<String>(name),
        Err(libmpv2::Error::Raw(libmpv2::mpv_error::PropertyUnavailable))
    )
}

fn event_thread(mpv: Arc<Mpv>, events: impl Fn(PlayerEvent)) {
    let mut available = Available::default();
    let sweep = |available: &mut Available, held: &mut Option<Value>| {
        if available.sweep(|name| unavailable(&mpv, name), &events) {
            *held = None;
        }
    };
    let mut last_time_pos: Option<Instant> = None;
    // A time-pos change inside the throttle window is held, not dropped, and
    // forwarded once the window has passed: a seek while paused is followed by
    // no further updates, so dropping it would strand the UI on the old position.
    let mut held_time_pos: Option<Value> = None;
    loop {
        let timeout = match (&held_time_pos, last_time_pos) {
            (Some(_), Some(prev)) => TIME_POS_THROTTLE.saturating_sub(prev.elapsed()),
            _ => Duration::from_millis(500),
        };
        let waited = Instant::now();
        let next = mpv.wait_event(timeout.as_secs_f64());
        // A wait that ends early with no event is (barring a spurious wake-up) a
        // property that became unavailable — see `Available`.
        let woke_early = next.is_none() && waited.elapsed() + FLUSH_SLACK < timeout;
        if last_time_pos.is_some_and(|prev| prev.elapsed() + FLUSH_SLACK >= TIME_POS_THROTTLE)
            && let Some(value) = held_time_pos.take()
        {
            last_time_pos = Some(Instant::now());
            events(PlayerEvent::Property {
                name: "time-pos".into(),
                value,
            });
        }
        match next {
            None => {
                if woke_early {
                    sweep(&mut available, &mut held_time_pos);
                }
            }
            Some(Err(e)) => {
                // Includes end-of-file on error (a failed stream): libmpv2
                // reports it as an Err in place of the EndFile event. Surfaced
                // to the UI as an error rather than a silent stop.
                events(PlayerEvent::Error {
                    message: readable(&e),
                });
            }
            Some(Ok(event)) => match event {
                Event::Shutdown => {
                    events(PlayerEvent::Shutdown);
                    break;
                }
                Event::PropertyChange { name, change, .. } => {
                    let value = match change {
                        PropertyData::Str(s) | PropertyData::OsdStr(s) => {
                            if property_kind(name) == PropKind::Json {
                                serde_json::from_str(s).unwrap_or(Value::String(s.to_string()))
                            } else {
                                Value::String(s.to_string())
                            }
                        }
                        PropertyData::Flag(b) => Value::Bool(b),
                        PropertyData::Int64(i) => json!(i),
                        PropertyData::Double(d) => json!(d),
                    };
                    if name == "time-pos" {
                        if last_time_pos.is_some_and(|prev| prev.elapsed() < TIME_POS_THROTTLE) {
                            held_time_pos = Some(value);
                            continue;
                        }
                        last_time_pos = Some(Instant::now());
                        held_time_pos = None;
                    }
                    available.note(name, &value);
                    events(PlayerEvent::Property {
                        name: name.to_string(),
                        value,
                    });
                }
                Event::StartFile => {
                    held_time_pos = None; // belongs to the previous file
                    events(PlayerEvent::StartFile);
                    sweep(&mut available, &mut held_time_pos);
                }
                Event::FileLoaded => {
                    events(PlayerEvent::FileLoaded);
                    sweep(&mut available, &mut held_time_pos);
                }
                Event::Seek => events(PlayerEvent::Seek),
                Event::PlaybackRestart => events(PlayerEvent::PlaybackRestart),
                Event::EndFile(reason) => {
                    events(PlayerEvent::EndFile {
                        reason: end_file_reason(reason),
                        file_error: None,
                    });
                    sweep(&mut available, &mut held_time_pos);
                }
                _ => {}
            },
        }
    }
    debug!("mpv event thread exiting");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_errors_read_as_text_with_the_code() {
        let e = libmpv2::Error::Raw(libmpv2::mpv_error::LoadingFailed);
        assert_eq!(
            readable(&e),
            "the stream could not be opened (mpv error -13)"
        );
    }

    #[test]
    fn every_bound_error_code_has_text() {
        use libmpv2::mpv_error as code;
        let fallback = mpv_error::describe(i32::MIN);
        for c in [
            code::EventQueueFull,
            code::NoMem,
            code::Uninitialized,
            code::InvalidParameter,
            code::OptionNotFound,
            code::OptionFormat,
            code::OptionError,
            code::PropertyNotFound,
            code::PropertyFormat,
            code::PropertyUnavailable,
            code::PropertyError,
            code::Command,
            code::LoadingFailed,
            code::AoInitFailed,
            code::VoInitFailed,
            code::NothingToPlay,
            code::UnknownFormat,
            code::Unsupported,
            code::NotImplemented,
            code::Generic,
        ] {
            assert_ne!(mpv_error::describe(c), fallback, "mpv error {c}");
        }
    }

    #[test]
    fn init_failures_name_their_likely_cause() {
        assert!(readable_init(&libmpv2::Error::Null).contains("could not create a player"));
        assert!(
            readable_init(&libmpv2::Error::Raw(libmpv2::mpv_error::InvalidParameter))
                .contains("no video output could be created")
        );
        let other = libmpv2::Error::Raw(libmpv2::mpv_error::OptionError);
        assert_eq!(readable_init(&other), readable(&other));
    }

    #[test]
    fn surfaces_map_to_pre_init_options() {
        let opts = |surface: Surface| surface_options(&surface);
        let wid = opts(Surface::Wid(42));
        assert!(
            wid.contains(&("vo", "gpu-next,".into())),
            "VO falls back: {wid:?}"
        );
        assert!(wid.contains(&("wid", "42".into())));
        assert!(wid.contains(&("force-window", "immediate".into())));
        let detached = opts(Surface::Detached);
        assert_eq!(detached, vec![("vo", "gpu-next,".to_string())]);
        let headless = opts(Surface::Headless);
        assert!(headless.contains(&("vo", "null".into())));
        assert!(headless.contains(&("ao", "null".into())));
        assert!(headless.iter().all(|(name, _)| *name != "wid"));
        let render = opts(Surface::RenderApi(Arc::new(RefusingHost::default())));
        assert!(render.contains(&("vo", "libmpv".into())));
        assert!(render.contains(&("force-window", "no".into())));
        assert!(render.iter().all(|(name, _)| *name != "wid"));
    }

    /// A render host that refuses to attach, and counts its calls.
    #[derive(Default)]
    struct RefusingHost {
        calls: std::sync::Mutex<Vec<&'static str>>,
    }

    impl RenderHost for RefusingHost {
        fn attach(&self, _core: RenderCore) -> Result<(), String> {
            self.calls.lock().unwrap().push("attach");
            Err("no GL here".into())
        }
        fn detach(&self) {
            self.calls.lock().unwrap().push("detach");
        }
    }

    /// A render host that cannot attach fails the spawn with its reason, and is told
    /// to detach (the core quits without a render context left behind).
    #[test]
    fn a_render_host_that_cannot_attach_fails_the_spawn() {
        let host = Arc::new(RefusingHost::default());
        let surface = Surface::RenderApi(host.clone());
        let spawned = EmbeddedPlayer::spawn(EmbeddedOptions::new(surface), |_| {});
        assert!(
            matches!(&spawned, Err(EmbeddedError::Init(m)) if m == "no GL here"),
            "{:?}",
            spawned.as_ref().err()
        );
        assert_eq!(*host.calls.lock().unwrap(), ["attach", "detach"]);
    }

    /// A headless core (no window, nothing loaded, so no window or audio device is
    /// ever opened) answers typed property reads through the actor.
    #[tokio::test]
    async fn get_property_reads_typed_values() {
        let player = EmbeddedPlayer::spawn(EmbeddedOptions::new(Surface::Headless), |_| {})
            .expect("libmpv initializes");
        let version = player.get_property("mpv-version".into()).await.unwrap();
        assert!(version.as_str().is_some_and(|v| v.starts_with("mpv")));
        assert_eq!(mpv_version().as_deref(), version.as_str());
        let volume = player.get_property("volume".into()).await.unwrap();
        assert!(volume.is_f64(), "volume reads as a number: {volume}");
        let paused = player.get_property("pause".into()).await.unwrap();
        assert!(paused.is_boolean());
        let tracks = player.get_property("track-list".into()).await.unwrap();
        assert_eq!(tracks, json!([]), "node properties arrive parsed");
        let missing = player.get_property("no-such-property".into()).await;
        assert!(matches!(missing, Err(SessionError::Mpv(m)) if m.contains("mpv error")));
        player.quit();
        assert!(matches!(
            player.get_property("volume".into()).await,
            Err(SessionError::Closed)
        ));
    }

    fn property(name: &str, value: Value) -> PlayerEvent {
        PlayerEvent::Property {
            name: name.into(),
            value,
        }
    }

    /// libmpv2 swallows mpv's "property unavailable" changes; the sweep restores
    /// them as `null`, like the external backend, and only for properties last
    /// reported with a value.
    #[test]
    fn properties_that_become_unavailable_are_reported_as_null() {
        use std::cell::RefCell;
        let sent = RefCell::new(Vec::new());
        let emit = |e: PlayerEvent| sent.borrow_mut().push(e);
        let mut available = Available::default();
        available.note("duration", &json!(60.0));
        available.note("chapter", &json!(2));
        available.note("volume", &json!(100.0));
        available.note("time-pos", &json!(1.5));
        available.note("not-observed", &json!(1));
        available.note("sid", &Value::Null); // never had a value

        let gone = ["duration", "chapter", "time-pos", "sid", "not-observed"];
        assert!(
            available.sweep(|n| gone.contains(&n), &emit),
            "time-pos went"
        );
        assert_eq!(
            sent.take(),
            vec![
                property("duration", Value::Null),
                property("chapter", Value::Null),
                property("time-pos", Value::Null),
            ]
        );
        // Reported once: a second sweep has nothing left to report but volume.
        assert!(!available.sweep(|_| true, &emit));
        assert_eq!(sent.take(), vec![property("volume", Value::Null)]);

        // A property that comes back is tracked again; a null value untracks it.
        available.note("duration", &json!(30.0));
        available.note("duration", &Value::Null);
        assert!(!available.sweep(|_| true, &emit));
        assert!(sent.take().is_empty());
    }

    /// Against a real core: `duration` exists only while a file is loaded, and the
    /// availability probe tells it apart from properties that always exist.
    #[test]
    fn availability_probe_reads_mpv_errors() {
        let mpv = Mpv::new().expect("libmpv initializes");
        assert!(unavailable(&mpv, "duration"));
        assert!(unavailable(&mpv, "chapter"));
        assert!(!unavailable(&mpv, "volume"));
        assert!(!unavailable(&mpv, "pause"));
        assert!(!unavailable(&mpv, "track-list"));
    }

    /// End to end on a real core with no video or audio output: after `stop`, the
    /// file's `duration` is reported as `null` (libmpv2 alone would stay silent).
    #[test]
    #[ignore = "plays generated media through libmpv"]
    fn a_stopped_file_reports_its_duration_as_null() {
        let mpv = Arc::new(
            Mpv::with_initializer(|init| {
                init.set_option("config", "no")?;
                init.set_option("vo", "null")?;
                init.set_option("ao", "null")?;
                init.set_option("idle", "yes")?;
                Ok(())
            })
            .expect("libmpv initializes"),
        );
        mpv.observe_property("duration", Format::Double, 0).unwrap();
        let (tx, rx) = mpsc::channel();
        let thread = {
            let mpv = Arc::clone(&mpv);
            std::thread::spawn(move || event_thread(mpv, move |e| drop(tx.send(e))))
        };
        let next = || rx.recv_timeout(Duration::from_secs(10)).expect("an event");
        mpv.command("loadfile", &["av://lavfi:testsrc=duration=5", "replace"])
            .unwrap();
        while !matches!(next(), PlayerEvent::Property { name, value }
            if name == "duration" && value.is_number())
        {}
        mpv.command("stop", &[]).unwrap();
        // lavfi keeps refining the duration while it plays, so earlier updates may
        // still be queued; once the file has ended the next report must be null.
        while !matches!(next(), PlayerEvent::EndFile { .. }) {}
        let duration = loop {
            if let PlayerEvent::Property { name, value } = next()
                && name == "duration"
            {
                break value;
            }
        };
        assert_eq!(duration, Value::Null);
        mpv.command("quit", &[]).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn version_mismatch_names_both_versions() {
        let e = libmpv2::Error::VersionMismatch {
            linked: libmpv2::MPV_CLIENT_API_VERSION,
            loaded: 1 << 16 | 109,
        };
        assert_eq!(
            readable(&e),
            "incompatible libmpv: it provides client API 1.109, Zeiler needs 2.x"
        );
    }
}
