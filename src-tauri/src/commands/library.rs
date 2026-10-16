//! The local library: its views, and the user's own changes to it (add/remove,
//! watched marks, dismissing from Continue Watching). Playback progress has no command:
//! the shell tracks it from player events, for the `context` given to `player_start`
//! (`crate::library::progress`). Every committed change is announced as
//! `library://changed { ids }`.

use serde::Deserialize;
use tauri::State;
use zeiler_account::{MetaSummary, VideoRef};

use crate::error::IpcError;
use crate::library::{
    LibraryDetail, LibraryEntry, LibraryQuery, LibraryState, check_id, check_videos,
};

/// The items of one view: `continueWatching` (most recently changed first),
/// `library` (in the requested sort) or `history` (everything played, most recent
/// first), optionally of one type and at most `limit` of them.
#[tauri::command]
pub async fn library_list(
    library: State<'_, LibraryState>,
    query: LibraryQuery,
) -> Result<Vec<LibraryEntry>, IpcError> {
    Ok(library.list(&query).await)
}

/// One item, with which of `videos` (the title's video list, as the Detail view has
/// it) are marked watched; `null` when the title was never added, played or marked.
#[tauri::command]
pub async fn library_get(
    library: State<'_, LibraryState>,
    id: String,
    videos: Option<Vec<VideoRef>>,
) -> Result<Option<LibraryDetail>, IpcError> {
    check_id(&id)?;
    let videos = videos.unwrap_or_default();
    check_videos(&videos)?;
    Ok(library.get(&id, &videos).await)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySetRequest {
    pub meta: MetaSummary,
    pub in_library: bool,
}

/// Add a title to the library or remove it. Removing also takes it out of Continue
/// Watching (as in Stremio); it stays in History, and comes back to Continue Watching
/// when it is played again. Returns the item, or `null` when removing a title that was
/// never there.
#[tauri::command]
pub async fn library_set(
    library: State<'_, LibraryState>,
    request: LibrarySetRequest,
) -> Result<Option<LibraryEntry>, IpcError> {
    library
        .set_in_library(&request.meta, request.in_library)
        .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkWatchedRequest {
    pub meta: MetaSummary,
    /// The title's full video list (empty for a movie).
    #[serde(default)]
    pub videos: Vec<VideoRef>,
    /// The videos to mark; empty (and no `season`) marks the whole title.
    #[serde(default)]
    pub video_ids: Vec<String>,
    /// Mark this whole season instead (not together with `video_ids`).
    #[serde(default)]
    pub season: Option<u32>,
    pub watched: bool,
}

/// Mark a whole title, some of its episodes or one season watched or unwatched.
/// Marking the episode Continue Watching points at moves it on, as Stremio's apps do:
/// to the next episode (past the already watched ones for a season or several
/// episodes), or out of Continue Watching when there is none. Returns the item with
/// its episodes' marks, or `null` when unmarking a title that was never stored.
#[tauri::command]
pub async fn library_mark_watched(
    library: State<'_, LibraryState>,
    request: MarkWatchedRequest,
) -> Result<Option<LibraryDetail>, IpcError> {
    library
        .mark_watched(
            &request.meta,
            &request.videos,
            &request.video_ids,
            request.season,
            request.watched,
        )
        .await
}

/// Take a title out of Continue Watching (drops its resume position). `null` when the
/// title is not stored.
#[tauri::command]
pub async fn library_clear_progress(
    library: State<'_, LibraryState>,
    id: String,
) -> Result<Option<LibraryEntry>, IpcError> {
    library.clear_progress(&id).await
}
