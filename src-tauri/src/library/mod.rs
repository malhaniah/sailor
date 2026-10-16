//! The local library: Stremio-compatible library items (`zeiler-account`) kept in
//! `library.json` under the profile dir, the views the UI lists, the user's own
//! changes (add/remove, watched marks, dismiss from Continue Watching) and the
//! progress the player reports ([`progress`]).
//!
//! Every change goes through [`JsonStore::try_update`]: the item is changed on a copy,
//! committed only when it differs from before (`zeiler_account::touch` stamps `_mtime`),
//! persisted, and only then announced as `library://changed` with the ids that changed.
//! Player progress is applied by one worker task in the order the player produced it,
//! so a session's final write can never be overtaken by an earlier one.

pub mod progress;

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tracing::warn;
use zeiler_account::{
    LibraryFilter, LibraryItem, LibrarySort, MetaSummary, PosterShape, ResumePoint, VideoRef,
    format_time, is_in_continue_watching, mark_season_watched, mark_videos_watched, mark_watched,
    new_item_from_meta, refresh_from_meta, resume_point, select, set_in_library, touch,
    watched_video_ids,
};

use self::progress::{Flush, PlaybackContext, SessionProgress};
use crate::error::IpcError;
use crate::persist::JsonStore;

/// Emitted with `{ ids }` (the library item ids that changed) after every committed
/// change, whoever made it.
pub const LIBRARY_CHANGED: &str = "library://changed";

const FILE_NAME: &str = "library.json";
const VERSION: u32 = 1;

/// Input limits: ids and names come from addons, through the webview.
const MAX_ID_LEN: usize = 512;
const MAX_TYPE_LEN: usize = 64;
const MAX_NAME_LEN: usize = 1024;
const MAX_VIDEOS: usize = 20_000;

/// `library.json`: one bucket of items, keyed by id, like stremio-core's
/// `LibraryBucket`. `uid` is the Stremio account the items belong to (always `null`
/// until account sync exists), so libraries of different accounts never mix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibraryFile {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default, deserialize_with = "lenient_uid")]
    pub uid: Option<String>,
    #[serde(default, deserialize_with = "lenient_items")]
    pub items: BTreeMap<String, LibraryItem>,
}

impl Default for LibraryFile {
    fn default() -> Self {
        Self {
            version: VERSION,
            uid: None,
            items: BTreeMap::new(),
        }
    }
}

fn current_version() -> u32 {
    VERSION
}

fn lenient_uid<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) if !s.is_empty() => Some(s),
        _ => None,
    })
}

/// Items are read one by one: an item that does not parse (or has no id) is skipped
/// with a warning instead of failing the file, which would move the whole library
/// aside. Each item is keyed by its own id.
fn lenient_items<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, LibraryItem>, D::Error> {
    let Value::Object(entries) = Value::deserialize(d)? else {
        warn!("library.json: `items` is not an object; starting from an empty library");
        return Ok(BTreeMap::new());
    };
    let mut items = BTreeMap::new();
    let mut skipped = 0usize;
    for (_, value) in entries {
        match serde_json::from_value::<LibraryItem>(value) {
            Ok(item) if !item.id.is_empty() => {
                items.insert(item.id.clone(), item);
            }
            _ => skipped += 1,
        }
    }
    if skipped > 0 {
        warn!(
            skipped,
            "library.json: skipped items that could not be read"
        );
    }
    Ok(items)
}

// --- views -----------------------------------------------------------------------------

/// One library item as the UI shows it (the stored item keeps Stremio's wire shape;
/// this is the stable, camelCase view of it). Times are RFC 3339, positions in ms.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub id: String,
    pub r#type: String,
    pub name: String,
    pub poster: Option<String>,
    pub poster_shape: PosterShape,
    /// In the user's library (added, not removed).
    pub in_library: bool,
    /// Shown in Continue Watching.
    pub in_continue_watching: bool,
    pub added_at: Option<String>,
    /// Last change of any kind (`_mtime`); Continue Watching sorts by it.
    pub updated_at: String,
    /// Last time it was played or marked watched.
    pub last_watched: Option<String>,
    /// The video played last, or the next episode Continue Watching moved on to.
    pub video_id: Option<String>,
    /// Resume position in `video_id`, ms (0: none; 1: "from the beginning").
    pub time_offset: u64,
    /// Duration of `video_id`, ms (0: unknown).
    pub duration: u64,
    pub times_watched: u32,
    /// Watched at least once.
    pub watched: bool,
    /// Where to resume `video_id`, when there is a real position to resume.
    pub resume: Option<ResumePoint>,
}

impl From<&LibraryItem> for LibraryEntry {
    fn from(item: &LibraryItem) -> Self {
        Self {
            id: item.id.clone(),
            r#type: item.r#type.clone(),
            name: item.name.clone(),
            // stored items can come from anywhere later (sync): only http(s) images
            poster: item
                .poster
                .as_ref()
                .filter(|url| matches!(url.scheme(), "http" | "https"))
                .map(|url| url.to_string()),
            poster_shape: item.poster_shape,
            in_library: !item.removed,
            in_continue_watching: is_in_continue_watching(item),
            added_at: item.ctime.as_ref().map(format_time),
            updated_at: format_time(&item.mtime),
            last_watched: item.state.last_watched.as_ref().map(format_time),
            video_id: item.state.video_id.clone(),
            time_offset: item.state.time_offset,
            duration: item.state.duration,
            times_watched: item.state.times_watched,
            watched: item.watched(),
            resume: resume_point(item, None),
        }
    }
}

/// One item with the watched marks of its videos (the Detail view).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDetail {
    pub entry: LibraryEntry,
    /// Ids of the given videos that are marked watched.
    pub watched_video_ids: Vec<String>,
}

impl LibraryDetail {
    fn new(item: &LibraryItem, videos: &[VideoRef]) -> Self {
        Self {
            entry: LibraryEntry::from(item),
            watched_video_ids: watched_video_ids(item, videos),
        }
    }
}

/// `library_list`'s query.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryQuery {
    pub filter: LibraryFilter,
    /// Applies to the `library` filter; the other views have their own order.
    #[serde(default)]
    pub sort: LibrarySort,
    /// Only items of this type.
    #[serde(default)]
    pub r#type: Option<String>,
    /// At most this many (the first ones in order).
    #[serde(default)]
    pub limit: Option<usize>,
}

// --- input checks ----------------------------------------------------------------------

fn check_text(what: &str, value: &str, max: usize, required: bool) -> Result<(), IpcError> {
    if required && value.is_empty() {
        return Err(IpcError::InvalidInput(format!("The {what} is missing.")));
    }
    if value.len() > max || value.chars().any(char::is_control) {
        return Err(IpcError::InvalidInput(format!("The {what} is malformed.")));
    }
    Ok(())
}

pub(crate) fn check_id(id: &str) -> Result<(), IpcError> {
    check_text("title id", id, MAX_ID_LEN, true)
}

pub(crate) fn check_video_id(id: &str) -> Result<(), IpcError> {
    check_text("video id", id, MAX_ID_LEN, true)
}

pub(crate) fn check_meta(meta: &MetaSummary) -> Result<(), IpcError> {
    check_id(&meta.id)?;
    check_text("title type", &meta.r#type, MAX_TYPE_LEN, true)?;
    check_text("title name", &meta.name, MAX_NAME_LEN, false)?;
    for id in [
        &meta.behavior_hints.default_video_id,
        &meta.behavior_hints.featured_video_id,
    ]
    .into_iter()
    .flatten()
    {
        check_video_id(id)?;
    }
    Ok(())
}

pub(crate) fn check_videos(videos: &[VideoRef]) -> Result<(), IpcError> {
    if videos.len() > MAX_VIDEOS {
        return Err(IpcError::InvalidInput(
            "The title has too many videos to keep track of.".into(),
        ));
    }
    videos
        .iter()
        .try_for_each(|video| check_video_id(&video.id))
}

// --- state -----------------------------------------------------------------------------

/// A batch of progress from one playback session, applied by the worker in order.
pub(crate) struct Job {
    pub context: Arc<PlaybackContext>,
    pub flush: Flush,
    /// Set once one of the session's batches is in the store. Until then each batch is
    /// applied as the session's first, so a first batch that failed to save does not
    /// make the stale-batch guard drop the rest of the session.
    pub committed: Arc<AtomicBool>,
    /// Answered once the job is applied (the exit path waits for it).
    pub done: Option<oneshot::Sender<()>>,
}

/// A failed progress write is tried once more after this long: on Windows a virus
/// scanner, indexer or backup tool briefly holding the file is the usual cause.
const RETRY_AFTER: Duration = Duration::from_millis(250);

type OnChange = Arc<dyn Fn(Vec<String>) + Send + Sync>;

/// The library store, registered with `app.manage` (see `lib.rs`).
pub struct LibraryState {
    store: Arc<JsonStore<LibraryFile>>,
    jobs: mpsc::UnboundedSender<Job>,
    /// Progress batches sent and not yet applied (a profile switch waits for none).
    pending: Arc<AtomicUsize>,
    on_change: OnChange,
}

/// The current time as the library stores it (millisecond precision).
pub(crate) fn now() -> DateTime<Utc> {
    zeiler_account::now_from_system(std::time::SystemTime::now())
}

impl LibraryState {
    /// Load `library.json` from `dir` (the profile dir) and start the progress worker.
    /// `on_change` is told the ids of every committed change (the shell emits
    /// [`LIBRARY_CHANGED`]).
    pub async fn load(
        dir: &Path,
        on_change: impl Fn(Vec<String>) + Send + Sync + 'static,
    ) -> anyhow::Result<Self> {
        let store = Arc::new(JsonStore::<LibraryFile>::load(dir.join(FILE_NAME)).await?);
        let on_change: OnChange = Arc::new(on_change);
        let (jobs, mut queue) = mpsc::unbounded_channel::<Job>();
        let worker_store = store.clone();
        let worker_change = on_change.clone();
        let pending = Arc::new(AtomicUsize::new(0));
        let worker_pending = pending.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(job) = queue.recv().await {
                let mut flush = job.flush;
                flush.first |= !job.committed.load(Ordering::SeqCst);
                let apply = || {
                    worker_store
                        .try_update(|file| Ok(progress::apply(file, &job.context, &flush, now())))
                };
                let mut applied = apply().await;
                if applied.is_err() {
                    tokio::time::sleep(RETRY_AFTER).await;
                    applied = apply().await;
                }
                match applied {
                    Ok(changed) => {
                        job.committed.store(true, Ordering::SeqCst);
                        if let Some(id) = changed {
                            worker_change(vec![id]);
                        }
                    }
                    Err(e) => warn!(error = %e, "could not save playback progress"),
                }
                worker_pending.fetch_sub(1, Ordering::SeqCst);
                if let Some(done) = job.done {
                    let _ = done.send(());
                }
            }
        });
        Ok(Self {
            store,
            jobs,
            pending,
            on_change,
        })
    }

    /// Read another profile's `library.json` from now on (no player may be running).
    pub(crate) async fn switch_to(&self, dir: &Path) -> anyhow::Result<()> {
        self.store.switch_to(dir.join(FILE_NAME)).await
    }

    /// Wait (up to `limit`) until every progress batch sent so far is applied, so a
    /// profile switch never lands one in the next profile's library. False on timeout.
    pub(crate) async fn settle(&self, limit: Duration) -> bool {
        let until = tokio::time::Instant::now() + limit;
        while self.pending.load(Ordering::SeqCst) > 0 {
            if tokio::time::Instant::now() >= until {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        true
    }

    /// Progress tracking for a playback, or none for live TV (never tracked).
    pub(crate) fn session(&self, context: PlaybackContext) -> Option<Arc<SessionProgress>> {
        SessionProgress::new(context, self.jobs.clone(), self.pending.clone())
    }

    pub async fn list(&self, query: &LibraryQuery) -> Vec<LibraryEntry> {
        self.store
            .read(|file| {
                select(
                    file.items.values(),
                    query.filter,
                    query.sort,
                    query.r#type.as_deref(),
                )
                .into_iter()
                .take(query.limit.unwrap_or(usize::MAX))
                .map(LibraryEntry::from)
                .collect()
            })
            .await
    }

    pub async fn get(&self, id: &str, videos: &[VideoRef]) -> Option<LibraryDetail> {
        self.store
            .read(|file| {
                file.items
                    .get(id)
                    .map(|item| LibraryDetail::new(item, videos))
            })
            .await
    }

    /// Add `meta` to the library, or remove it. Removing a title that was never in the
    /// library changes nothing (`None`).
    pub async fn set_in_library(
        &self,
        meta: &MetaSummary,
        in_library: bool,
    ) -> Result<Option<LibraryEntry>, IpcError> {
        check_meta(meta)?;
        let now = now();
        self.change(&meta.id, |previous| {
            let mut item = match previous {
                Some(previous) => {
                    let mut item = previous.clone();
                    refresh(&mut item, meta);
                    item
                }
                None if in_library => new_item_from_meta(meta, now),
                None => return None,
            };
            set_in_library(&mut item, in_library);
            Some(item)
        })
        .await
        .map(|item| item.as_ref().map(LibraryEntry::from))
    }

    /// Mark the whole title (`video_ids` empty, no `season`), some of its videos, or one
    /// season watched or not. `videos` is the title's full video list (the watched
    /// bitfield is aligned to it). Marking the video Continue Watching points at moves it
    /// on, as Stremio's apps do (`zeiler_account::mark_videos_watched`,
    /// `mark_season_watched`). Unmarking a title that is not in the library changes
    /// nothing (`None`).
    pub async fn mark_watched(
        &self,
        meta: &MetaSummary,
        videos: &[VideoRef],
        video_ids: &[String],
        season: Option<u32>,
        watched: bool,
    ) -> Result<Option<LibraryDetail>, IpcError> {
        check_meta(meta)?;
        check_videos(videos)?;
        if let Some(season) = season {
            if !video_ids.is_empty() {
                return Err(IpcError::InvalidInput(
                    "Mark either a season or some episodes, not both.".into(),
                ));
            }
            if !videos
                .iter()
                .any(|v| v.season == Some(season) && v.episode.is_some())
            {
                return Err(IpcError::InvalidInput(
                    "That season is not part of this title.".into(),
                ));
            }
        }
        if video_ids.len() > MAX_VIDEOS {
            return Err(IpcError::InvalidInput(
                "Too many videos to mark at once.".into(),
            ));
        }
        let known: HashSet<&str> = videos.iter().map(|v| v.id.as_str()).collect();
        let mut marked = Vec::with_capacity(video_ids.len());
        let mut seen = HashSet::new();
        for id in video_ids {
            check_video_id(id)?;
            if !known.contains(id.as_str()) {
                return Err(IpcError::InvalidInput(
                    "That episode is not part of this title.".into(),
                ));
            }
            if seen.insert(id.as_str()) {
                marked.push(id.as_str());
            }
        }
        let now = now();
        let changed = self
            .change(&meta.id, |previous| {
                let mut item = match previous {
                    Some(previous) => {
                        let mut item = previous.clone();
                        refresh(&mut item, meta);
                        item
                    }
                    None if watched => new_item_from_meta(meta, now),
                    None => return None,
                };
                match season {
                    Some(season) => {
                        mark_season_watched(&mut item, videos, season, watched, now);
                    }
                    None if marked.is_empty() => mark_watched(&mut item, watched, now),
                    None => {
                        mark_videos_watched(&mut item, videos, &marked, watched, now);
                    }
                }
                Some(item)
            })
            .await?;
        Ok(changed.map(|item| LibraryDetail::new(&item, videos)))
    }

    /// Every item and the account the library belongs to, for account sync.
    pub(crate) async fn sync_snapshot(&self) -> (Option<String>, BTreeMap<String, LibraryItem>) {
        self.store
            .read(|file| (file.uid.clone(), file.items.clone()))
            .await
    }

    /// Take items pulled from the account `uid` (each only if at least as new as the
    /// local copy and different from it) and mark the library as that account's.
    /// Returns the ids that changed; listeners hear about them.
    pub(crate) async fn merge_remote(
        &self,
        uid: &str,
        pulled: Vec<LibraryItem>,
    ) -> Result<Vec<String>, IpcError> {
        let pulled: Vec<LibraryItem> = pulled
            .into_iter()
            .filter(|i| check_id(&i.id).is_ok())
            .collect();
        let changed = self
            .store
            .try_update(|file| {
                let changed = zeiler_sync::sync::merge_pulled(&mut file.items, pulled);
                let new_uid = file.uid.as_deref() != Some(uid);
                file.uid = Some(uid.to_string());
                Ok((new_uid || !changed.is_empty()).then_some(changed))
            })
            .await?
            .unwrap_or_default();
        if !changed.is_empty() {
            (self.on_change)(changed.clone());
        }
        Ok(changed)
    }

    /// The library is about to sync with account `uid`. When it belongs to another
    /// account, that library is kept beside it (`library-<uid>.json`) and this one
    /// starts empty for `uid`, so two accounts' libraries never mix (stremio-core
    /// keeps one bucket per user the same way). A library of no account (used before
    /// signing in) is kept and merged. Returns whether it started over.
    pub(crate) async fn prepare_for_account(&self, uid: &str) -> Result<bool, IpcError> {
        let path = self.store.path();
        let started_over = self
            .store
            .try_update(|file| {
                let Some(previous) = file.uid.clone().filter(|p| p != uid) else {
                    return Ok(None);
                };
                let backup = path.with_file_name(format!(
                    "library-{}.json",
                    previous.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                ));
                let json = serde_json::to_vec_pretty(&*file)
                    .map_err(|e| IpcError::Storage(format!("Could not keep the library: {e}")))?;
                std::fs::write(&backup, json)
                    .map_err(|e| IpcError::Storage(format!("Could not keep the library: {e}")))?;
                file.items.clear();
                file.uid = Some(uid.to_string());
                Ok(Some(()))
            })
            .await?
            .is_some();
        if started_over {
            (self.on_change)(Vec::new());
        }
        Ok(started_over)
    }

    /// Take the title out of Continue Watching (its resume position is dropped).
    pub async fn clear_progress(&self, id: &str) -> Result<Option<LibraryEntry>, IpcError> {
        check_id(id)?;
        self.change(id, |previous| {
            let mut item = previous?.clone();
            zeiler_account::clear_progress(&mut item);
            Some(item)
        })
        .await
        .map(|item| item.as_ref().map(LibraryEntry::from))
    }

    /// The one write path for user changes: `f` gets the stored item (if any) and
    /// returns the next one (`None`: leave it). The item is written, and `on_change`
    /// told, only when it really changed; either way the item as it now stands is
    /// returned.
    async fn change(
        &self,
        id: &str,
        f: impl FnOnce(Option<&LibraryItem>) -> Option<LibraryItem>,
    ) -> Result<Option<LibraryItem>, IpcError> {
        let now = now();
        let mut unchanged = None;
        let written = self
            .store
            .try_update(|file| {
                let previous = file.items.get(id);
                let Some(mut item) = f(previous) else {
                    unchanged = previous.cloned();
                    return Ok(None);
                };
                if !touch(previous, &mut item, now) {
                    unchanged = Some(item);
                    return Ok(None);
                }
                file.items.insert(id.to_string(), item.clone());
                Ok(Some(item))
            })
            .await?;
        if written.is_some() {
            (self.on_change)(vec![id.to_string()]);
        }
        Ok(written.or(unchanged))
    }
}

/// Refresh an item's meta fields from what the UI sent, keeping a known poster when
/// the meta came without one.
pub(crate) fn refresh(item: &mut LibraryItem, meta: &MetaSummary) {
    let poster = item.poster.clone();
    refresh_from_meta(item, meta);
    if item.poster.is_none() {
        item.poster = poster;
    }
}

/// The JSON object `library://changed` carries.
#[derive(Clone, Debug, Serialize)]
pub struct LibraryChanged {
    pub ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    use crate::persist::tests::temp_dir;

    fn meta(id: &str, kind: &str) -> MetaSummary {
        serde_json::from_value(json!({
            "id": id, "type": kind, "name": format!("Title {id}"),
            "poster": format!("https://img.example/{id}.jpg")
        }))
        .unwrap()
    }

    fn episodes(n: u32) -> Vec<VideoRef> {
        (1..=n)
            .map(|e| VideoRef {
                id: format!("tt9:1:{e}"),
                season: Some(1),
                episode: Some(e),
                released: None,
            })
            .collect()
    }

    async fn library(test: &str) -> (LibraryState, Arc<Mutex<Vec<String>>>, std::path::PathBuf) {
        let dir = temp_dir(test);
        let changes = Arc::new(Mutex::new(Vec::new()));
        let sink = changes.clone();
        let state = LibraryState::load(&dir, move |ids| sink.lock().unwrap().extend(ids))
            .await
            .unwrap();
        (state, changes, dir)
    }

    #[tokio::test]
    async fn add_then_remove_persists_and_announces_each_change_once() {
        let (library, changes, dir) = library("library-add-remove").await;
        let movie = meta("tt1", "movie");

        let added = library.set_in_library(&movie, true).await.unwrap().unwrap();
        assert!(added.in_library);
        assert_eq!(added.poster.as_deref(), Some("https://img.example/tt1.jpg"));
        // adding again is not a change
        library.set_in_library(&movie, true).await.unwrap();
        assert_eq!(*changes.lock().unwrap(), ["tt1"]);

        let on_disk: LibraryFile =
            serde_json::from_slice(&std::fs::read(dir.join("library.json")).unwrap()).unwrap();
        assert_eq!(on_disk.version, 1);
        assert_eq!(on_disk.uid, None);
        assert!(!on_disk.items["tt1"].removed);

        let removed = library
            .set_in_library(&movie, false)
            .await
            .unwrap()
            .unwrap();
        assert!(!removed.in_library);
        assert_eq!(changes.lock().unwrap().len(), 2);
        let listed = library
            .list(&LibraryQuery {
                filter: LibraryFilter::Library,
                sort: LibrarySort::Name,
                r#type: None,
                limit: None,
            })
            .await;
        assert!(listed.is_empty());

        // removing something never added creates nothing
        assert_eq!(
            library
                .set_in_library(&meta("tt2", "movie"), false)
                .await
                .unwrap(),
            None
        );
        assert!(library.get("tt2", &[]).await.is_none());
    }

    #[tokio::test]
    async fn episode_marks_round_trip_and_unknown_episodes_are_refused() {
        let (library, _, _) = library("library-marks").await;
        let show = meta("tt9", "series");
        let videos = episodes(5);

        let detail = library
            .mark_watched(
                &show,
                &videos,
                &["tt9:1:2".into(), "tt9:1:4".into()],
                None,
                true,
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail.watched_video_ids, ["tt9:1:2", "tt9:1:4"]);
        assert!(
            !detail.entry.in_library,
            "marking does not add to the library"
        );

        let detail = library
            .mark_watched(&show, &videos, &["tt9:1:4".into()], None, false)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail.watched_video_ids, ["tt9:1:2"]);
        assert_eq!(
            library.get("tt9", &videos).await.unwrap().watched_video_ids,
            ["tt9:1:2"]
        );

        let refused = library
            .mark_watched(&show, &videos, &["tt9:7:1".into()], None, true)
            .await;
        assert!(matches!(refused, Err(IpcError::InvalidInput(_))));

        // unmarking a title that was never stored stores nothing
        assert_eq!(
            library
                .mark_watched(&meta("tt5", "movie"), &[], &[], None, false)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn season_marks_cover_the_season_and_refuse_bad_requests() {
        let (library, _, _) = library("library-mark-season").await;
        let show = meta("tt9", "series");
        let mut videos = episodes(3);
        videos.push(VideoRef {
            id: "tt9:2:1".into(),
            season: Some(2),
            episode: Some(1),
            released: None,
        });
        let detail = library
            .mark_watched(&show, &videos, &[], Some(1), true)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail.watched_video_ids, ["tt9:1:1", "tt9:1:2", "tt9:1:3"]);

        for (ids, season) in [(vec!["tt9:1:1".to_string()], Some(1)), (vec![], Some(5))] {
            assert!(matches!(
                library
                    .mark_watched(&show, &videos, &ids, season, true)
                    .await,
                Err(IpcError::InvalidInput(_))
            ));
        }
    }

    #[tokio::test]
    async fn whole_title_marks_count_views() {
        let (library, _, _) = library("library-mark-title").await;
        let movie = meta("tt1", "movie");
        let detail = library
            .mark_watched(&movie, &[], &[], None, true)
            .await
            .unwrap()
            .unwrap();
        assert!(detail.entry.watched);
        assert_eq!(detail.entry.times_watched, 1);
        let detail = library
            .mark_watched(&movie, &[], &[], None, false)
            .await
            .unwrap()
            .unwrap();
        assert!(!detail.entry.watched);
    }

    #[tokio::test]
    async fn bad_input_is_refused_before_anything_is_written() {
        let (library, changes, dir) = library("library-bad-input").await;
        for bad in [
            meta("", "movie"),
            meta("tt1", ""),
            meta(&"x".repeat(MAX_ID_LEN + 1), "movie"),
            meta("tt\n1", "movie"),
        ] {
            assert!(matches!(
                library.set_in_library(&bad, true).await,
                Err(IpcError::InvalidInput(_))
            ));
        }
        assert!(library.clear_progress("").await.is_err());
        assert!(changes.lock().unwrap().is_empty());
        assert!(!dir.join("library.json").exists());
    }

    #[tokio::test]
    async fn clear_progress_takes_a_title_out_of_continue_watching() {
        let (library, _, _) = library("library-clear").await;
        let movie = meta("tt1", "movie");
        library.set_in_library(&movie, true).await.unwrap();
        library
            .store
            .update(|file| {
                let item = file.items.get_mut("tt1").unwrap();
                item.state.video_id = Some("tt1".into());
                item.state.time_offset = 60_000;
                item.state.duration = 600_000;
            })
            .await
            .unwrap();
        let cw = LibraryQuery {
            filter: LibraryFilter::ContinueWatching,
            sort: LibrarySort::default(),
            r#type: None,
            limit: Some(10),
        };
        let listed = library.list(&cw).await;
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].resume,
            Some(ResumePoint {
                video_id: "tt1".into(),
                time_offset: 60_000,
                duration: 600_000
            })
        );

        let cleared = library.clear_progress("tt1").await.unwrap().unwrap();
        assert!(!cleared.in_continue_watching);
        assert!(library.list(&cw).await.is_empty());
        assert_eq!(library.clear_progress("nope").await.unwrap(), None);
    }

    #[test]
    fn a_damaged_item_is_skipped_not_the_whole_library() {
        let file: LibraryFile = serde_json::from_value(json!({
            "version": 1,
            "uid": 42,
            "items": {
                "tt1": {"_id": "tt1", "name": "A", "type": "movie", "_mtime": "2024-01-01T00:00:00Z"},
                "bad": "not an item",
                "noid": {"name": "B"},
                "wrongkey": {"_id": "tt3", "name": "C", "type": "movie"}
            }
        }))
        .unwrap();
        assert_eq!(file.uid, None);
        assert_eq!(file.items.keys().collect::<Vec<_>>(), ["tt1", "tt3"]);

        let empty: LibraryFile = serde_json::from_value(json!({"items": []})).unwrap();
        assert_eq!(empty, LibraryFile::default());
        assert_eq!(
            serde_json::from_value::<LibraryFile>(json!({}))
                .unwrap()
                .version,
            1
        );
    }

    #[test]
    fn entries_only_expose_http_posters_and_serialize_camel_case() {
        let mut item = new_item_from_meta(&meta("tt1", "movie"), now());
        let entry = serde_json::to_value(LibraryEntry::from(&item)).unwrap();
        assert_eq!(entry["type"], "movie");
        assert_eq!(entry["posterShape"], "poster");
        assert_eq!(entry["inLibrary"], false);
        assert_eq!(entry["resume"], Value::Null);
        assert!(entry["updatedAt"].as_str().unwrap().ends_with('Z'));

        item.poster = Some("ftp://img.example/p.jpg".parse().unwrap());
        assert_eq!(LibraryEntry::from(&item).poster, None);
    }

    #[test]
    fn refresh_keeps_a_known_poster() {
        let mut item = new_item_from_meta(&meta("tt1", "movie"), now());
        let mut bare = meta("tt1", "movie");
        bare.poster = None;
        bare.name = "Renamed".into();
        refresh(&mut item, &bare);
        assert_eq!(item.name, "Renamed");
        assert!(item.poster.is_some());
    }
}
