//! Skip times for the player (M7): where a video's intro, recap and credits are, from
//! AniSkip and TheIntroDB (`zeiler-skip`). The webview cannot reach those hosts (the CSP
//! allows only IPC), so the lookup runs here. The UI falls back to the file's chapter
//! names when this finds nothing (src/lib/skip.ts).

use serde::Deserialize;
use tauri::State;
use zeiler_skip::{Segment, SkipClient, route};

use crate::error::IpcError;
use crate::state::AppState;

/// Longest id accepted (Stremio ids are short; a longer one is not a lookup).
const MAX_ID_LEN: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipQuery {
    pub meta_id: String,
    pub video_id: String,
    /// The video's length, in seconds.
    pub duration_sec: f64,
}

/// The skip segments for a video, sorted by start; empty when online lookups are off
/// (`skip.onlineLookup`), the id is not one these services know (`kitsu:`, `mal:`,
/// `tt…`, `tmdb:`), or they know nothing about it. Never an error for an answer that
/// did not come: playback goes on without it.
#[tauri::command]
pub async fn skip_segments_get(
    state: State<'_, AppState>,
    client: State<'_, SkipClient>,
    query: SkipQuery,
) -> Result<Vec<Segment>, IpcError> {
    if query.meta_id.len() > MAX_ID_LEN || query.video_id.len() > MAX_ID_LEN {
        return Err(IpcError::InvalidInput("That video id is too long.".into()));
    }
    let duration = query.duration_sec;
    if !duration.is_finite() || !(0.0..=1_000_000.0).contains(&duration) {
        return Err(IpcError::InvalidInput(
            "The video's length must be a number of seconds.".into(),
        ));
    }
    if !state.settings.read().await.skip.online_lookup {
        return Ok(Vec::new());
    }
    let Some(lookup) = route(&query.meta_id, &query.video_id) else {
        return Ok(Vec::new());
    };
    Ok(client.segments(&lookup, duration).await)
}
