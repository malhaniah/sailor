use std::time::Duration;

use serde::Deserialize;
use tauri::State;
use url::Url;
use zeiler_addon::{
    AddonStreams, AddonSubtitles, CatalogResponse, ExtraValue, MetaLookup, ResourcePath,
    aggregate_streams, aggregate_subtitles, find_meta,
};

use crate::error::IpcError;
use crate::state::AppState;

const META_TIMEOUT: Duration = Duration::from_secs(15);
const STREAMS_TIMEOUT: Duration = Duration::from_secs(25);
/// Per addon: subtitles are asked while the video opens and must not hold it up.
const SUBTITLES_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogArgs {
    pub transport_url: String,
    pub r#type: String,
    pub id: String,
    #[serde(default)]
    pub extra: Vec<ExtraValue>,
}

/// Load one catalog page from one addon.
#[tauri::command]
pub async fn catalog_load(
    state: State<'_, AppState>,
    args: CatalogArgs,
) -> Result<CatalogResponse, IpcError> {
    let transport_url = Url::parse(&args.transport_url)
        .map_err(|e| IpcError::InvalidInput(format!("bad transport url: {e}")))?;
    let mut path = ResourcePath::new("catalog", args.r#type, args.id);
    path.extra = args.extra;
    Ok(state.client.catalog(&transport_url, &path).await?)
}

/// Fetch the meta object for an item: first success across supporting addons, in
/// install order. An anime id (`kitsu:`, `mal:`, `anilist:`, `anidb:`) that nobody
/// answers under its catalog type is retried as series, movie and anime, since anime
/// addons file titles under types other than the catalog's. A missing meta is not an
/// error — the UI falls back to the catalog preview — so the outcome says who was asked
/// and what failed. Streams a built-in embeds in its videos are dropped.
#[tauri::command]
pub async fn meta_get(
    state: State<'_, AppState>,
    r#type: String,
    id: String,
) -> Result<MetaLookup, IpcError> {
    let addons = state.effective_addons().await;
    let lookup = find_meta(&state.client, &addons, &r#type, &id, META_TIMEOUT).await;
    Ok(crate::builtin::without_builtin_streams(lookup, &addons))
}

/// Fan out a stream request to every supporting addon; per-addon results, in install order.
#[tauri::command]
pub async fn streams_get(
    state: State<'_, AppState>,
    r#type: String,
    id: String,
) -> Result<Vec<AddonStreams>, IpcError> {
    let addons = state.effective_addons().await;
    Ok(aggregate_streams(&state.client, &addons, &r#type, &id, STREAMS_TIMEOUT).await)
}

/// Fan out a subtitles request to every addon that serves subtitles for this type and
/// video id (the built-in OpenSubtitles v3 when it is on); per-addon results, in
/// install order. What the stream says about its file — the OpenSubtitles hash, the
/// size and the file name — goes along, so addons can match the exact release; a
/// value that is not well-formed is left out rather than refusing the request.
#[tauri::command]
pub async fn subtitles_get(
    state: State<'_, AppState>,
    r#type: String,
    id: String,
    video_hash: Option<String>,
    video_size: Option<u64>,
    filename: Option<String>,
) -> Result<Vec<AddonSubtitles>, IpcError> {
    let extra = subtitle_extra(video_hash, video_size, filename);
    let addons = state.effective_addons().await;
    Ok(aggregate_subtitles(
        &state.client,
        &addons,
        &r#type,
        &id,
        &extra,
        SUBTITLES_TIMEOUT,
    )
    .await)
}

/// The `videoHash`, `videoSize`, `filename` extras, in Stremio's order, each only when
/// it is well-formed: a 16-digit hex OpenSubtitles hash, a size above zero, and a file
/// name (no path) without control characters.
fn subtitle_extra(
    video_hash: Option<String>,
    video_size: Option<u64>,
    filename: Option<String>,
) -> Vec<ExtraValue> {
    let extra = |name: &str, value: String| ExtraValue {
        name: name.to_string(),
        value,
    };
    let hash = video_hash
        .filter(|h| (1..=16).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_hexdigit()));
    let size = video_size.filter(|s| *s > 0);
    let filename = filename.filter(|f| {
        (1..=255).contains(&f.len()) && !f.contains(['/', '\\']) && !f.chars().any(char::is_control)
    });
    [
        hash.map(|h| extra("videoHash", h)),
        size.map(|s| extra("videoSize", s.to_string())),
        filename.map(|f| extra("filename", f)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_extras_keep_only_well_formed_values() {
        let names = |extra: Vec<ExtraValue>| -> Vec<String> {
            extra
                .into_iter()
                .map(|e| format!("{}={}", e.name, e.value))
                .collect()
        };
        assert_eq!(
            names(subtitle_extra(
                Some("8e245d9679d31e12".into()),
                Some(1_351_620_608),
                Some("Show.S01E01.1080p.WEB.mkv".into())
            )),
            [
                "videoHash=8e245d9679d31e12",
                "videoSize=1351620608",
                "filename=Show.S01E01.1080p.WEB.mkv"
            ]
        );
        assert!(names(subtitle_extra(None, None, None)).is_empty());
        assert!(
            names(subtitle_extra(
                Some("not hex".into()),
                Some(0),
                Some("../../etc/passwd".into())
            ))
            .is_empty()
        );
        assert!(names(subtitle_extra(None, None, Some("a\nb.mkv".into()))).is_empty());
        assert_eq!(
            names(subtitle_extra(Some("0123456789abcdef0".into()), None, None)).len(),
            0,
            "too long for an OpenSubtitles hash"
        );
    }
}
