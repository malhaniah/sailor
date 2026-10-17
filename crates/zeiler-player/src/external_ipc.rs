//! External mpv backend over mpv's JSON IPC.
//!
//! Zeiler spawns the located `mpv` binary in its own window with
//! `--input-ipc-server=<endpoint>` and then drives it over that connection:
//! newline-delimited JSON, `{"command": [...], "request_id": n}` out, replies
//! (`{"request_id": n, "error": "success", "data": ...}`) and events
//! (`{"event": "property-change", ...}`) in. The user's mpv config, scripts and
//! key bindings stay active (it is their mpv; only `--ytdl=no` is forced, as in
//! the embedded core), but the stream URL, its headers,
//! user agent and referrer only ever travel over the private IPC connection,
//! never on argv where any local process could read them.
//!
//! Endpoints:
//! - Windows: a named pipe `\\.\pipe\zeiler-mpv-<uuid>`. Pipe names are machine
//!   wide, so the name is unguessable (v4 UUID from the OS CSPRNG).
//! - Unix: a socket in a private (0700, owned by us) directory: `$XDG_RUNTIME_DIR`
//!   when it is short enough, else `/tmp/zeiler-<uid>`, with a short random file
//!   name. macOS limits a socket path to 104 bytes (`sun_path`), which rules out
//!   anything under `~/Library/Caches`.
//!
//! Events are mapped to the same [`PlayerEvent`] shapes the embedded backend
//! emits (see [`crate::normalize`]), and `time-pos` is throttled to the same
//! 200 ms. When the mpv process exits — including when the user closes its
//! window — the session emits `Shutdown` and removes its socket.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::{
    OBSERVED, PlaybackRequest, PlayerEvent, SessionError, TIME_POS_THROTTLE, end_file_reason_name,
    header_lines, normalize, property_kind, redact_url,
};

/// Where the events go (the shell forwards them as `player://event`). Called from
/// tokio tasks; must not block.
pub type EventSink = Arc<dyn Fn(PlayerEvent) + Send + Sync>;

/// Connection attempts while mpv starts up, and the pause between them (5 s total).
const CONNECT_ATTEMPTS: u32 = 50;
const CONNECT_INTERVAL: Duration = Duration::from_millis(100);
/// How long a request may wait for mpv's reply.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// How long mpv gets to quit on its own before it is killed.
const QUIT_GRACE: Duration = Duration::from_secs(2);
/// Lines longer than this are dropped (a track list is a few KiB; this is a
/// safety valve against a runaway peer, not a real limit).
const MAX_LINE: usize = 8 * 1024 * 1024;
/// How close to the end of the throttle window a held `time-pos` may be flushed
/// (timer wake-ups can land a hair early).
const FLUSH_SLACK: Duration = Duration::from_millis(2);

#[derive(Debug, thiserror::Error)]
pub enum ExternalError {
    #[error("failed to launch mpv: {0}")]
    Spawn(std::io::Error),
    /// The IPC connection never came up (no endpoint, mpv exited, or it never
    /// listened). The caller may fall back to the plain stdin launch.
    #[error("mpv's control connection did not come up: {0}")]
    IpcUnavailable(String),
    /// Connected, but mpv refused the initial setup or the file.
    #[error("{0}")]
    Load(String),
}

/// mpv arguments for an IPC session. Only fixed, non-secret values: no URL,
/// headers, user agent or referrer (those go over the connection).
pub fn ipc_args(endpoint: &str) -> Vec<String> {
    vec![
        format!("--input-ipc-server={endpoint}"),
        "--idle=yes".into(),
        "--keep-open=yes".into(),
        "--force-window=immediate".into(),
        "--title=Zeiler".into(),
        // Addon-supplied URLs must not reach yt-dlp: on a failed load its hook would
        // put the stream URL (and any token in it) on yt-dlp's command line. Same
        // policy as the embedded core; everything else in the user's config stays.
        "--ytdl=no".into(),
        // mpv's terminal output shares Zeiler's stdout, and it names the stream
        // ("Playing: <url>", "Failed to open <url>"), whose URL can carry an account
        // (IPTV streams): Zeiler follows mpv over IPC, not its terminal
        "--really-quiet".into(),
    ]
}

/// A running external mpv, remote-controlled over JSON IPC. Dropping it quits mpv.
pub struct ExternalIpcPlayer {
    conn: Arc<Connection>,
    /// Fires (or is dropped) to ask the child watcher to make sure mpv exits.
    quit_tx: Mutex<Option<oneshot::Sender<()>>>,
    /// The user's own values (from their mpv config), restored when a request
    /// does not set its own.
    defaults: Defaults,
}

#[derive(Debug, Default)]
struct Defaults {
    user_agent: String,
    /// The `referrer` option from the user's config ("" when unset).
    referrer: String,
    headers: Vec<String>,
}

impl ExternalIpcPlayer {
    /// Launch `binary`, connect, observe the shared property set and load `request`.
    pub async fn start(
        binary: &Path,
        request: &PlaybackRequest,
        events: EventSink,
    ) -> Result<Self, ExternalError> {
        Self::start_with_args(binary, &[], request, events).await
    }

    /// [`start`](Self::start) with extra mpv arguments after Zeiler's own (tests run
    /// mpv headless with `--vo=null --ao=null --no-config`). Never pass anything
    /// secret here: argv is visible to every local process.
    pub async fn start_with_args(
        binary: &Path,
        extra_args: &[String],
        request: &PlaybackRequest,
        events: EventSink,
    ) -> Result<Self, ExternalError> {
        let endpoint = Endpoint::create()
            .map_err(|e| ExternalError::IpcUnavailable(format!("no IPC endpoint: {e}")))?;
        info!(player = %binary.display(), url = %redact_url(&request.url), "launching external mpv (JSON IPC)");
        let mut child = Command::new(binary)
            .args(ipc_args(endpoint.name()))
            .args(extra_args)
            .stdin(Stdio::null())
            .kill_on_drop(false)
            .spawn()
            .map_err(ExternalError::Spawn)?;

        let stream = match endpoint.connect(&mut child).await {
            Ok(stream) => stream,
            Err(reason) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                endpoint.remove();
                return Err(ExternalError::IpcUnavailable(reason));
            }
        };
        debug!("connected to mpv's JSON IPC");

        let (read_half, write_half) = tokio::io::split(stream);
        let (line_tx, line_rx) = mpsc::unbounded_channel::<String>();
        let conn = Arc::new(Connection {
            writer: line_tx,
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            closed: AtomicBool::new(false),
        });
        let shutdown_sent = Arc::new(AtomicBool::new(false));
        tokio::spawn(write_loop(write_half, line_rx));
        let reader = tokio::spawn(read_loop(
            read_half,
            Arc::clone(&conn),
            Arc::clone(&events),
            Arc::clone(&shutdown_sent),
        ));
        let (quit_tx, quit_rx) = oneshot::channel();
        tokio::spawn(watch_child(
            child,
            quit_rx,
            reader,
            events,
            shutdown_sent,
            endpoint,
        ));

        for (i, (name, _)) in OBSERVED.iter().enumerate() {
            conn.notify(vec![json!("observe_property"), json!(i), json!(name)]);
        }
        let defaults = Defaults {
            user_agent: conn.get_text("user-agent").await,
            referrer: conn.get_text("referrer").await,
            headers: conn
                .request(vec![json!("get_property"), json!("http-header-fields")])
                .await
                .ok()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
        };
        let player = Self {
            conn,
            quit_tx: Mutex::new(Some(quit_tx)),
            defaults,
        };
        if let Err(e) = player.load(request).await {
            player.quit();
            return Err(ExternalError::Load(e.to_string()));
        }
        Ok(player)
    }

    /// Load (replace) the current file: headers, user agent, referrer, start
    /// offset and title first, then `loadfile replace`, then unpause.
    pub async fn load(&self, request: &PlaybackRequest) -> Result<(), SessionError> {
        info!(url = %redact_url(&request.url), "external mpv loading");
        for LoadStep { command, required } in load_commands(request, &self.defaults) {
            match self.conn.request(command).await {
                Err(SessionError::Mpv(error)) if !required => {
                    warn!(%error, "mpv refused a pre-load option");
                }
                result => {
                    result?;
                }
            }
        }
        Ok(())
    }

    pub async fn set_property(&self, name: &str, value: Value) -> Result<(), SessionError> {
        self.conn
            .request(vec![json!("set_property"), json!(name), value])
            .await
            .map(drop)
    }

    pub async fn command(&self, args: Vec<String>) -> Result<(), SessionError> {
        if args.is_empty() {
            return Err(SessionError::Mpv("empty command".into()));
        }
        self.conn
            .request(args.into_iter().map(Value::String).collect())
            .await
            .map(drop)
    }

    /// Read a property, in the same JSON shape the embedded backend reports.
    pub async fn get_property(&self, name: &str) -> Result<Value, SessionError> {
        let value = self
            .conn
            .request(vec![json!("get_property"), json!(name)])
            .await?;
        Ok(normalize(property_kind(name), value))
    }

    /// Ask mpv to quit; it is killed if it has not exited within a short grace
    /// period. Never blocks. A `Shutdown` event follows once the process is gone.
    pub fn quit(&self) {
        self.conn.notify(vec![json!("quit")]);
        if let Some(tx) = self.quit_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ExternalIpcPlayer {
    fn drop(&mut self) {
        self.quit();
    }
}

/// The request's own referrer: its `referrer`, else a `Referer` among its headers.
fn stream_referrer(request: &PlaybackRequest) -> Option<&str> {
    request
        .referrer
        .as_deref()
        .filter(|r| !r.is_empty())
        .or_else(|| {
            request
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("referer"))
                .map(|(_, value)| value.as_str())
        })
}

/// One command of a load, and whether the load fails when mpv refuses it.
#[derive(Debug, PartialEq)]
struct LoadStep {
    command: Vec<Value>,
    /// False for the shell's pre-load options (preferences): an older mpv may not
    /// know one, and that must not cost the playback.
    required: bool,
}

/// The commands that load `request`, in order.
fn load_commands(request: &PlaybackRequest, defaults: &Defaults) -> Vec<LoadStep> {
    let mut commands = vec![vec![
        json!("change-list"),
        json!("http-header-fields"),
        json!("clr"),
        json!(""),
    ]];
    // mpv sends its `referrer` option as a `Referer` header ahead of every
    // http-header-fields line. With no referrer in the user's config the option stays
    // unset and the stream's referrer is one of the lines (see `header_lines`). When
    // the user's config sets one, a second Referer line would follow theirs, so the
    // stream's referrer replaces the option for this load instead (and the user's
    // value comes back with the next load that has none).
    let user_referrer = !defaults.referrer.is_empty();
    let lines = if user_referrer {
        request
            .headers
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("referer"))
            .map(|(name, value)| format!("{name}: {value}"))
            .collect()
    } else {
        header_lines(request)
    };
    // change-list append (not set): commas in values cannot split the list
    for header in defaults.headers.iter().cloned().chain(lines) {
        commands.push(vec![
            json!("change-list"),
            json!("http-header-fields"),
            json!("append"),
            json!(header),
        ]);
    }
    let set = |name: &str, value: Value| vec![json!("set_property"), json!(name), value];
    commands.push(set(
        "user-agent",
        json!(
            request
                .user_agent
                .as_deref()
                .unwrap_or(&defaults.user_agent)
        ),
    ));
    if user_referrer {
        commands.push(set(
            "referrer",
            json!(stream_referrer(request).unwrap_or(&defaults.referrer)),
        ));
    }
    // Zeiler's library is the one source of resume positions: a user's
    // save-position-on-quit would otherwise make mpv's own watch-later start (applied
    // at load, over a runtime `start`) win over the picker's "Resume" or "Start from
    // the beginning". Optional, like the preferences: an odd build cannot fail a load.
    let resume = commands.len();
    commands.push(set("resume-playback", json!("no")));
    // `start` rather than loadfile's per-file options: their syntax changed across
    // mpv versions (the playlist index argument), the property did not
    commands.push(set(
        "start",
        match request.start_at_sec {
            Some(start) if start > 0.0 => json!(format!("{start}")),
            _ => json!("none"),
        },
    ));
    commands.push(set(
        "force-media-title",
        json!(request.title.as_deref().unwrap_or("")),
    ));
    let options = commands.len()..commands.len() + request.options.len();
    for (name, value) in &request.options {
        commands.push(set(name, json!(value)));
    }
    commands.push(vec![
        json!("loadfile"),
        json!(request.url),
        json!("replace"),
    ]);
    // `pause` persists across files (keep-open leaves mpv paused at EOF)
    commands.push(set("pause", json!(false)));
    commands
        .into_iter()
        .enumerate()
        .map(|(i, command)| LoadStep {
            command,
            required: !options.contains(&i) && i != resume,
        })
        .collect()
}

// --- connection ------------------------------------------------------------------------

struct Connection {
    writer: mpsc::UnboundedSender<String>,
    pending: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    /// Set once the reader has seen the connection end.
    closed: AtomicBool,
}

enum Pending {
    /// Someone awaits the reply.
    Reply(oneshot::Sender<Result<Value, String>>),
    /// Fire-and-forget: log a failure under this command name.
    Log(String),
}

impl Connection {
    fn send(&self, args: Vec<Value>, pending: Pending) -> Result<u64, SessionError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(SessionError::Closed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.pending.lock().unwrap().insert(id, pending);
        if self.writer.send(encode_request(&args, id)).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(SessionError::Closed);
        }
        Ok(id)
    }

    async fn request(&self, args: Vec<Value>) -> Result<Value, SessionError> {
        let (tx, rx) = oneshot::channel();
        let id = self.send(args, Pending::Reply(tx))?;
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(result)) => result.map_err(|e| SessionError::Mpv(format!("mpv: {e}"))),
            Ok(Err(_)) => Err(SessionError::Closed),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(SessionError::Mpv("mpv did not answer in time".into()))
            }
        }
    }

    fn notify(&self, args: Vec<Value>) {
        let name = args
            .first()
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let _ = self.send(args, Pending::Log(name));
    }

    /// A string property, or "" when it cannot be read.
    async fn get_text(&self, name: &str) -> String {
        match self.request(vec![json!("get_property"), json!(name)]).await {
            Ok(Value::String(s)) => s,
            _ => String::new(),
        }
    }

    fn resolve(&self, id: u64, result: Result<Value, String>) {
        let Some(pending) = self.pending.lock().unwrap().remove(&id) else {
            return;
        };
        match (pending, result) {
            (Pending::Reply(tx), result) => {
                let _ = tx.send(result);
            }
            (Pending::Log(command), Err(error)) => {
                warn!(%command, %error, "mpv command failed");
            }
            (Pending::Log(_), Ok(_)) => {}
        }
    }

    /// The connection is gone: every waiter gets `Closed`.
    fn fail_all(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.pending.lock().unwrap().clear();
    }
}

/// One request line. serde_json escapes control characters inside strings, so no
/// value (an addon-supplied header, say) can break the line framing.
fn encode_request(args: &[Value], id: u64) -> String {
    let mut line = json!({ "command": args, "request_id": id }).to_string();
    line.push('\n');
    line
}

async fn write_loop<W: AsyncWrite + Unpin>(
    mut writer: W,
    mut lines: mpsc::UnboundedReceiver<String>,
) {
    while let Some(line) = lines.recv().await {
        if let Err(e) = writer.write_all(line.as_bytes()).await {
            debug!("mpv IPC write failed: {e}");
            break;
        }
    }
}

async fn read_loop<R: AsyncRead + Unpin>(
    mut reader: R,
    conn: Arc<Connection>,
    events: EventSink,
    shutdown_sent: Arc<AtomicBool>,
) {
    let mut framer = LineFramer::default();
    let mut throttle = TimePosThrottle::default();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let flush_at = throttle.due();
        let read = tokio::select! {
            read = reader.read(&mut buf) => Some(read),
            () = sleep_until(flush_at) => None,
        };
        let n = match read {
            None => {
                if let Some(value) = throttle.flush(Instant::now()) {
                    events(time_pos(value));
                }
                continue;
            }
            Some(Ok(0)) => break,
            Some(Ok(n)) => n,
            Some(Err(e)) => {
                debug!("mpv IPC read failed: {e}");
                break;
            }
        };
        for line in framer.push(&buf[..n]) {
            match parse_line(&line) {
                Incoming::Reply { id, result } => conn.resolve(id, result),
                Incoming::Events(list) => {
                    for event in list {
                        match &event {
                            PlayerEvent::Property { name, value } if name == "time-pos" => {
                                if let Some(value) = throttle.offer(Instant::now(), value.clone()) {
                                    events(time_pos(value));
                                }
                                continue;
                            }
                            // a held position belongs to the previous file
                            PlayerEvent::StartFile => throttle.reset(),
                            PlayerEvent::Shutdown if shutdown_sent.swap(true, Ordering::SeqCst) => {
                                continue;
                            }
                            _ => {}
                        }
                        events(event);
                    }
                }
                Incoming::Ignored => {}
            }
        }
    }
    conn.fail_all();
}

async fn sleep_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending().await,
    }
}

fn time_pos(value: Value) -> PlayerEvent {
    PlayerEvent::Property {
        name: "time-pos".into(),
        value,
    }
}

/// Waits for mpv to exit (on its own, e.g. the user closed the window, or after a
/// quit request, killing it if it lingers), then removes the socket and emits the
/// session's single `Shutdown` — after the reader has drained, so it is the last event.
async fn watch_child(
    mut child: Child,
    mut quit_rx: oneshot::Receiver<()>,
    reader: JoinHandle<()>,
    events: EventSink,
    shutdown_sent: Arc<AtomicBool>,
    endpoint: Endpoint,
) {
    let status = tokio::select! {
        status = child.wait() => status,
        // a quit request, or the player handle was dropped
        _ = &mut quit_rx => match tokio::time::timeout(QUIT_GRACE, child.wait()).await {
            Ok(status) => status,
            Err(_) => {
                warn!("mpv did not quit in time; killing it");
                let _ = child.start_kill();
                child.wait().await
            }
        },
    };
    debug!(?status, "external mpv exited");
    let _ = tokio::time::timeout(Duration::from_secs(1), reader).await;
    endpoint.remove();
    if !shutdown_sent.swap(true, Ordering::SeqCst) {
        events(PlayerEvent::Shutdown);
    }
}

// --- framing and event mapping (pure; unit-tested on every OS) -----------------------------

/// Splits a byte stream into newline-terminated lines. Tolerates CRLF, blank lines,
/// chunks that end mid-line or mid-UTF-8 sequence, and drops (rather than buffers
/// forever) a line longer than [`MAX_LINE`].
#[derive(Debug, Default)]
struct LineFramer {
    buf: Vec<u8>,
    /// Inside an over-long line: skip to its end.
    discarding: bool,
}

impl LineFramer {
    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        let mut rest = chunk;
        while let Some(pos) = rest.iter().position(|&b| b == b'\n') {
            let (head, tail) = rest.split_at(pos);
            rest = &tail[1..];
            if self.discarding {
                self.discarding = false;
                self.buf.clear();
                continue;
            }
            self.buf.extend_from_slice(head);
            let line = std::mem::take(&mut self.buf);
            let line = line.strip_suffix(b"\r").unwrap_or(&line);
            if !line.iter().all(u8::is_ascii_whitespace) {
                lines.push(String::from_utf8_lossy(line).into_owned());
            }
        }
        if !self.discarding {
            self.buf.extend_from_slice(rest);
            if self.buf.len() > MAX_LINE {
                warn!("dropping an over-long line from mpv");
                self.buf.clear();
                self.discarding = true;
            }
        }
        lines
    }
}

/// What one line from mpv means.
#[derive(Debug, PartialEq)]
enum Incoming {
    Reply {
        id: u64,
        result: Result<Value, String>,
    },
    Events(Vec<PlayerEvent>),
    Ignored,
}

fn parse_line(line: &str) -> Incoming {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(line) else {
        debug!("ignoring a non-JSON line from mpv");
        return Incoming::Ignored;
    };
    if let Some(event) = object.get("event").and_then(Value::as_str) {
        return Incoming::Events(map_event(event, &object));
    }
    match object.get("request_id").and_then(Value::as_u64) {
        Some(id) => {
            let result = match object.get("error").and_then(Value::as_str) {
                Some("success") => Ok(object.get("data").cloned().unwrap_or(Value::Null)),
                Some(error) => Err(error.to_string()),
                None => Err("malformed reply".to_string()),
            };
            Incoming::Reply { id, result }
        }
        None => Incoming::Ignored,
    }
}

fn map_event(event: &str, object: &serde_json::Map<String, Value>) -> Vec<PlayerEvent> {
    match event {
        "property-change" => {
            let Some(name) = object.get("name").and_then(Value::as_str) else {
                return Vec::new();
            };
            let value = object.get("data").cloned().unwrap_or(Value::Null);
            vec![PlayerEvent::Property {
                name: name.to_string(),
                value: normalize(property_kind(name), value),
            }]
        }
        "start-file" => vec![PlayerEvent::StartFile],
        "file-loaded" => vec![PlayerEvent::FileLoaded],
        "seek" => vec![PlayerEvent::Seek],
        "playback-restart" => vec![PlayerEvent::PlaybackRestart],
        "shutdown" => vec![PlayerEvent::Shutdown],
        "end-file" => {
            let reason =
                end_file_reason_name(object.get("reason").and_then(Value::as_str).unwrap_or(""));
            let file_error = object
                .get("file_error")
                .and_then(Value::as_str)
                .map(str::to_string);
            let mut out = Vec::new();
            if reason == "error" {
                // the specific text first, so it wins over the generic end-file error
                out.push(PlayerEvent::Error {
                    message: match &file_error {
                        Some(e) => format!("the stream could not be played ({e})"),
                        None => "the stream could not be played".into(),
                    },
                });
            }
            out.push(PlayerEvent::EndFile { reason, file_error });
            out
        }
        "client-message" => {
            let args = object
                .get("args")
                .and_then(Value::as_array)
                .map(|args| {
                    args.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            vec![PlayerEvent::ClientMessage { args }]
        }
        _ => Vec::new(),
    }
}

/// Forwards at most one `time-pos` per [`TIME_POS_THROTTLE`]. A change inside the
/// window is held, not dropped, and flushed once the window has passed: a seek while
/// paused is followed by no further updates, so dropping it would strand the UI on
/// the old position. (Same policy as the embedded event thread.)
#[derive(Debug, Default)]
struct TimePosThrottle {
    last: Option<Instant>,
    held: Option<Value>,
}

impl TimePosThrottle {
    /// A new position: returns it when it may be forwarded now.
    fn offer(&mut self, now: Instant, value: Value) -> Option<Value> {
        if self
            .last
            .is_some_and(|prev| now.duration_since(prev) < TIME_POS_THROTTLE)
        {
            self.held = Some(value);
            return None;
        }
        self.last = Some(now);
        self.held = None;
        Some(value)
    }

    /// When the held position becomes due, if one is held.
    fn due(&self) -> Option<Instant> {
        match (&self.held, self.last) {
            (Some(_), Some(prev)) => Some(prev + TIME_POS_THROTTLE),
            _ => None,
        }
    }

    /// The held position, once its window has passed.
    fn flush(&mut self, now: Instant) -> Option<Value> {
        let due = self
            .last
            .is_some_and(|prev| now.duration_since(prev) + FLUSH_SLACK >= TIME_POS_THROTTLE);
        if !due {
            return None;
        }
        let value = self.held.take()?;
        self.last = Some(now);
        Some(value)
    }

    fn reset(&mut self) {
        self.held = None;
    }
}

// --- endpoints -------------------------------------------------------------------------

#[cfg(unix)]
type IpcStream = tokio::net::UnixStream;
#[cfg(windows)]
type IpcStream = tokio::net::windows::named_pipe::NamedPipeClient;

/// The IPC endpoint of one mpv process.
struct Endpoint {
    name: String,
    #[cfg(unix)]
    path: std::path::PathBuf,
}

impl Endpoint {
    fn name(&self) -> &str {
        &self.name
    }

    /// Wait for mpv to listen. Gives up when mpv exits first or after
    /// [`CONNECT_ATTEMPTS`].
    async fn connect(&self, child: &mut Child) -> Result<IpcStream, String> {
        let mut last_error = String::from("mpv never listened");
        for _ in 0..CONNECT_ATTEMPTS {
            match self.try_connect().await {
                Ok(Some(stream)) => return Ok(stream),
                Ok(None) => {}
                Err(e) => last_error = e.to_string(),
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "mpv exited before it could be controlled ({status})"
                ));
            }
            tokio::time::sleep(CONNECT_INTERVAL).await;
        }
        Err(last_error)
    }
}

#[cfg(unix)]
impl Endpoint {
    fn create() -> std::io::Result<Self> {
        let dir = unix::socket_dir()?;
        let path = dir.join(format!("zeiler-mpv-{}.sock", short_random()));
        unix::check_socket_path(&path)?;
        let name = path
            .to_str()
            .ok_or_else(|| std::io::Error::other("the socket path is not UTF-8"))?
            .to_string();
        Ok(Self { name, path })
    }

    /// `Ok(None)`: not listening yet, retry.
    async fn try_connect(&self) -> std::io::Result<Option<IpcStream>> {
        match tokio::net::UnixStream::connect(&self.path).await {
            Ok(stream) => Ok(Some(stream)),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    fn remove(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(windows)]
impl Endpoint {
    fn create() -> std::io::Result<Self> {
        Ok(Self {
            name: format!(r"\\.\pipe\zeiler-mpv-{}", uuid::Uuid::new_v4().simple()),
        })
    }

    /// `Ok(None)`: the pipe does not exist yet, or every instance is busy; retry.
    async fn try_connect(&self) -> std::io::Result<Option<IpcStream>> {
        /// winerror.h ERROR_PIPE_BUSY
        const ERROR_PIPE_BUSY: i32 = 231;
        match tokio::net::windows::named_pipe::ClientOptions::new().open(&self.name) {
            Ok(client) => Ok(Some(client)),
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(ERROR_PIPE_BUSY) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Named pipes vanish with their server; nothing to clean up.
    fn remove(&self) {}
}

/// Eight random hex characters (from a v4 UUID, i.e. the OS CSPRNG): the socket
/// already sits in a private directory, the name only has to be unique and short.
#[cfg(unix)]
fn short_random() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

#[cfg(unix)]
mod unix {
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    /// `sun_path` is 104 bytes on macOS/BSD and 108 on Linux, including the NUL.
    const SUN_PATH_MAX: usize = 104;
    /// Prefer `$XDG_RUNTIME_DIR` only when it leaves room for the file name.
    const MAX_DIR_LEN: usize = 64;

    fn uid() -> u32 {
        // SAFETY: getuid() has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    /// A directory only we can enter: `$XDG_RUNTIME_DIR` (0700 by the XDG spec, and
    /// checked anyway) when short enough, else `/tmp/zeiler-<uid>`, created 0700. An
    /// existing `/tmp/zeiler-<uid>` that is a symlink, someone else's, or open to
    /// others is refused rather than used.
    pub(super) fn socket_dir() -> io::Result<PathBuf> {
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
            && dir.is_absolute()
            && dir.as_os_str().len() <= MAX_DIR_LEN
            && is_private_dir(&dir)
        {
            return Ok(dir);
        }
        let dir = PathBuf::from(format!("/tmp/zeiler-{}", uid()));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        if is_private_dir(&dir) {
            Ok(dir)
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} is not a private directory", dir.display()),
            ))
        }
    }

    fn is_private_dir(dir: &Path) -> bool {
        std::fs::symlink_metadata(dir).is_ok_and(|meta| {
            meta.is_dir() && meta.uid() == uid() && meta.permissions().mode() & 0o077 == 0
        })
    }

    pub(super) fn check_socket_path(path: &Path) -> io::Result<()> {
        if path.as_os_str().len() >= SUN_PATH_MAX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("socket path too long: {}", path.display()),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_socket_dir_is_private_and_short() {
            let dir = socket_dir().unwrap();
            assert!(is_private_dir(&dir));
            let path = dir.join(format!("zeiler-mpv-{}.sock", super::super::short_random()));
            assert!(path.as_os_str().len() < SUN_PATH_MAX, "{}", path.display());
            check_socket_path(&path).unwrap();
        }

        #[test]
        fn long_paths_are_refused() {
            let long = PathBuf::from(format!("/tmp/{}/x.sock", "a".repeat(120)));
            assert!(check_socket_path(&long).is_err());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(line: &str) -> Vec<PlayerEvent> {
        match parse_line(line) {
            Incoming::Events(events) => events,
            other => panic!("expected events, got {other:?}"),
        }
    }

    fn property(name: &str, value: Value) -> PlayerEvent {
        PlayerEvent::Property {
            name: name.into(),
            value,
        }
    }

    #[test]
    fn framer_splits_lines_across_chunks() {
        let mut framer = LineFramer::default();
        assert!(framer.push(b"{\"event\":\"se").is_empty());
        assert_eq!(
            framer.push(b"ek\"}\n{\"a\":1}\r\n\n  \n{\"b\""),
            vec!["{\"event\":\"seek\"}".to_string(), "{\"a\":1}".to_string()]
        );
        assert_eq!(framer.push(b":2}\n"), vec!["{\"b\":2}".to_string()]);
    }

    #[test]
    fn framer_keeps_utf8_split_mid_character() {
        let mut framer = LineFramer::default();
        let line = "{\"t\":\"Amélie\"}\n".as_bytes();
        let split = line.iter().position(|&b| b == 0xC3).unwrap() + 1;
        assert!(framer.push(&line[..split]).is_empty());
        assert_eq!(framer.push(&line[split..]), vec!["{\"t\":\"Amélie\"}"]);
    }

    #[test]
    fn framer_drops_over_long_lines_and_recovers() {
        let mut framer = LineFramer::default();
        let big = vec![b'x'; MAX_LINE + 1];
        assert!(framer.push(&big).is_empty());
        assert!(framer.push(b"still the same line").is_empty());
        assert_eq!(framer.push(b"\n{\"ok\":1}\n"), vec!["{\"ok\":1}"]);
    }

    #[test]
    fn requests_are_one_line_even_with_hostile_values() {
        let line = encode_request(
            &[
                json!("change-list"),
                json!("http-header-fields"),
                json!("append"),
                json!("X-Evil: a\r\n{\"command\":[\"quit\"]}"),
            ],
            7,
        );
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let back: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(back["request_id"], 7);
        assert_eq!(back["command"][3], "X-Evil: a\r\n{\"command\":[\"quit\"]}");
    }

    #[test]
    fn replies_resolve_by_request_id() {
        assert_eq!(
            parse_line(r#"{"data":"mpv 0.37.0","request_id":3,"error":"success"}"#),
            Incoming::Reply {
                id: 3,
                result: Ok(json!("mpv 0.37.0"))
            }
        );
        assert_eq!(
            parse_line(r#"{"request_id":4,"error":"success"}"#),
            Incoming::Reply {
                id: 4,
                result: Ok(Value::Null)
            }
        );
        assert_eq!(
            parse_line(r#"{"request_id":5,"error":"property unavailable"}"#),
            Incoming::Reply {
                id: 5,
                result: Err("property unavailable".into())
            }
        );
        assert_eq!(parse_line("not json"), Incoming::Ignored);
        assert_eq!(parse_line("[1,2]"), Incoming::Ignored);
        assert_eq!(parse_line(r#"{"error":"success"}"#), Incoming::Ignored);
    }

    #[test]
    fn property_changes_take_the_embedded_shapes() {
        assert_eq!(
            events(r#"{"event":"property-change","id":0,"name":"time-pos","data":12.5}"#),
            vec![property("time-pos", json!(12.5))]
        );
        assert_eq!(
            events(r#"{"event":"property-change","id":12,"name":"aid","data":1}"#),
            vec![property("aid", json!("1"))]
        );
        assert_eq!(
            events(r#"{"event":"property-change","id":13,"name":"sid","data":false}"#),
            vec![property("sid", json!("no"))]
        );
        assert_eq!(
            events(r#"{"event":"property-change","id":1,"name":"duration"}"#),
            vec![property("duration", Value::Null)],
            "no data = unavailable"
        );
        let tracks = events(
            r#"{"event":"property-change","id":10,"name":"track-list","data":[{"id":1,"type":"video","codec":"h264","selected":true},{"id":1,"type":"audio","lang":"eng"}]}"#,
        );
        let [PlayerEvent::Property { name, value }] = tracks.as_slice() else {
            panic!("{tracks:?}");
        };
        assert_eq!(name, "track-list");
        assert_eq!(value[1]["lang"], "eng", "real JSON, not a string");
    }

    #[test]
    fn lifecycle_events_map_one_to_one() {
        assert_eq!(
            events(r#"{"event":"start-file","playlist_entry_id":1}"#),
            vec![PlayerEvent::StartFile]
        );
        assert_eq!(
            events(r#"{"event":"file-loaded"}"#),
            vec![PlayerEvent::FileLoaded]
        );
        assert_eq!(events(r#"{"event":"seek"}"#), vec![PlayerEvent::Seek]);
        assert_eq!(
            events(r#"{"event":"playback-restart"}"#),
            vec![PlayerEvent::PlaybackRestart]
        );
        assert_eq!(
            events(r#"{"event":"shutdown"}"#),
            vec![PlayerEvent::Shutdown]
        );
        assert!(events(r#"{"event":"idle"}"#).is_empty());
        assert!(events(r#"{"event":"audio-reconfig"}"#).is_empty());
    }

    #[test]
    fn end_file_maps_reason_and_errors() {
        assert_eq!(
            events(r#"{"event":"end-file","reason":"eof","playlist_entry_id":1}"#),
            vec![PlayerEvent::EndFile {
                reason: "eof",
                file_error: None
            }]
        );
        assert_eq!(
            events(
                r#"{"event":"end-file","reason":"error","playlist_entry_id":1,"file_error":"loading failed"}"#
            ),
            vec![
                PlayerEvent::Error {
                    message: "the stream could not be played (loading failed)".into()
                },
                PlayerEvent::EndFile {
                    reason: "error",
                    file_error: Some("loading failed".into())
                }
            ]
        );
        assert_eq!(
            events(r#"{"event":"end-file","reason":"something-new"}"#),
            vec![PlayerEvent::EndFile {
                reason: "unknown",
                file_error: None
            }]
        );
    }

    #[test]
    fn client_messages_carry_their_args() {
        assert_eq!(
            events(r#"{"event":"client-message","args":["zeiler","next-episode"]}"#),
            vec![PlayerEvent::ClientMessage {
                args: vec!["zeiler".into(), "next-episode".into()]
            }]
        );
        assert_eq!(
            events(r#"{"event":"client-message"}"#),
            vec![PlayerEvent::ClientMessage { args: vec![] }]
        );
    }

    #[test]
    fn time_pos_is_throttled_but_never_lost() {
        let mut throttle = TimePosThrottle::default();
        let t0 = Instant::now();
        assert_eq!(throttle.offer(t0, json!(1.0)), Some(json!(1.0)));
        assert_eq!(throttle.due(), None);
        let t1 = t0 + Duration::from_millis(50);
        assert_eq!(throttle.offer(t1, json!(1.05)), None);
        assert_eq!(throttle.offer(t1, json!(1.1)), None, "newest wins");
        assert_eq!(throttle.due(), Some(t0 + TIME_POS_THROTTLE));
        assert_eq!(throttle.flush(t1), None, "not due yet");
        let t2 = t0 + TIME_POS_THROTTLE;
        assert_eq!(throttle.flush(t2), Some(json!(1.1)));
        assert_eq!(throttle.due(), None);
        // a new file drops a held position
        assert_eq!(
            throttle.offer(t2 + Duration::from_millis(1), json!(9.0)),
            None
        );
        throttle.reset();
        assert_eq!(throttle.flush(t2 + TIME_POS_THROTTLE), None);
    }

    #[test]
    fn argv_has_no_url_headers_or_user_agent() {
        let args = ipc_args("/tmp/zeiler-1000/zeiler-mpv-0123abcd.sock");
        assert_eq!(
            args[0],
            "--input-ipc-server=/tmp/zeiler-1000/zeiler-mpv-0123abcd.sock"
        );
        for flag in [
            "--idle=yes",
            "--keep-open=yes",
            "--force-window=immediate",
            "--title=Zeiler",
            "--ytdl=no",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
        assert!(
            !args.iter().any(|a| a.contains("config")),
            "user config stays on"
        );
    }

    #[test]
    fn load_sends_headers_and_restores_user_defaults() {
        let defaults = Defaults {
            user_agent: "mpv/0.37".into(),
            referrer: String::new(),
            headers: vec!["X-User: 1".into()],
        };
        let request = PlaybackRequest {
            url: "https://u:p@host/v.mkv".into(),
            title: Some("Film".into()),
            headers: vec![("Authorization".into(), "Bearer a,b".into())],
            referrer: Some("https://site.example".into()),
            start_at_sec: Some(90.0),
            options: vec![("alang".into(), "en".into())],
            ..PlaybackRequest::default()
        };
        let commands = load_commands(&request, &defaults);
        let lines: Vec<String> = commands
            .iter()
            .map(|c| Value::Array(c.command.clone()).to_string())
            .collect();
        let optional: Vec<String> = commands
            .iter()
            .filter(|c| !c.required)
            .map(|c| Value::Array(c.command.clone()).to_string())
            .collect();
        assert_eq!(
            optional,
            [
                r#"["set_property","resume-playback","no"]"#,
                r#"["set_property","alang","en"]"#
            ],
            "only the preferences (and the watch-later opt-out) may fail"
        );
        assert_eq!(
            lines,
            vec![
                r#"["change-list","http-header-fields","clr",""]"#,
                r#"["change-list","http-header-fields","append","X-User: 1"]"#,
                r#"["change-list","http-header-fields","append","Authorization: Bearer a,b"]"#,
                r#"["change-list","http-header-fields","append","Referer: https://site.example"]"#,
                r#"["set_property","user-agent","mpv/0.37"]"#,
                r#"["set_property","resume-playback","no"]"#,
                r#"["set_property","start","90"]"#,
                r#"["set_property","force-media-title","Film"]"#,
                r#"["set_property","alang","en"]"#,
                r#"["loadfile","https://u:p@host/v.mkv","replace"]"#,
                r#"["set_property","pause",false]"#,
            ]
        );

        let plain = load_commands(
            &PlaybackRequest {
                url: "https://host/v.mkv".into(),
                ..PlaybackRequest::default()
            },
            &Defaults::default(),
        );
        assert!(
            plain
                .iter()
                .any(|c| c.command == [json!("set_property"), json!("start"), json!("none")])
        );
        assert!(
            !plain
                .iter()
                .any(|c| c.command.get(1) == Some(&json!("referrer"))),
            "no referrer in the user's config: the option is never touched"
        );
    }

    #[test]
    fn user_referrer_is_replaced_not_doubled() {
        // mpv.conf has referrer=…: mpv sends it as a Referer header before the
        // http-header-fields lines, so a stream's referrer must replace the option
        let defaults = Defaults {
            user_agent: "mpv/0.37".into(),
            referrer: "https://user.example/".into(),
            headers: Vec::new(),
        };
        let lines = |request: &PlaybackRequest| -> Vec<String> {
            load_commands(request, &defaults)
                .iter()
                .map(|c| Value::Array(c.command.clone()).to_string())
                .collect()
        };
        let referrer_set = |value: &str| format!(r#"["set_property","referrer","{value}"]"#);

        let with_own = lines(&PlaybackRequest {
            url: "https://cdn.example/v.mkv".into(),
            referrer: Some("https://cdn-site.example/".into()),
            ..PlaybackRequest::default()
        });
        assert!(with_own.contains(&referrer_set("https://cdn-site.example/")));
        assert!(
            !with_own.iter().any(|l| l.contains("Referer:")),
            "{with_own:?}"
        );

        // a Referer among the request's headers counts as its referrer too
        let in_headers = lines(&PlaybackRequest {
            url: "https://cdn.example/v.mkv".into(),
            headers: vec![
                ("referer".into(), "https://hdr.example/".into()),
                ("X-A".into(), "1".into()),
            ],
            ..PlaybackRequest::default()
        });
        assert!(in_headers.contains(&referrer_set("https://hdr.example/")));
        assert!(
            !in_headers
                .iter()
                .any(|l| l.to_ascii_lowercase().contains("referer:"))
        );
        assert!(in_headers.iter().any(|l| l.contains("X-A: 1")));

        // the next load without one gets the user's value back
        let without = lines(&PlaybackRequest {
            url: "https://host/v.mkv".into(),
            ..PlaybackRequest::default()
        });
        assert!(without.contains(&referrer_set("https://user.example/")));
        assert!(!without.iter().any(|l| l.contains("Referer:")));
    }
}
