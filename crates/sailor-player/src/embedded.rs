//! Embedded libmpv backend (feature `embedded`).
//!
//! One mpv instance lives behind an actor: callers talk to it through a command
//! channel (so no caller ever blocks on libmpv), and a dedicated thread drains
//! `mpv_wait_event`, forwarding a filtered event stream to a sink the shell
//! provides (Sailor wires it to a `player://event` Tauri event).
//!
//! The Windows window wiring (mpv child HWND pinned under a transparent
//! WebView2) is NOT here — this crate never touches the OS surface beyond
//! passing the parent `wid` to mpv pre-init. See `src-tauri/src/commands/mpv.rs`.
//!
//! The embedding approach is adapted from Harbor (github.com/9000000/harbor,
//! MIT) — see THIRD-PARTY-NOTICES.md.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use libmpv2::events::{Event, PropertyData};
use libmpv2::{Format, Mpv};
use serde::Serialize;
use serde_json::{Value, json};
use tracing::{debug, info, warn};

use crate::{PlaybackRequest, redact_url};

/// Minimum spacing between forwarded `time-pos` changes. mpv fires this every
/// frame; the UI only needs it a few times a second.
const TIME_POS_THROTTLE: Duration = Duration::from_millis(200);

/// Properties observed on behalf of the UI. Node-shaped properties (lists) are
/// observed as strings — mpv serializes them to JSON, and libmpv2 ≥ 5 has no
/// MpvNode — and re-parsed before they are forwarded.
const OBSERVED: &[(&str, Format)] = &[
    ("time-pos", Format::Double),
    ("duration", Format::Double),
    ("pause", Format::Flag),
    ("paused-for-cache", Format::Flag),
    ("cache-buffering-state", Format::Int64),
    ("eof-reached", Format::Flag),
    ("seeking", Format::Flag),
    ("volume", Format::Double),
    ("mute", Format::Flag),
    ("media-title", Format::String),
    ("track-list", Format::String),
    ("chapter-list", Format::String),
    ("aid", Format::String),
    ("sid", Format::String),
    ("vid", Format::String),
];

/// Property names whose string value is a JSON document (see [`OBSERVED`]).
const JSON_PROPS: &[&str] = &["track-list", "chapter-list"];

#[derive(Debug, thiserror::Error)]
pub enum EmbeddedError {
    #[error("embedded player failed to initialize: {0}")]
    Init(String),
    #[error("embedded player is shut down")]
    Closed,
}

/// Event stream forwarded to the UI. Serialized as `{ "type": ..., ... }`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PlayerEvent {
    /// An observed property changed. List-valued properties arrive as parsed
    /// JSON (arrays/objects), scalars as JSON scalars.
    Property {
        name: String,
        value: Value,
    },
    StartFile,
    FileLoaded,
    Seek,
    PlaybackRestart,
    /// Playback of the current file ended. `reason` is one of
    /// `eof | stop | quit | error | redirect | unknown`.
    EndFile {
        reason: &'static str,
    },
    /// A non-fatal player error (including end-of-file on error).
    Error {
        message: String,
    },
    /// The mpv core is gone; no further events will arrive.
    Shutdown,
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

enum Cmd {
    Load(PlaybackRequest),
    SetProperty(String, Value),
    Command(Vec<String>),
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
    /// `wid`: native parent window handle (HWND on Windows) that mpv embeds its
    /// video child window into. Must be set pre-init, hence a constructor
    /// argument. `events` receives the event stream; it is called from the
    /// event thread and must not block.
    pub fn spawn(
        wid: Option<i64>,
        events: impl Fn(PlayerEvent) + Send + Sync + 'static,
    ) -> Result<Self, EmbeddedError> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), EmbeddedError>>(1);

        std::thread::Builder::new()
            .name("mpv-actor".into())
            .spawn(move || actor_thread(wid, rx, init_tx, events))
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

fn actor_thread(
    wid: Option<i64>,
    rx: mpsc::Receiver<Cmd>,
    init_tx: mpsc::SyncSender<Result<(), EmbeddedError>>,
    events: impl Fn(PlayerEvent) + Send + Sync + 'static,
) {
    // libmpv requires LC_NUMERIC=C; on Windows the CRT starts in the "C"
    // locale and Rust never changes it, so there is nothing to do here.
    // (The unix backends will need a setlocale call — v0.3.)
    let mpv = match Mpv::with_initializer(|init| {
        // No config files, no scripts, no mpv-owned input: Sailor's UI is the
        // only control surface, and addon-supplied URLs must not be able to
        // pull in ytdl or user scripts.
        init.set_option("config", "no")?;
        init.set_option("load-scripts", "no")?;
        init.set_option("ytdl", "no")?;
        init.set_option("terminal", "no")?;
        init.set_option("osc", "no")?;
        init.set_option("osd-level", "0")?;
        init.set_option("input-default-bindings", "no")?;
        init.set_option("input-vo-keyboard", "no")?;
        init.set_option("input-cursor", "no")?;
        init.set_option("cursor-autohide", "no")?;
        init.set_option("title", "Sailor")?;
        init.set_option("audio-client-name", "Sailor")?;
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
        init.set_option("vo", "gpu-next")?;
        init.set_option("hwdec", "auto-safe")?;
        if let Some(wid) = wid {
            // Embed into the host window. The child window must exist before
            // the first frame so the shell can pin it below the webview.
            init.set_option("wid", wid)?;
            init.set_option("force-window", "immediate")?;
        }
        Ok(())
    }) {
        Ok(mpv) => Arc::new(mpv),
        Err(e) => {
            let _ = init_tx.send(Err(EmbeddedError::Init(e.to_string())));
            return;
        }
    };

    for (i, (name, format)) in OBSERVED.iter().enumerate() {
        if let Err(e) = mpv.observe_property(name, *format, i as u64) {
            warn!(property = name, error = %e, "mpv observe_property failed");
        }
    }

    {
        let mpv = Arc::clone(&mpv);
        if let Err(e) = std::thread::Builder::new()
            .name("mpv-events".into())
            .spawn(move || event_thread(mpv, events))
        {
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
                if let Err(e) = load(&mpv, &request) {
                    warn!(error = %e, "mpv load failed");
                    // Surfaced via the event stream too (end-file/error), but a
                    // bad loadfile argument fails synchronously only here.
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
                    warn!(property = %name, error = %e, "mpv set_property failed");
                }
            }
            Cmd::Command(args) => {
                if args.is_empty() {
                    continue;
                }
                let tail: Vec<&str> = args[1..].iter().map(String::as_str).collect();
                if let Err(e) = mpv.command(&args[0], &tail) {
                    warn!(command = %args[0], error = %e, "mpv command failed");
                }
            }
            Cmd::Quit => break,
        }
    }

    // Quit the core; the event thread sees Shutdown, forwards it, and exits.
    // Whichever thread drops the last Arc destroys the handle.
    let _ = mpv.command("quit", &[]);
    debug!("mpv actor thread exiting");
}

fn load(mpv: &Mpv, request: &PlaybackRequest) -> Result<(), libmpv2::Error> {
    info!(url = %redact_url(&request.url), "embedded player loading");

    // Per-stream HTTP headers (`behaviorHints.proxyHeaders.request`). Built via
    // `change-list append` — argv-based, so commas in values cannot corrupt the
    // list option — and cleared first so headers never leak between streams.
    mpv.command("change-list", &["http-header-fields", "clr", ""])?;
    for (name, value) in &request.headers {
        mpv.command(
            "change-list",
            &["http-header-fields", "append", &format!("{name}: {value}")],
        )?;
    }
    mpv.set_property("user-agent", request.user_agent.as_deref().unwrap_or(""))?;
    mpv.set_property("referrer", request.referrer.as_deref().unwrap_or(""))?;

    match request.start_at_sec {
        Some(start) if start > 0.0 => mpv.set_property("start", format!("{start}"))?,
        _ => mpv.set_property("start", "none")?,
    }
    mpv.set_property("force-media-title", request.title.as_deref().unwrap_or(""))?;

    mpv.command("loadfile", &[&request.url, "replace"])?;
    // `pause` persists across files (and keep-open leaves the core paused at
    // EOF) — a fresh load always starts playing.
    mpv.set_property("pause", false)
}

fn event_thread(mpv: Arc<Mpv>, events: impl Fn(PlayerEvent)) {
    let mut last_time_pos: Option<Instant> = None;
    loop {
        match mpv.wait_event(0.5) {
            None => {}
            Some(Err(e)) => {
                // Includes end-of-file on error (a failed stream), surfaced to
                // the UI as an error rather than a silent stop.
                events(PlayerEvent::Error {
                    message: e.to_string(),
                });
            }
            Some(Ok(event)) => match event {
                Event::Shutdown => {
                    events(PlayerEvent::Shutdown);
                    break;
                }
                Event::PropertyChange { name, change, .. } => {
                    if name == "time-pos" {
                        let now = Instant::now();
                        if last_time_pos
                            .is_some_and(|prev| now.duration_since(prev) < TIME_POS_THROTTLE)
                        {
                            continue;
                        }
                        last_time_pos = Some(now);
                    }
                    let value = match change {
                        PropertyData::Str(s) | PropertyData::OsdStr(s) => {
                            if JSON_PROPS.contains(&name) {
                                serde_json::from_str(s).unwrap_or(Value::String(s.to_string()))
                            } else {
                                Value::String(s.to_string())
                            }
                        }
                        PropertyData::Flag(b) => Value::Bool(b),
                        PropertyData::Int64(i) => json!(i),
                        PropertyData::Double(d) => json!(d),
                    };
                    events(PlayerEvent::Property {
                        name: name.to_string(),
                        value,
                    });
                }
                Event::StartFile => events(PlayerEvent::StartFile),
                Event::FileLoaded => events(PlayerEvent::FileLoaded),
                Event::Seek => events(PlayerEvent::Seek),
                Event::PlaybackRestart => events(PlayerEvent::PlaybackRestart),
                Event::EndFile(reason) => events(PlayerEvent::EndFile {
                    reason: end_file_reason(reason),
                }),
                _ => {}
            },
        }
    }
    debug!("mpv event thread exiting");
}
