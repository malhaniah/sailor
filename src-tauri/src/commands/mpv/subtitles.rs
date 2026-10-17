//! Subtitle commands of the player, on whichever backend plays.
//!
//! `sub-add` is never open to the webview (`validate.rs`): a subtitle reaches mpv only
//! through these commands, as a UTF-8 file Zeiler wrote itself under the cache dir
//! (`crate::subtitles::SubtitleStore`). A link is http(s) only and capped at 10 MiB; a
//! local file comes from the OS file dialog, opened here in Rust (no dialog permission
//! is granted to the webview), or from a file dropped on the window; the track title
//! and language are display text and a language code, passed as mpv argv (no shell, no
//! command string). An add that finishes after the video changed (another file of
//! ours, or one the user opened in external mpv) is not applied.
//!
//! - `player_subtitle_add { request: { url, lang?, label?, select?, encoding? } }`
//! - `player_subtitle_reencode { file, encoding? }`: the same subtitle again, read as
//!   another encoding (the UI then removes the old track).
//! - `player_subtitle_pick_local {}`: `null` when the dialog is cancelled.
//! - `player_set_sub_style { style }`: apply style fields live (the UI saves them);
//!   answers with the properties mpv refused.
//! - `player_prefs_get { metaId }` / `player_prefs_set { metaId, patch }`.
//! - `player_video_hash {}`: the OpenSubtitles hash and size of the file playing, for
//!   subtitle addons when the stream names none; read with two range requests against
//!   the stream itself (the URL and headers stay in Rust).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::warn;
use url::Url;
use zeiler_addon::{
    Encoding, HashTarget, MAX_SUBTITLE_BYTES, SubtitleFile, SubtitleFormat, VideoHasher,
};
use zeiler_player::Session;

use super::{PlayerState, sanitize_title, session_error};
use crate::error::IpcError;
use crate::settings_groups::{SubtitleStyle, is_language_code, parse_strict};
use crate::state::AppState;
use crate::subtitles::prefs::{PlayerPrefs, TitlePrefs};
use crate::subtitles::{Added, Origin, SubtitleStore, languages};

/// The event telling the UI how a subtitle file dropped on the window went.
pub const SUBTITLE_DROPPED: &str = "subtitles://dropped";

/// The file extensions the picker offers and a drop accepts.
pub(crate) const SUBTITLE_EXTENSIONS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub"];

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest track title passed to mpv.
const MAX_TITLE_CHARS: usize = 120;

/// What an add did, for the UI: the file's name (the new track is the one whose
/// `external-filename` ends with it), its format and the encoding it was read as.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedSubtitle {
    pub file: String,
    pub format: SubtitleFormat,
    pub encoding: &'static str,
}

/// A subtitle to add from a link.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleRequest {
    pub url: String,
    /// The track's language code.
    pub lang: Option<String>,
    /// The track title.
    pub label: Option<String>,
    /// Select it once added (default: yes).
    pub select: Option<bool>,
    /// An encoding label (`windows-1256`, …) that replaces detection.
    pub encoding: Option<String>,
}

/// Add a subtitle from an http(s) link (a subtitle addon's, or one the stream lists).
#[tauri::command]
pub async fn player_subtitle_add(
    state: State<'_, AppState>,
    player: State<'_, PlayerState>,
    store: State<'_, SubtitleStore>,
    request: SubtitleRequest,
) -> Result<AddedSubtitle, IpcError> {
    let SubtitleRequest {
        url,
        lang,
        label,
        select,
        encoding,
    } = request;
    let url = Url::parse(&url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .ok_or_else(|| {
            IpcError::InvalidInput("Only http and https subtitle links can be loaded.".into())
        })?;
    let encoding = encoding_override(encoding.as_deref())?;
    let target = player.target()?;
    let file = download(&state, &url, encoding).await?;
    let added = Added {
        origin: Origin::Url(url),
        title: label.as_deref().and_then(clean_title),
        lang: lang.filter(|l| is_language_code(l)),
    };
    add(
        &player,
        &store,
        &target,
        &file,
        added,
        select.unwrap_or(true),
    )
    .await
}

/// Add an already added subtitle (`file`, as an add returned it) again, read as
/// `encoding` (none: detect), and select it.
#[tauri::command]
pub async fn player_subtitle_reencode(
    state: State<'_, AppState>,
    player: State<'_, PlayerState>,
    store: State<'_, SubtitleStore>,
    file: String,
    encoding: Option<String>,
) -> Result<AddedSubtitle, IpcError> {
    let added = store
        .added(&file)
        .ok_or_else(|| IpcError::NotFound("That subtitle is no longer loaded.".into()))?;
    let encoding = encoding_override(encoding.as_deref())?;
    let target = player.target()?;
    let subtitle = match &added.origin {
        Origin::Url(url) => download(&state, url, encoding).await?,
        Origin::Local(path) => read_local(path, encoding).await?,
    };
    add(&player, &store, &target, &subtitle, added, true).await
}

/// Let the user pick a subtitle file (the OS dialog, over Zeiler's window) and add it,
/// selected. `None` when the dialog was cancelled.
#[tauri::command]
pub async fn player_subtitle_pick_local(
    app: AppHandle,
    player: State<'_, PlayerState>,
    store: State<'_, SubtitleStore>,
) -> Result<Option<AddedSubtitle>, IpcError> {
    player.target()?; // nothing plays: say so before a dialog opens
    let Some(path) = pick(&app).await? else {
        return Ok(None);
    };
    add_local(&player, &store, path).await.map(Some)
}

/// Apply subtitle style fields (the `subtitles.style` settings shape; unnamed fields
/// are left as they are) to the running player. The UI saves them separately. Returns
/// the mpv properties the player refused (an older external mpv lacks some, e.g.
/// `sub-border-style` before 0.38), so the UI can say so.
#[tauri::command]
pub async fn player_set_sub_style(
    player: State<'_, PlayerState>,
    style: serde_json::Value,
) -> Result<Vec<&'static str>, IpcError> {
    let style: SubtitleStyle = parse_strict(&style)
        .map_err(|e| IpcError::InvalidInput(format!("That subtitle style is not valid: {e}.")))?;
    Ok(apply_style(&*player.session()?, &style).await)
}

/// The OpenSubtitles hash of a video, as the subtitle addons take it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoHashResult {
    pub hash: String,
    pub size: u64,
}

/// The OpenSubtitles hash and size of the file playing now (`None`: nothing of ours
/// plays, the server does not serve ranges, or the file changed meanwhile). Never an
/// error: subtitles are then asked for without it.
#[tauri::command]
pub async fn player_video_hash(
    player: State<'_, PlayerState>,
    hasher: State<'_, VideoHasher>,
) -> Result<Option<VideoHashResult>, IpcError> {
    let Some((loads, request)) = player.playing_request() else {
        return Ok(None);
    };
    let target = HashTarget {
        url: request.url,
        headers: request.headers,
        user_agent: request.user_agent,
        referrer: request.referrer,
    };
    match hasher.hash(&target).await {
        Ok(hashed) if player.load_count() == loads => Ok(Some(VideoHashResult {
            hash: hashed.hash,
            size: hashed.size,
        })),
        Ok(_) => Ok(None),
        Err(e) => {
            // (the error never names the URL, which may carry credentials)
            warn!(error = %e, "could not hash the video for subtitles");
            Ok(None)
        }
    }
}

/// The choices remembered for a title (`None`: nothing yet).
#[tauri::command]
pub async fn player_prefs_get(
    prefs: State<'_, PlayerPrefs>,
    meta_id: String,
) -> Result<Option<TitlePrefs>, IpcError> {
    prefs.get(&meta_id).await
}

/// Remember choices for a title (`subLang`, `subSource`, `audioLang`, `subDelaySec`,
/// `rate`; `null` forgets one).
#[tauri::command]
pub async fn player_prefs_set(
    prefs: State<'_, PlayerPrefs>,
    meta_id: String,
    patch: serde_json::Value,
) -> Result<TitlePrefs, IpcError> {
    prefs.set(&meta_id, patch).await
}

/// A subtitle file was dropped on the window: add the first one with a subtitle
/// extension to the running player, and tell the UI how it went. Other drops, and
/// drops while nothing plays, are ignored.
pub fn on_drop(app: &AppHandle, paths: &[PathBuf]) {
    let Some(path) = paths.iter().find(|p| has_subtitle_extension(p)).cloned() else {
        return;
    };
    let Some(player) = app.try_state::<PlayerState>() else {
        return;
    };
    if player.target().is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (Some(player), Some(store)) = (
            app.try_state::<PlayerState>(),
            app.try_state::<SubtitleStore>(),
        ) else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let outcome = match add_local(&player, &store, path).await {
            Ok(added) => serde_json::json!({ "name": name, "added": added }),
            Err(e) => serde_json::json!({ "name": name, "error": e.to_string() }),
        };
        let _ = app.emit(SUBTITLE_DROPPED, outcome);
    });
}

/// Apply a style's fields to `session`, trying each field's property names in turn
/// (mpv 0.38 renamed `sub-border-*` to `sub-outline-*`). Best effort: a refusal is
/// logged, never fatal; the properties mpv refused are returned. Only external mpv
/// reports refusals (it can be any version): the built-in core takes property writes
/// without waiting, and it is the pinned libmpv, which has every property here.
pub(crate) async fn apply_style(session: &Session, style: &SubtitleStyle) -> Vec<&'static str> {
    let mut refusals = Vec::new();
    for (names, value) in style.properties() {
        let mut refused = None;
        for name in names {
            match session.set_property(name, value.clone()).await {
                Ok(()) => {
                    refused = None;
                    break;
                }
                Err(e) => refused = Some(e),
            }
        }
        if let Some(e) = refused {
            warn!(property = names[0], error = %e, "mpv refused a subtitle style");
            refusals.push(names[0]);
        }
    }
    refusals
}

// --- the shared path ------------------------------------------------------------------------

/// The session an add goes to, and which file it plays then.
pub(crate) struct Target {
    session: Arc<Session>,
    id: u64,
    load: u64,
}

impl PlayerState {
    /// The running session and its current file; refused when nothing plays.
    pub(crate) fn target(&self) -> Result<Target, IpcError> {
        let active = self
            .active()
            .ok_or_else(|| IpcError::Player("Nothing is playing.".into()))?;
        Ok(Target {
            session: active.session,
            id: active.id,
            load: self.load_count(),
        })
    }

    /// Whether `target` still plays the same file.
    fn still(&self, target: &Target) -> bool {
        self.active().is_some_and(|a| a.id == target.id) && self.load_count() == target.load
    }
}

async fn add(
    player: &PlayerState,
    store: &SubtitleStore,
    target: &Target,
    file: &SubtitleFile,
    added: Added,
    select: bool,
) -> Result<AddedSubtitle, IpcError> {
    let (name, path) = store.write(file, added.clone()).await?;
    if !player.still(target) {
        return Err(IpcError::Player(
            "The video changed before the subtitle was ready.".into(),
        ));
    }
    let mut args = vec![
        "sub-add".to_string(),
        path.to_string_lossy().into_owned(),
        if select { "select" } else { "auto" }.to_string(),
    ];
    // title and language are positional: a language needs a title in front of it
    if added.title.is_some() || added.lang.is_some() {
        args.push(added.title.clone().unwrap_or_default());
    }
    if let Some(lang) = added.lang {
        args.push(lang);
    }
    target
        .session
        .command_checked(args)
        .await
        .map_err(session_error)?;
    Ok(AddedSubtitle {
        file: name,
        format: file.format,
        encoding: file.encoding,
    })
}

async fn add_local(
    player: &PlayerState,
    store: &SubtitleStore,
    path: PathBuf,
) -> Result<AddedSubtitle, IpcError> {
    let target = player.target()?;
    let file = read_local(&path, None).await?;
    let added = Added {
        title: path
            .file_stem()
            .and_then(|s| clean_title(&s.to_string_lossy())),
        lang: language_from_file_name(&path),
        origin: Origin::Local(path),
    };
    add(player, store, &target, &file, added, true).await
}

async fn download(
    state: &AppState,
    url: &Url,
    encoding: Option<&'static Encoding>,
) -> Result<SubtitleFile, IpcError> {
    match tokio::time::timeout(
        DOWNLOAD_TIMEOUT,
        zeiler_addon::fetch_subtitle(&state.client, url, encoding),
    )
    .await
    {
        Ok(Ok(file)) => Ok(file),
        Ok(Err(e)) => Err(IpcError::Addon(e.to_string())),
        Err(_) => Err(IpcError::Network(
            "The subtitle took too long to download.".into(),
        )),
    }
}

async fn read_local(
    path: &Path,
    encoding: Option<&'static Encoding>,
) -> Result<SubtitleFile, IpcError> {
    use tokio::io::AsyncReadExt;
    let unreadable =
        |e: std::io::Error| IpcError::Storage(format!("The subtitle file could not be read: {e}"));
    let too_large = || IpcError::InvalidInput(zeiler_addon::SubtitleError::TooLarge.to_string());
    // one open: a regular file (not a device or a pipe), read no further than the cap
    let file = tokio::fs::File::open(path).await.map_err(unreadable)?;
    let metadata = file.metadata().await.map_err(unreadable)?;
    if !metadata.is_file() {
        return Err(IpcError::InvalidInput(
            "That is not a subtitle file.".into(),
        ));
    }
    if metadata.len() > MAX_SUBTITLE_BYTES as u64 {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(MAX_SUBTITLE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(unreadable)?;
    if bytes.len() > MAX_SUBTITLE_BYTES {
        return Err(too_large());
    }
    zeiler_addon::decode_subtitle(&bytes, encoding)
        .map_err(|e| IpcError::InvalidInput(e.to_string()))
}

#[cfg(desktop)]
async fn pick(app: &AppHandle) -> Result<Option<PathBuf>, IpcError> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Load a subtitle file")
        .add_filter("Subtitles", SUBTITLE_EXTENSIONS);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    Ok(rx.await.ok().flatten().and_then(|p| p.into_path().ok()))
}

#[cfg(not(desktop))]
async fn pick(_app: &AppHandle) -> Result<Option<PathBuf>, IpcError> {
    Err(IpcError::Unsupported(
        "Picking a subtitle file is not available here.".into(),
    ))
}

/// An encoding label (`windows-1256`, `utf-8`, `shift_jis`, …) as encoding_rs knows it;
/// none or an empty label means "detect".
fn encoding_override(label: Option<&str>) -> Result<Option<&'static Encoding>, IpcError> {
    match label.map(str::trim).filter(|l| !l.is_empty()) {
        None => Ok(None),
        Some(label) => Encoding::for_label(label.as_bytes())
            .map(Some)
            .ok_or_else(|| {
                IpcError::InvalidInput(format!("\"{label}\" is not a text encoding Zeiler knows."))
            }),
    }
}

/// A track title: one line of display text, at most [`MAX_TITLE_CHARS`] characters.
fn clean_title(title: &str) -> Option<String> {
    let title: String = sanitize_title(title)
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    (!title.is_empty()).then_some(title)
}

fn has_subtitle_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        SUBTITLE_EXTENSIONS
            .iter()
            .any(|s| e.eq_ignore_ascii_case(s))
    })
}

/// The language a subtitle file's name ends with (`Film.2019.en.srt`,
/// `Show.S01E01.pt-BR.forced.srt`), if any.
fn language_from_file_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let mut parts = stem.rsplit('.');
    let mut last = parts.next()?;
    if ["forced", "sdh", "cc"].contains(&last.to_ascii_lowercase().as_str()) {
        last = parts.next()?;
    }
    (stem.contains('.') && is_language_code(last) && languages::is_known(last))
        .then(|| last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_are_named_by_their_labels() {
        assert_eq!(encoding_override(None).unwrap(), None);
        assert_eq!(encoding_override(Some(" ")).unwrap(), None);
        for (label, name) in [
            ("windows-1256", "windows-1256"),
            ("cp1251", "windows-1251"),
            ("UTF-8", "UTF-8"),
            ("latin1", "windows-1252"),
            ("shift_jis", "Shift_JIS"),
            ("gb18030", "gb18030"),
            ("big5", "Big5"),
            ("euc-kr", "EUC-KR"),
            ("iso-8859-8", "ISO-8859-8"),
        ] {
            assert_eq!(
                encoding_override(Some(label)).unwrap().unwrap().name(),
                name
            );
        }
        assert!(matches!(
            encoding_override(Some("klingon")),
            Err(IpcError::InvalidInput(_))
        ));
    }

    #[test]
    fn titles_are_one_short_line() {
        assert_eq!(clean_title("English\nSDH").as_deref(), Some("English SDH"));
        assert_eq!(clean_title(" \n\0 "), None);
        assert_eq!(
            clean_title(&"x".repeat(500)).unwrap().len(),
            MAX_TITLE_CHARS
        );
    }

    #[test]
    fn subtitle_files_are_known_by_extension_and_name() {
        assert!(has_subtitle_extension(Path::new("a/Film.SRT")));
        assert!(has_subtitle_extension(Path::new("Film.ass")));
        assert!(!has_subtitle_extension(Path::new("Film.mkv")));
        assert!(!has_subtitle_extension(Path::new("srt")));
        let lang = |name: &str| language_from_file_name(Path::new(name));
        assert_eq!(lang("Film.2019.en.srt").as_deref(), Some("en"));
        assert_eq!(
            lang("Show.S01E01.pt-BR.forced.srt").as_deref(),
            Some("pt-BR")
        );
        assert_eq!(lang("Film.ara.srt").as_deref(), Some("ara"));
        assert_eq!(lang("Film.1080p.srt"), None);
        assert_eq!(lang("Film.2019.web.srt"), None, "not a language");
        assert_eq!(lang("en.srt"), None, "a bare name is not a language");
        assert_eq!(lang("Film.srt"), None);
    }
}
