//! Playback commands: one backend-neutral player, embedded or external mpv.
//!
//! - **Backends.** `player_start` picks the backend in Rust, because embedded init
//!   and video-output failures are synchronous: `auto` means embedded when this
//!   build has it and it starts, otherwise external mpv over JSON IPC (and if that
//!   IPC never comes up, the plain v0.1 launch without remote control). The result
//!   says which backend runs and why it fell back. `auto` follows the
//!   `playerBackend` setting.
//! - **One session.** At most one player runs; starting another replaces it. Its
//!   events go to the webview as `player://event` (`PlayerEvent`, the same shapes
//!   for both backends); events from a replaced session are dropped.
//! - **Validation.** Property writes, property reads and commands from the UI are
//!   checked against the tables in [`validate`]; requests are scheme- and
//!   header-checked here before anything reaches mpv.
//! - **Library progress.** `player_start` and `player_load` take an optional
//!   `context` (the title and video being played); the session's events then also
//!   feed a progress tracker (`crate::library::progress`) that writes the library.
//!   Replacing, stopping or loading over a session closes its tracker first, so its
//!   last write always lands before the next playback's first.
//! - **Per-OS surfaces.** The embedded video surface is OS code: `windows.rs`
//!   (libmpv child window under the transparent WebView2; its module docs hold the
//!   transparency policy), `macos.rs`/`linux.rs` (not available yet) and `stub.rs`
//!   (feature off). `player_set_geometry` and `player_surface_refresh` act on it.
//!
//! The pre-M2 command names stay as thin aliases for one release (`aliases.rs`).

pub mod aliases;
pub mod audio;
pub mod osd;
pub mod subtitles;
pub mod validate;

#[cfg(all(windows, feature = "embedded"))]
mod windows;
#[cfg(all(windows, feature = "embedded"))]
use self::windows as imp;

#[cfg(all(target_os = "macos", feature = "embedded"))]
mod macos;
#[cfg(all(target_os = "macos", feature = "embedded"))]
use self::macos as imp;

#[cfg(all(target_os = "linux", feature = "embedded"))]
mod linux;
#[cfg(all(target_os = "linux", feature = "embedded"))]
use self::linux as imp;

#[cfg(not(all(windows, feature = "embedded")))]
mod stub;
#[cfg(not(all(
    feature = "embedded",
    any(windows, target_os = "macos", target_os = "linux")
)))]
use self::stub as imp;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;
use tracing::warn;
use zeiler_player::external_ipc::{ExternalError, ExternalIpcPlayer};
use zeiler_player::{
    Backend, PlaybackRequest, PlayerBackend, PlayerError, PlayerEvent, Session, SessionError,
};

use crate::error::IpcError;
use crate::library::LibraryState;
use crate::library::progress::{OwnFile, PlaybackContext, SessionProgress};
use crate::state::AppState;
use crate::subtitles::SubtitleStore;
use crate::subtitles::prefs::{PlayerPrefs, TitlePrefs};

/// The Tauri event carrying every [`PlayerEvent`].
pub const PLAYER_EVENT: &str = "player://event";

/// Target rectangle for the embedded video, in physical pixels relative to the main
/// window's client area (the UI computes getBoundingClientRect() × devicePixelRatio).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
// only the Windows surface reads the fields
#[cfg_attr(not(all(windows, feature = "embedded")), allow(dead_code))]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Player state managed by Tauri: the running session and the embedded surface.
#[derive(Default)]
pub struct PlayerState {
    slot: Mutex<Option<Active>>,
    /// The session whose events reach the webview (the latest one started).
    current: AtomicU64,
    next_id: AtomicU64,
    /// Serializes start/load/stop, so two starts can never both spawn a player.
    op: tokio::sync::Mutex<()>,
    surface: imp::Surface,
    /// Library progress of the playback in session `.0` (absent: not tracked).
    progress: Mutex<Option<(u64, Arc<SessionProgress>)>>,
    /// Counts files started (`player_start`, `player_load`, and another file the user
    /// opened in the player), so work for one file (a subtitle download) is never
    /// applied to the next.
    loads: AtomicU64,
    /// Tells session `.0`'s own file (the one `player_start` or `player_load` played
    /// last) from another one the user opened in the same player, as external mpv
    /// allows; the progress tracker follows the same rule.
    own_file: Mutex<(u64, OwnFile)>,
    /// What the file of ours playing now was loaded from (`player_start`/`player_load`),
    /// with its session and the load count it belongs to: the subtitle hash reads it
    /// (`subtitles.rs`), so the URL and headers never travel back through the webview.
    playing: Mutex<Option<(u64, u64, PlaybackRequest)>>,
}

#[derive(Clone)]
struct Active {
    id: u64,
    session: Arc<Session>,
}

impl PlayerState {
    fn active(&self) -> Option<Active> {
        self.slot.lock().unwrap().clone()
    }

    fn put(&self, id: u64, session: Session) {
        let previous = self.slot.lock().unwrap().replace(Active {
            id,
            session: Arc::new(session),
        });
        if let Some(previous) = previous {
            previous.session.quit();
        }
    }

    /// The session `id` shut down on its own: forget it (if still current).
    fn forget(&self, id: u64) {
        let gone = {
            let mut slot = self.slot.lock().unwrap();
            if slot.as_ref().is_some_and(|a| a.id == id) {
                slot.take()
            } else {
                None
            }
        };
        drop(gone);
    }

    /// The progress tracker fed by session `id`'s events.
    fn progress_for(&self, id: u64) -> Option<Arc<SessionProgress>> {
        self.progress
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(owner, _)| *owner == id)
            .map(|(_, progress)| progress.clone())
    }

    /// Track session `id`'s playback with `progress` from now on, after closing the
    /// tracker before it (its last write is queued ahead of anything the new one
    /// writes).
    fn track(&self, id: u64, progress: Option<Arc<SessionProgress>>) {
        self.close_progress();
        *self.progress.lock().unwrap() = progress.map(|progress| (id, progress));
    }

    /// Stop tracking the current playback: its last write is queued, and the receiver
    /// resolves once it is on disk (`None`: nothing to write).
    pub fn close_progress(&self) -> Option<oneshot::Receiver<()>> {
        let closing = self.progress.lock().unwrap().take();
        closing.and_then(|(_, progress)| progress.close())
    }

    /// How many files have been started so far (see `loads`).
    fn load_count(&self) -> u64 {
        self.loads.load(Ordering::SeqCst)
    }

    /// Session `id` plays a file of ours (`player_start`, `player_load`): work for the
    /// file before is stale from now on, and the session's next `start-file` is this
    /// file's own.
    fn new_file(&self, id: u64) {
        *self.own_file.lock().unwrap() = (id, OwnFile::default());
        self.loads.fetch_add(1, Ordering::SeqCst);
    }

    /// Remember what session `id`'s file of ours just started was loaded from.
    fn remember_request(&self, id: u64, request: &PlaybackRequest) {
        *self.playing.lock().unwrap() = Some((id, self.load_count(), request.clone()));
    }

    /// What the file playing now was loaded from, if it is still ours: its session is
    /// the running one (not stopped or replaced) and no other file came since (another
    /// one the user opened in external mpv) — with the load count it belongs to.
    fn playing_request(&self) -> Option<(u64, PlaybackRequest)> {
        let active = self.active()?.id;
        let (id, loads, request) = self.playing.lock().unwrap().clone()?;
        (id == active && loads == self.load_count()).then_some((loads, request))
    }

    /// Take one of session `id`'s events: another file opened in its player counts as
    /// a new file too, so a subtitle add started for the file before fails instead of
    /// landing on it.
    fn watch_file(&self, id: u64, event: &PlayerEvent) {
        let mut own_file = self.own_file.lock().unwrap();
        let (owner, own) = &mut *own_file;
        if *owner == id && own.another_file(event) {
            self.loads.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Whether the running session (if any) is embedded.
    fn embedded_active(&self) -> bool {
        self.active()
            .is_some_and(|a| a.session.backend() == Backend::Embedded)
    }

    fn session(&self) -> Result<Arc<Session>, IpcError> {
        self.active()
            .map(|a| a.session)
            .ok_or_else(|| IpcError::Player("Nothing is playing.".into()))
    }

    /// Quit the running session, if any (and, if `only` is set, only when it runs on
    /// that backend). Returns the backend that was stopped.
    fn stop_session(&self, only: Option<Backend>) -> Option<Backend> {
        let stopped = {
            let mut slot = self.slot.lock().unwrap();
            match &*slot {
                Some(active) if !stop_matches(active.session.backend(), only) => None,
                _ => slot.take(),
            }
        }?;
        stopped.session.quit();
        Some(stopped.session.backend())
    }
}

/// Whether a stop limited to `only` (none = any backend) applies to a session on
/// `running`.
fn stop_matches(running: Backend, only: Option<Backend>) -> bool {
    only.is_none_or(|backend| backend == running)
}

/// What `player_start` did.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    /// The backend actually playing.
    pub backend: Backend,
    /// Set when embedded was wanted (auto) but failed to start: why. Playback then
    /// went to external mpv.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    /// False only for the last-resort external launch without JSON IPC: mpv plays,
    /// but Zeiler cannot control it or see its state.
    pub remote_control: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedStatus {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libmpv_version: Option<String>,
}

impl EmbeddedStatus {
    fn unavailable(reason: String) -> Self {
        Self {
            available: false,
            reason: Some(reason),
            libmpv_version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalStatus {
    pub available: bool,
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backends {
    pub embedded: EmbeddedStatus,
    pub external: ExternalStatus,
}

fn external_status(state: &AppState) -> ExternalStatus {
    let path = state.player.binary();
    ExternalStatus {
        available: path.is_some(),
        path: path.map(|p| p.display().to_string()),
    }
}

/// What each backend can do on this machine and build.
#[tauri::command]
pub async fn player_backends(state: State<'_, AppState>) -> Result<Backends, IpcError> {
    Ok(Backends {
        embedded: imp::embedded_status(),
        external: external_status(&state),
    })
}

/// Start (or replace) playback of an http(s) stream. `backend` defaults to `auto`,
/// which follows the `playerBackend` setting. Returns the backend actually used.
/// With `context` (the title and video), the playback's progress is written to the
/// library.
#[tauri::command]
pub async fn player_start(
    app: AppHandle,
    state: State<'_, AppState>,
    player: State<'_, PlayerState>,
    mut request: PlaybackRequest,
    backend: Option<PlayerBackend>,
    context: Option<serde_json::Value>,
) -> Result<StartResult, IpcError> {
    check_request(&mut request)?;
    let settings = state.settings.read().await.clone();
    let preference = match backend.unwrap_or_default() {
        PlayerBackend::Auto => settings.player_backend,
        explicit => explicit,
    };
    let context = checked_context(context);
    let title = title_prefs(&app, context.as_ref()).await;
    request.options = crate::subtitles::player_options(&settings, context.as_ref(), title.as_ref());
    let started = start(&app, &state, &player, request, preference, context).await?;
    // the subtitle look (only what the user set; a refusal costs only that field) and
    // volume normalization (a filter stays set across loads in a core)
    if let Ok(session) = player.session() {
        let _ = subtitles::apply_style(&session, &settings.subtitles.style).await;
        if settings.audio.normalize {
            let _ = audio::apply_normalize(&session, true).await;
        }
    }
    Ok(started)
}

/// Load another file into the running player (same window, same backend). Progress
/// of the file it replaces is closed; `context` tracks the new one.
#[tauri::command]
pub async fn player_load(
    app: AppHandle,
    state: State<'_, AppState>,
    player: State<'_, PlayerState>,
    mut request: PlaybackRequest,
    context: Option<serde_json::Value>,
) -> Result<(), IpcError> {
    check_request(&mut request)?;
    let context = checked_context(context);
    let title = title_prefs(&app, context.as_ref()).await;
    let settings = state.settings.read().await.clone();
    request.options = crate::subtitles::player_options(&settings, context.as_ref(), title.as_ref());
    let _op = player.op.lock().await;
    let active = player
        .active()
        .ok_or_else(|| IpcError::Player("Nothing is playing.".into()))?;
    player.new_file(active.id);
    player.remember_request(active.id, &request);
    player.track(active.id, progress(&app, context));
    // (normalization follows the setting at every file, so a change made meanwhile
    // reaches a player kept running)
    let _ = audio::apply_normalize(&active.session, settings.audio.normalize).await;
    active.session.load(request).await.map_err(session_error)
}

/// Stop whatever is playing, on any backend (including an external mpv launched
/// without remote control). With `only`, stop just a session on that backend: the
/// player screen uses `only: "embedded"` to clear a hidden built-in video without
/// closing an external mpv the user left playing.
#[tauri::command]
pub async fn player_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    player: State<'_, PlayerState>,
    only: Option<Backend>,
) -> Result<(), IpcError> {
    let _op = player.op.lock().await;
    // the tracked playback ends with this stop unless `only` spares its session
    if player
        .active()
        .is_none_or(|active| stop_matches(active.session.backend(), only))
    {
        player.close_progress();
        if let Some(store) = app.try_state::<SubtitleStore>() {
            store.purge();
        }
    }
    player.stop_session(only);
    // `only: "external"` leaves a built-in video playing: keep its surface
    if !player.embedded_active() {
        imp::on_stop(&app, &player.surface);
    }
    if stop_matches(Backend::External, only) {
        state.player.stop();
    }
    Ok(())
}

/// Set an allow-listed property to a validated value (see [`validate`]).
#[tauri::command]
pub async fn player_set_property(
    player: State<'_, PlayerState>,
    name: String,
    value: serde_json::Value,
) -> Result<(), IpcError> {
    validate::validate_property(&name, &value).map_err(IpcError::InvalidInput)?;
    player
        .session()?
        .set_property(&name, value)
        .await
        .map_err(session_error)
}

/// Run an allow-listed command with validated arguments (see [`validate`]).
#[tauri::command]
pub async fn player_command(
    player: State<'_, PlayerState>,
    args: Vec<String>,
) -> Result<(), IpcError> {
    validate::validate_command(&args).map_err(IpcError::InvalidInput)?;
    player.session()?.command(args).await.map_err(session_error)
}

/// Read an allow-listed property (diagnostics, or the observed set).
#[tauri::command]
pub async fn player_get_property(
    player: State<'_, PlayerState>,
    name: String,
) -> Result<serde_json::Value, IpcError> {
    validate::validate_readable(&name).map_err(IpcError::InvalidInput)?;
    player
        .session()?
        .get_property(&name)
        .await
        .map_err(session_error)
}

/// Position the embedded video (physical px, client-area relative).
#[tauri::command]
pub async fn player_set_geometry(
    app: AppHandle,
    player: State<'_, PlayerState>,
    geometry: Geometry,
) -> Result<(), IpcError> {
    imp::set_geometry(&app, &player.surface, geometry, player.embedded_active())
}

/// Re-assert the embedded video surface after a window change (fullscreen, resize,
/// move, DPI): on Windows the transparent webview background, the video window pinned
/// under the webview with the last geometry, and the keyboard focus. A no-op when no
/// built-in video plays, and on builds and systems without one.
#[tauri::command]
pub async fn player_surface_refresh(
    app: AppHandle,
    player: State<'_, PlayerState>,
) -> Result<(), IpcError> {
    imp::refresh_surface(&app, &player.surface, player.embedded_active())
}

// --- start ---------------------------------------------------------------------------------

async fn start(
    app: &AppHandle,
    state: &AppState,
    player: &PlayerState,
    request: PlaybackRequest,
    preference: PlayerBackend,
    context: Option<PlaybackContext>,
) -> Result<StartResult, IpcError> {
    let _op = player.op.lock().await;
    let id = player.next_id.fetch_add(1, Ordering::SeqCst) + 1;
    // from here on, events of the session being replaced are dropped
    player.current.store(id, Ordering::SeqCst);
    player.new_file(id);
    player.remember_request(id, &request);
    // the replaced session's subtitle files go with it
    if let Some(store) = app.try_state::<SubtitleStore>() {
        store.purge();
    }
    // and its progress is closed before this playback's can start
    player.track(id, progress(app, context));
    // A built-in session that is replaced leaves its surface set up: a new built-in
    // session takes it over (no flash of the opaque background in between); every
    // other outcome below gives the plain window back (`on_stop`, idempotent).
    player.stop_session(None);
    state.player.stop();

    let mut fallback_reason = None;
    if preference != PlayerBackend::External {
        let status = imp::embedded_status();
        if status.available {
            match imp::spawn_embedded(app, &player.surface, event_sink(app, id)) {
                Ok(session) => {
                    if let Err(e) = session.load(request).await {
                        session.quit();
                        imp::on_stop(app, &player.surface);
                        return Err(session_error(e));
                    }
                    player.put(id, session);
                    imp::after_start(app, &player.surface);
                    return Ok(StartResult {
                        backend: Backend::Embedded,
                        fallback_reason: None,
                        remote_control: true,
                    });
                }
                Err(reason) if preference == PlayerBackend::Auto => {
                    warn!(%reason, "embedded player failed to start; using external mpv");
                    fallback_reason = Some(reason);
                }
                Err(reason) => {
                    imp::on_stop(app, &player.surface);
                    return Err(IpcError::Player(reason));
                }
            }
        } else if preference == PlayerBackend::Embedded {
            imp::on_stop(app, &player.surface);
            return Err(IpcError::Unsupported(status.reason.unwrap_or_else(|| {
                "Built-in playback is not available.".to_string()
            })));
        }
    }
    imp::on_stop(app, &player.surface);

    let binary = state.player.binary().ok_or(PlayerError::NoPlayer)?;
    let events = Arc::new(event_sink(app, id));
    match ExternalIpcPlayer::start(&binary, &request, events).await {
        Ok(session) => {
            player.put(id, Session::External(session));
            Ok(StartResult {
                backend: Backend::External,
                fallback_reason,
                remote_control: true,
            })
        }
        Err(ExternalError::IpcUnavailable(reason)) => {
            warn!(%reason, "mpv's JSON IPC did not come up; launching it without remote control");
            state.player.play(&request).await?;
            Ok(StartResult {
                backend: Backend::External,
                fallback_reason,
                remote_control: false,
            })
        }
        Err(ExternalError::Spawn(e)) => Err(PlayerError::Spawn(e).into()),
        Err(e) => Err(IpcError::Player(e.to_string())),
    }
}

/// The context, if it is well-formed: a malformed one only costs the library entry,
/// never the playback. (It arrives as plain JSON and is read here, so not even a shape
/// error can refuse the command.)
fn checked_context(context: Option<serde_json::Value>) -> Option<PlaybackContext> {
    let context = serde_json::from_value::<PlaybackContext>(context?)
        .map_err(|e| IpcError::InvalidInput(e.to_string()))
        .and_then(|context| context.check().map(|()| context));
    match context {
        Ok(context) => Some(context),
        Err(e) => {
            warn!(error = %e, "ignoring a malformed playback context; progress is not saved");
            None
        }
    }
}

/// What the user chose for this title before (subtitles off, or a subtitle or audio
/// language), so mpv's own track selection follows it from the start.
async fn title_prefs(app: &AppHandle, context: Option<&PlaybackContext>) -> Option<TitlePrefs> {
    let prefs = app.try_state::<PlayerPrefs>()?;
    prefs.get(&context?.meta.id).await.ok().flatten()
}

/// A progress tracker for `context`, if there is one and it is not live TV.
fn progress(app: &AppHandle, context: Option<PlaybackContext>) -> Option<Arc<SessionProgress>> {
    let context = context?;
    app.try_state::<LibraryState>()?.session(context)
}

/// Forwards session `id`'s events to the webview while it is the current session,
/// and forgets the session when it shuts down by itself (e.g. the user closed the
/// external mpv window). Every event also feeds the session's progress tracker, if it
/// has one. Another file the user opened in the player counts as a new file before
/// the webview hears of it.
fn event_sink(app: &AppHandle, id: u64) -> impl Fn(PlayerEvent) + Send + Sync + 'static {
    let app = app.clone();
    move |event| {
        let Some(player) = app.try_state::<PlayerState>() else {
            return;
        };
        if let Some(progress) = player.progress_for(id) {
            progress.on_event(&event);
        }
        if player.current.load(Ordering::SeqCst) != id {
            return; // a replaced session
        }
        player.watch_file(id, &event);
        let shutdown = event == PlayerEvent::Shutdown;
        let _ = app.emit(PLAYER_EVENT, &event);
        if shutdown {
            player.forget(id);
        }
    }
}

fn session_error(error: SessionError) -> IpcError {
    match error {
        SessionError::Closed => IpcError::Player("The player has shut down.".into()),
        SessionError::Mpv(message) => IpcError::Player(message),
    }
}

/// Addon-supplied requests are untrusted: only http(s) is ever handed to mpv, and
/// header names/values, user agent and referrer must not carry line breaks or NULs
/// (they would split HTTP headers or fail inside libmpv); such a request is refused
/// with an error naming the field. The title is display text only (often a
/// multi-line addon stream name such as "Addon\n1080p"), so it is sanitized instead
/// of refused: line breaks and other control characters become single spaces and
/// NULs are dropped.
fn check_request(request: &mut PlaybackRequest) -> Result<(), IpcError> {
    let scheme = request
        .url
        .split("://")
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(IpcError::InvalidInput(format!(
            "refusing to play a '{scheme}' URL (only http/https streams are supported)"
        )));
    }
    let clean = |s: &str| !s.contains(['\r', '\n', '\0']);
    let token = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
    };
    let malformed = |what: &str| {
        Err(IpcError::InvalidInput(format!(
            "The stream's {what} is malformed, so it cannot be played."
        )))
    };
    if !clean(&request.url) {
        return malformed("URL");
    }
    if !request
        .headers
        .iter()
        .all(|(name, value)| token(name) && clean(value))
    {
        return malformed("HTTP header");
    }
    if !request.user_agent.as_deref().is_none_or(clean) {
        return malformed("user agent");
    }
    if !request.referrer.as_deref().is_none_or(clean) {
        return malformed("referrer");
    }
    if let Some(title) = request.title.take() {
        let title = sanitize_title(&title);
        request.title = (!title.is_empty()).then_some(title);
    }
    Ok(())
}

/// One line of display text: NULs dropped, each run of control characters (and the
/// whitespace around it) collapsed to a single space.
fn sanitize_title(title: &str) -> String {
    title
        .replace('\0', "")
        .split(char::is_control)
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(url: &str) -> PlaybackRequest {
        PlaybackRequest {
            url: url.into(),
            ..PlaybackRequest::default()
        }
    }

    #[test]
    fn only_http_urls_are_played() {
        assert!(check_request(&mut request("https://host/v.mkv")).is_ok());
        assert!(check_request(&mut request("HTTP://127.0.0.1:1/x")).is_ok());
        for url in [
            "file:///etc/passwd",
            "ytdl://x",
            "av://lavfi:testsrc",
            "edl://x",
            "",
        ] {
            assert!(
                matches!(
                    check_request(&mut request(url)),
                    Err(IpcError::InvalidInput(_))
                ),
                "{url}"
            );
        }
    }

    #[test]
    fn headers_cannot_smuggle_lines() {
        let mut ok = request("https://host/v");
        ok.headers = vec![("Authorization".into(), "Bearer a, b".into())];
        ok.user_agent = Some("Mozilla/5.0 (X11)".into());
        assert!(check_request(&mut ok.clone()).is_ok());

        let bad_value = PlaybackRequest {
            headers: vec![("X-A".into(), "1\r\nX-Evil: 2".into())],
            ..ok.clone()
        };
        let bad_name = PlaybackRequest {
            headers: vec![("X A:".into(), "1".into())],
            ..ok.clone()
        };
        let empty_name = PlaybackRequest {
            headers: vec![(String::new(), "1".into())],
            ..ok.clone()
        };
        let bad_ua = PlaybackRequest {
            user_agent: Some("ua\nX: y".into()),
            ..ok.clone()
        };
        let bad_referrer = PlaybackRequest {
            referrer: Some("https://a/\r\nX: y".into()),
            ..ok.clone()
        };
        let bad_url = request("https://host/v\nX: y");
        for (mut r, field) in [
            (bad_value, "HTTP header"),
            (bad_name, "HTTP header"),
            (empty_name, "HTTP header"),
            (bad_ua, "user agent"),
            (bad_referrer, "referrer"),
            (bad_url, "URL"),
        ] {
            match check_request(&mut r) {
                Err(IpcError::InvalidInput(message)) => {
                    assert!(message.contains(field), "{message} names {field}");
                }
                other => panic!("{r:?} -> {other:?}"),
            }
        }
    }

    /// Stremio addons put line breaks in stream names ("Addon\n1080p"), which the UI
    /// passes on as the title: it is display text, so it is flattened, not refused.
    #[test]
    fn multi_line_titles_are_flattened_not_refused() {
        for (title, expected) in [
            ("Torrentio\n1080p", Some("Torrentio 1080p")),
            ("MediaFusion \r\n 4K\tHDR", Some("MediaFusion 4K HDR")),
            ("a\0b", Some("ab")),
            ("Plain title", Some("Plain title")),
            ("\n\r\0", None),
        ] {
            let mut r = PlaybackRequest {
                title: Some(title.into()),
                ..request("https://host/v")
            };
            assert!(check_request(&mut r).is_ok(), "{title:?}");
            assert_eq!(r.title.as_deref(), expected, "{title:?}");
        }
        let mut untitled = request("https://host/v");
        assert!(check_request(&mut untitled).is_ok());
        assert_eq!(untitled.title, None);
    }

    #[test]
    fn a_stop_limited_to_a_backend_spares_the_other() {
        let only: Option<Backend> = serde_json::from_value(serde_json::json!("embedded")).unwrap();
        assert_eq!(only, Some(Backend::Embedded));
        assert!(stop_matches(Backend::Embedded, only));
        assert!(!stop_matches(Backend::External, only));
        assert!(stop_matches(Backend::External, Some(Backend::External)));
        assert!(!stop_matches(Backend::Embedded, Some(Backend::External)));
        // no filter: any backend
        assert!(stop_matches(Backend::Embedded, None));
        assert!(stop_matches(Backend::External, None));
        let missing: Option<Backend> = serde_json::from_value(serde_json::Value::Null).unwrap();
        assert_eq!(missing, None);
    }

    /// A subtitle add compares `loads` before and after its download: it moves for each
    /// file of ours and for another file the user opens in the player, never for the
    /// own file's start-file, a redirect, or a replaced session's late events.
    #[test]
    fn another_file_opened_in_the_player_is_a_new_file() {
        let player = PlayerState::default();
        let end = |reason| PlayerEvent::EndFile {
            reason,
            file_error: None,
        };
        let watch = |id, events: &[PlayerEvent]| {
            for event in events {
                player.watch_file(id, event);
            }
        };

        // player_start: session 1 plays its file, which redirects
        player.new_file(1);
        let launched = player.load_count();
        watch(
            1,
            &[
                PlayerEvent::StartFile,
                end("redirect"),
                PlayerEvent::StartFile,
                PlayerEvent::FileLoaded,
            ],
        );
        assert_eq!(player.load_count(), launched, "its own file");

        // the user drops another file into the external mpv window, then a third
        watch(1, &[end("stop"), PlayerEvent::StartFile]);
        assert_eq!(player.load_count(), launched + 1, "another file");
        watch(1, &[end("eof"), PlayerEvent::StartFile]);
        assert_eq!(player.load_count(), launched + 2, "and another");

        // player_load: the loaded file's start-file follows the end of the one before,
        // and is its own
        player.new_file(1);
        let loaded = player.load_count();
        assert_eq!(loaded, launched + 3);
        watch(1, &[end("stop"), PlayerEvent::StartFile]);
        assert_eq!(player.load_count(), loaded, "the loaded file");

        // player_start replaces session 1: its late events change nothing
        player.new_file(2);
        let replaced = player.load_count();
        watch(1, &[end("stop"), PlayerEvent::StartFile]);
        watch(2, &[PlayerEvent::StartFile]);
        assert_eq!(player.load_count(), replaced);
        watch(2, &[end("eof"), PlayerEvent::StartFile]);
        assert_eq!(player.load_count(), replaced + 1);
    }

    #[test]
    fn start_results_serialize_for_the_ui() {
        let value = serde_json::to_value(StartResult {
            backend: Backend::External,
            fallback_reason: Some("no VO".into()),
            remote_control: true,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"backend": "external", "fallbackReason": "no VO", "remoteControl": true})
        );
        let backends = serde_json::to_value(Backends {
            embedded: EmbeddedStatus::unavailable("not here".into()),
            external: ExternalStatus {
                available: false,
                path: None,
            },
        })
        .unwrap();
        assert_eq!(
            backends,
            serde_json::json!({
                "embedded": {"available": false, "reason": "not here"},
                "external": {"available": false, "path": null}
            })
        );
    }

    /// The context is read inside the command, so even a shape error only costs the
    /// library entry; a bad episode number costs only that number.
    #[test]
    fn a_malformed_context_never_refuses_playback() {
        use serde_json::json;
        assert!(checked_context(None).is_none());
        assert!(checked_context(Some(json!("not an object"))).is_none());
        assert!(checked_context(Some(json!({"meta": {"id": 5}, "videoId": "x"}))).is_none());
        assert!(
            checked_context(Some(
                json!({"meta": {"id": "", "type": "movie"}, "videoId": "x"})
            ))
            .is_none()
        );
        let context = checked_context(Some(json!({
            "meta": {"id": "tt9", "type": "series", "name": "A Show",
                     "behaviorHints": {"featuredVideoId": 3, "isLive": "no"}},
            "videoId": "tt9:1:1",
            "videos": [{"id": "tt9:1:1", "season": -1, "episode": 1}]
        })))
        .expect("usable");
        assert_eq!(context.videos[0].season, None);
        assert_eq!(context.meta.behavior_hints.featured_video_id, None);
    }
}
