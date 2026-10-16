//! Playback for Zeiler.
//!
//! Two backends drive mpv and emit the same [`PlayerEvent`] stream:
//! - **external** ([`external_ipc`]): a system (or user-configured) `mpv` binary in its
//!   own window, remote-controlled over mpv's JSON IPC (a Unix socket or a Windows
//!   named pipe). If the IPC connection never comes up, [`Player::play`] still launches
//!   mpv the v0.1 way (URL on stdin, no control afterwards).
//! - **embedded** (`embedded` feature, [`embedded`]): libmpv in-process, rendering
//!   into a native child window under the transparent webview.
//!
//! [`Session`] is the backend-neutral handle the shell keeps for whichever is playing.

#[cfg(feature = "embedded")]
pub mod embedded;
pub mod external_ipc;
pub mod mpv_error;
#[cfg(feature = "embedded")]
pub mod render;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tracing::info;

/// Minimum spacing between forwarded `time-pos` changes, in both backends. mpv fires
/// this every frame; the UI only needs it a few times a second.
pub(crate) const TIME_POS_THROTTLE: Duration = Duration::from_millis(200);

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error(
        "mpv was not found. Install mpv (https://mpv.io) or set its path in Settings, then try again"
    )]
    NoPlayer,
    #[error("failed to launch mpv: {0}")]
    Spawn(#[from] std::io::Error),
}

/// A playback request. Headers come from a stream's `behaviorHints.proxyHeaders.request`
/// and apply to every HTTP request mpv makes for this file.
///
/// This is also the IPC payload of the player commands, so every field the webview
/// can set is listed here explicitly; anything the webview must never control is
/// `#[serde(skip)]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaybackRequest {
    pub url: String,
    pub title: Option<String>,
    pub headers: Vec<(String, String)>,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub start_at_sec: Option<f64>,
    /// Shell-only pre-load mpv options (`name`, `value`), applied with `set_property`
    /// right before `loadfile` — e.g. preferred audio/subtitle languages from settings.
    /// Never deserialized, so the webview cannot inject mpv options through a request;
    /// only Rust code fills this in. Options persist in the core across loads, so the
    /// shell sends its full set with every load. One that mpv refuses (an older
    /// external mpv may not know it) is logged and skipped; the file still loads.
    #[serde(skip)]
    pub options: Vec<(String, String)>,
}

/// The user's backend preference (`playerBackend` setting, and `player_start`'s
/// `backend` argument).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerBackend {
    /// Embedded when this build has it and it starts, otherwise external mpv.
    #[default]
    Auto,
    Embedded,
    External,
}

/// The backend a [`Session`] actually runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Embedded,
    External,
}

/// How a property value is typed. Shared by both backends so they report the same
/// JSON shapes: the embedded backend picks its libmpv read format from it, the
/// external one normalizes mpv's JSON with it ([`normalize`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    Double,
    Int,
    Flag,
    /// mpv's string form (`"no"`, `"auto"`, `"2"` for track ids; titles; codecs).
    Text,
    /// A node (list or map), forwarded as parsed JSON.
    Json,
}

/// Properties observed on behalf of the UI, by both backends (the index is the
/// observation id).
///
/// One cache property is observed: `demuxer-cache-time`, the timestamp (seconds) up
/// to which the demuxer has data buffered — exactly the seek bar's "buffered up to"
/// mark. `demuxer-cache-state` was not chosen: it is a large node (seekable ranges,
/// byte counts, reader state) that changes several times a second, would be
/// re-serialized to JSON on every change in the embedded backend, and is only needed
/// for a ranges view that no UI draws.
pub const OBSERVED: &[(&str, PropKind)] = &[
    ("time-pos", PropKind::Double),
    ("duration", PropKind::Double),
    ("pause", PropKind::Flag),
    ("paused-for-cache", PropKind::Flag),
    ("cache-buffering-state", PropKind::Int),
    ("eof-reached", PropKind::Flag),
    ("seeking", PropKind::Flag),
    ("volume", PropKind::Double),
    ("mute", PropKind::Flag),
    ("media-title", PropKind::Text),
    ("track-list", PropKind::Json),
    ("chapter-list", PropKind::Json),
    ("aid", PropKind::Text),
    ("sid", PropKind::Text),
    ("vid", PropKind::Text),
    ("secondary-sid", PropKind::Text),
    ("sub-delay", PropKind::Double),
    ("audio-delay", PropKind::Double),
    ("speed", PropKind::Double),
    ("chapter", PropKind::Int),
    ("dwidth", PropKind::Int),
    ("dheight", PropKind::Int),
    ("demuxer-cache-time", PropKind::Double),
];

/// Types of properties that are read on demand (`get_property`) but not observed.
const READ_KINDS: &[(&str, PropKind)] = &[
    ("video-codec", PropKind::Text),
    ("audio-codec", PropKind::Text),
    ("hwdec-current", PropKind::Text),
    ("estimated-vf-fps", PropKind::Double),
    ("container-fps", PropKind::Double),
    ("video-bitrate", PropKind::Double),
    ("audio-bitrate", PropKind::Double),
    ("frame-drop-count", PropKind::Int),
    ("vo-drop-frame-count", PropKind::Int),
    ("demuxer-cache-duration", PropKind::Double),
    ("cache-speed", PropKind::Int),
    ("mpv-version", PropKind::Text),
];

/// The type of a property; unknown names read as text.
pub fn property_kind(name: &str) -> PropKind {
    OBSERVED
        .iter()
        .chain(READ_KINDS)
        .find(|(n, _)| *n == name)
        .map_or(PropKind::Text, |(_, kind)| *kind)
}

/// Bring a JSON value as mpv's JSON IPC reports it into the shape the embedded backend
/// reports for the same property. Only text properties differ: over JSON IPC a track
/// id is a number (or `false` for "no"), while libmpv's string format gives `"2"`/`"no"`.
/// `null` (property unavailable) stays `null`.
pub fn normalize(kind: PropKind, value: Value) -> Value {
    match (kind, value) {
        (PropKind::Text, Value::Bool(b)) => Value::String(if b { "yes" } else { "no" }.into()),
        (PropKind::Text, Value::Number(n)) => Value::String(n.to_string()),
        (_, value) => value,
    }
}

/// Event stream forwarded to the UI (`player://event`), identical for both backends.
/// Serialized as `{ "type": ..., ... }`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PlayerEvent {
    /// An observed property changed. List-valued properties arrive as parsed JSON
    /// (arrays/objects), scalars as JSON scalars; `null` while unavailable.
    Property {
        name: String,
        value: Value,
    },
    StartFile,
    FileLoaded,
    Seek,
    PlaybackRestart,
    /// Playback of the current file ended. `reason` is one of
    /// `eof | stop | quit | error | redirect | unknown`. `fileError` is mpv's own
    /// error text when the external backend reports one.
    EndFile {
        reason: &'static str,
        #[serde(rename = "fileError", skip_serializing_if = "Option::is_none")]
        file_error: Option<String>,
    },
    /// A non-fatal player error (including end-of-file on error).
    Error {
        message: String,
    },
    /// A `script-message` addressed to clients (mpv `client-message` event): user
    /// key bindings in the external mpv window can talk to Zeiler this way.
    ClientMessage {
        args: Vec<String>,
    },
    /// The mpv core is gone (quit, crashed, or the user closed the external mpv
    /// window); no further events will arrive from this session.
    Shutdown,
}

/// Map mpv's end-file reason names to the strings [`PlayerEvent::EndFile`] carries.
pub(crate) fn end_file_reason_name(reason: &str) -> &'static str {
    match reason {
        "eof" => "eof",
        "stop" => "stop",
        "quit" => "quit",
        "error" => "error",
        "redirect" => "redirect",
        _ => "unknown",
    }
}

/// Errors of a running [`Session`].
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("the player has shut down")]
    Closed,
    /// mpv refused or failed the request; the text is mpv's (or ours) and readable.
    #[error("{0}")]
    Mpv(String),
}

/// One running player, whichever backend it is on.
pub enum Session {
    #[cfg(feature = "embedded")]
    Embedded(embedded::EmbeddedPlayer),
    External(external_ipc::ExternalIpcPlayer),
}

impl Session {
    pub fn backend(&self) -> Backend {
        match self {
            #[cfg(feature = "embedded")]
            Session::Embedded(_) => Backend::Embedded,
            Session::External(_) => Backend::External,
        }
    }

    /// Load (replace) the current file.
    pub async fn load(&self, request: PlaybackRequest) -> Result<(), SessionError> {
        match self {
            #[cfg(feature = "embedded")]
            Session::Embedded(player) => player.load(request).map_err(embedded_error),
            Session::External(player) => player.load(&request).await,
        }
    }

    /// Set a property. Callers validate names and values first (the shell's table).
    pub async fn set_property(&self, name: &str, value: Value) -> Result<(), SessionError> {
        match self {
            #[cfg(feature = "embedded")]
            Session::Embedded(player) => player
                .set_property(name.to_string(), value)
                .map_err(embedded_error),
            Session::External(player) => player.set_property(name, value).await,
        }
    }

    /// Run a command (argv form). Callers validate it first.
    pub async fn command(&self, args: Vec<String>) -> Result<(), SessionError> {
        match self {
            #[cfg(feature = "embedded")]
            Session::Embedded(player) => player.command(args).map_err(embedded_error),
            Session::External(player) => player.command(args).await,
        }
    }

    /// Run a command and wait for mpv's answer, on either backend: for commands whose
    /// failure must reach the user, such as `sub-add`. Callers validate it first.
    pub async fn command_checked(&self, args: Vec<String>) -> Result<(), SessionError> {
        match self {
            #[cfg(feature = "embedded")]
            Session::Embedded(player) => player.command_checked(args).await,
            Session::External(player) => player.command(args).await,
        }
    }

    /// Read a property, typed per [`property_kind`].
    pub async fn get_property(&self, name: &str) -> Result<Value, SessionError> {
        match self {
            #[cfg(feature = "embedded")]
            Session::Embedded(player) => player.get_property(name.to_string()).await,
            Session::External(player) => player.get_property(name).await,
        }
    }

    /// Ask the player to quit. Never blocks; a `Shutdown` event follows.
    pub fn quit(&self) {
        match self {
            #[cfg(feature = "embedded")]
            Session::Embedded(player) => player.quit(),
            Session::External(player) => player.quit(),
        }
    }
}

#[cfg(feature = "embedded")]
fn embedded_error(error: embedded::EmbeddedError) -> SessionError {
    match error {
        embedded::EmbeddedError::Closed => SessionError::Closed,
        other => SessionError::Mpv(other.to_string()),
    }
}

/// The per-stream HTTP header lines of a request, as both backends append them to
/// mpv's `http-header-fields` (cleared before every load): the request's headers, then
/// its referrer as a `Referer` header unless the headers already carry one.
///
/// The referrer travels as a header rather than mpv's `referrer` option because that
/// option, once set, can only be set to an empty string again — and mpv then sends an
/// empty `Referer:` with every later stream. A header in the list is gone with the
/// next `clr`. This is right only while the `referrer` option is unset: mpv sends the
/// option as its own `Referer` line ahead of the list, so the external backend, which
/// keeps the user's mpv config, sets the option per load instead when that config has
/// a referrer (`external_ipc::load_commands`). The embedded core never reads a config.
pub(crate) fn header_lines(request: &PlaybackRequest) -> Vec<String> {
    let mut lines: Vec<String> = request
        .headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect();
    let has_referer = request
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("referer"));
    if let Some(referrer) = request.referrer.as_deref().filter(|r| !r.is_empty())
        && !has_referer
    {
        lines.push(format!("Referer: {referrer}"));
    }
    lines
}

/// Build the mpv CLI arguments for a request. The URL itself is NOT part of the argv —
/// it is fed through stdin (`--playlist=-`) so loopback stream URLs carrying per-run
/// credentials never show up in the process list. Each header goes through
/// `--http-header-fields-append` so commas in values cannot corrupt the list option.
/// (Headers remain on argv here. This launch is only the fallback for when mpv's JSON IPC
/// does not come up; the IPC backend sends headers over the socket instead.)
pub fn build_mpv_args(request: &PlaybackRequest) -> Vec<String> {
    let mut args = vec![
        "--force-window=immediate".to_string(),
        "--keep-open=yes".to_string(),
        "--playlist=-".to_string(),
    ];
    if let Some(title) = &request.title {
        args.push(format!("--title={title}"));
        args.push(format!("--force-media-title={title}"));
    }
    if let Some(ua) = &request.user_agent {
        args.push(format!("--user-agent={ua}"));
    }
    if let Some(referrer) = &request.referrer {
        args.push(format!("--referrer={referrer}"));
    }
    for (name, value) in &request.headers {
        args.push(format!("--http-header-fields-append={name}: {value}"));
    }
    // the library owns resume positions, not the user's mpv watch-later files
    args.push("--resume-playback=no".to_string());
    if let Some(start) = request.start_at_sec {
        args.push(format!("--start={start}"));
    }
    args
}

/// Strip userinfo from a URL for logging (`http://user:pass@host/…` → `http://***@host/…`).
pub fn redact_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        let authority_end = rest.find('/').unwrap_or(rest.len());
        if let Some(at) = rest[..authority_end].rfind('@') {
            return format!("{}***{}", &url[..scheme_end + 3], &rest[at..]);
        }
    }
    url.to_string()
}

/// Locate an mpv binary: an explicit configured path first, then `mpv(.exe)` on PATH.
pub fn locate_mpv(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = configured {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        return None;
    }
    let exe = if cfg!(windows) { "mpv.exe" } else { "mpv" };
    let on_path = std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var)
            .map(|dir| dir.join(exe))
            .find(|candidate| candidate.is_file())
    });
    // Apps launched from Finder / the Start Menu / a desktop launcher don't inherit the
    // shell's PATH, so a Homebrew/winget/scoop mpv is invisible to them without this.
    on_path.or_else(|| known_locations().into_iter().find(|p| p.is_file()))
}

/// Standard install locations of mpv per OS, most likely first.
fn known_locations() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(windows) {
        let env_dir = |var: &str| std::env::var_os(var).map(PathBuf::from);
        if let Some(local) = env_dir("LOCALAPPDATA") {
            paths.push(local.join(r"Microsoft\WinGet\Links\mpv.exe"));
            // winget portable packages live in a source-suffixed folder
            if let Ok(entries) = std::fs::read_dir(local.join(r"Microsoft\WinGet\Packages")) {
                paths.extend(
                    entries
                        .flatten()
                        .filter(|e| e.file_name().to_string_lossy().starts_with("shinchiro.mpv"))
                        .map(|e| e.path().join("mpv.exe")),
                );
            }
        }
        if let Some(home) = env_dir("USERPROFILE") {
            paths.push(home.join(r"scoop\shims\mpv.exe"));
            paths.push(home.join(r"scoop\apps\mpv\current\mpv.exe"));
        }
        if let Some(program_data) = env_dir("ProgramData") {
            paths.push(program_data.join(r"chocolatey\bin\mpv.exe"));
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(dir) = env_dir(var) {
                paths.push(dir.join(r"mpv\mpv.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        for p in [
            "/opt/homebrew/bin/mpv",
            "/usr/local/bin/mpv",
            "/opt/local/bin/mpv",
            "/Applications/mpv.app/Contents/MacOS/mpv",
        ] {
            paths.push(PathBuf::from(p));
        }
    } else {
        for p in ["/usr/bin/mpv", "/usr/local/bin/mpv", "/snap/bin/mpv"] {
            paths.push(PathBuf::from(p));
        }
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".local/bin/mpv"));
        }
    }
    paths
}

/// Locates mpv, and launches it the v0.1 way when the JSON IPC backend cannot be used:
/// URL on stdin, no control afterwards. Keeps at most one such child process; starting a
/// new playback stops the previous one.
pub struct Player {
    binary: Mutex<Option<PathBuf>>,
    child: Mutex<Option<Child>>,
}

impl Player {
    pub fn new(configured: Option<&Path>) -> Self {
        Self {
            binary: Mutex::new(locate_mpv(configured)),
            child: Mutex::new(None),
        }
    }

    /// Re-run binary discovery (after the user changes the configured path).
    pub fn relocate(&self, configured: Option<&Path>) {
        *self.binary.lock().unwrap() = locate_mpv(configured);
    }

    pub fn binary(&self) -> Option<PathBuf> {
        self.binary.lock().unwrap().clone()
    }

    pub async fn play(&self, request: &PlaybackRequest) -> Result<(), PlayerError> {
        let binary = self.binary().ok_or(PlayerError::NoPlayer)?;
        let args = build_mpv_args(request);
        info!(player = %binary.display(), url = %redact_url(&request.url), "launching external player");
        let mut child = Command::new(&binary)
            .args(&args)
            .stdin(Stdio::piped())
            .kill_on_drop(false)
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(request.url.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.shutdown().await?;
        }
        // Swap atomically under the lock so concurrent plays can never orphan a child,
        // then stop whatever was playing before.
        let previous = self.child.lock().unwrap().replace(child);
        if let Some(mut previous) = previous {
            let _ = previous.start_kill();
        }
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_args_with_headers_and_title_but_no_url() {
        let args = build_mpv_args(&PlaybackRequest {
            url: "https://user:secret@127.0.0.1:9999/v.mp4".into(),
            title: Some("Big Buck Bunny".into()),
            headers: vec![("Referer".into(), "https://site.example".into())],
            user_agent: Some("Zeiler".into()),
            referrer: None,
            start_at_sec: Some(42.5),
            options: Vec::new(),
        });
        assert!(args.contains(&"--playlist=-".to_string()));
        assert!(args.contains(&"--force-media-title=Big Buck Bunny".to_string()));
        assert!(args.contains(&"--user-agent=Zeiler".to_string()));
        assert!(
            args.contains(&"--http-header-fields-append=Referer: https://site.example".to_string())
        );
        assert!(args.contains(&"--start=42.5".to_string()));
        assert!(args.contains(&"--resume-playback=no".to_string()));
        // the URL (and its credentials) must never be an argument
        assert!(args.iter().all(|a| !a.contains("secret")));
    }

    #[test]
    fn the_referrer_travels_as_a_header() {
        let mut request = PlaybackRequest {
            headers: vec![("X-A".into(), "1, 2".into())],
            referrer: Some("https://site.example/".into()),
            ..PlaybackRequest::default()
        };
        assert_eq!(
            header_lines(&request),
            vec!["X-A: 1, 2", "Referer: https://site.example/"]
        );
        // an explicit Referer header wins; an empty referrer adds nothing
        request
            .headers
            .push(("referer".into(), "https://other.example/".into()));
        assert_eq!(header_lines(&request).len(), 2);
        request.headers.clear();
        request.referrer = Some(String::new());
        assert!(header_lines(&request).is_empty());
    }

    #[test]
    fn the_webview_cannot_set_shell_only_options() {
        let request: PlaybackRequest = serde_json::from_value(serde_json::json!({
            "url": "https://host/v.mkv",
            "options": [["script", "/tmp/evil.lua"]],
        }))
        .unwrap();
        assert_eq!(request.url, "https://host/v.mkv");
        assert!(request.options.is_empty());
        let mut with_options = request.clone();
        with_options.options.push(("alang".into(), "en".into()));
        let value = serde_json::to_value(&with_options).unwrap();
        assert!(
            value.get("options").is_none(),
            "never sent to the webview either"
        );
    }

    #[test]
    fn backend_preferences_use_the_settings_spelling() {
        for (backend, text) in [
            (PlayerBackend::Auto, "auto"),
            (PlayerBackend::Embedded, "embedded"),
            (PlayerBackend::External, "external"),
        ] {
            assert_eq!(serde_json::to_value(backend).unwrap(), text);
            let back: PlayerBackend = serde_json::from_value(text.into()).unwrap();
            assert_eq!(back, backend);
        }
        assert!(serde_json::from_value::<PlayerBackend>("vlc".into()).is_err());
        assert_eq!(serde_json::to_value(Backend::External).unwrap(), "external");
    }

    #[test]
    fn observed_properties_are_unique_and_typed() {
        let mut names: Vec<&str> = OBSERVED.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OBSERVED.len());
        for name in [
            "sub-delay",
            "audio-delay",
            "speed",
            "secondary-sid",
            "chapter",
            "dwidth",
            "dheight",
            "demuxer-cache-time",
        ] {
            assert!(names.contains(&name), "{name} is observed");
        }
        assert_eq!(property_kind("track-list"), PropKind::Json);
        assert_eq!(property_kind("frame-drop-count"), PropKind::Int);
        assert_eq!(property_kind("no-such-thing"), PropKind::Text);
    }

    #[test]
    fn json_ipc_values_take_the_embedded_shapes() {
        use serde_json::json;
        assert_eq!(normalize(PropKind::Text, json!(2)), json!("2"));
        assert_eq!(normalize(PropKind::Text, json!(false)), json!("no"));
        assert_eq!(normalize(PropKind::Text, json!("auto")), json!("auto"));
        assert_eq!(normalize(PropKind::Text, Value::Null), Value::Null);
        assert_eq!(normalize(PropKind::Double, json!(1.5)), json!(1.5));
        assert_eq!(normalize(PropKind::Json, json!([1])), json!([1]));
    }

    #[test]
    fn events_serialize_kebab_case_tagged() {
        use serde_json::json;
        assert_eq!(
            serde_json::to_value(PlayerEvent::EndFile {
                reason: "error",
                file_error: Some("loading failed".into())
            })
            .unwrap(),
            json!({"type": "end-file", "reason": "error", "fileError": "loading failed"})
        );
        assert_eq!(
            serde_json::to_value(PlayerEvent::EndFile {
                reason: "eof",
                file_error: None
            })
            .unwrap(),
            json!({"type": "end-file", "reason": "eof"})
        );
        assert_eq!(
            serde_json::to_value(PlayerEvent::ClientMessage {
                args: vec!["zeiler".into(), "next".into()]
            })
            .unwrap(),
            json!({"type": "client-message", "args": ["zeiler", "next"]})
        );
        assert_eq!(
            serde_json::to_value(PlayerEvent::Shutdown).unwrap(),
            json!({"type": "shutdown"})
        );
    }

    #[test]
    fn redacts_userinfo() {
        assert_eq!(
            redact_url("http://u:p@127.0.0.1:1/x/y?z=1"),
            "http://***@127.0.0.1:1/x/y?z=1"
        );
        assert_eq!(redact_url("https://host/a@b"), "https://host/a@b");
        assert_eq!(redact_url("not a url"), "not a url");
    }

    #[test]
    fn locate_with_bogus_configured_path_returns_none() {
        assert!(locate_mpv(Some(Path::new("Z:/definitely/not/mpv.exe"))).is_none());
    }

    #[test]
    fn known_locations_cover_the_platform_package_managers() {
        let paths: Vec<String> = known_locations()
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        if cfg!(windows) {
            assert!(
                paths
                    .iter()
                    .any(|p| p.ends_with("Microsoft/WinGet/Links/mpv.exe"))
            );
            assert!(paths.iter().any(|p| p.ends_with("scoop/shims/mpv.exe")));
        } else if cfg!(target_os = "macos") {
            assert!(paths.iter().any(|p| p == "/opt/homebrew/bin/mpv"));
        } else {
            assert!(paths.iter().any(|p| p == "/usr/bin/mpv"));
        }
    }
}
