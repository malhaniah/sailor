use std::time::Duration;

use serde::Deserialize;
use tauri::State;
use url::Url;
use zeiler_addon::{
    AddonStreams, CatalogResponse, ExtraValue, MetaLookup, ResourcePath, aggregate_streams,
    find_meta,
};

use crate::error::IpcError;
use crate::state::AppState;

const META_TIMEOUT: Duration = Duration::from_secs(15);
const STREAMS_TIMEOUT: Duration = Duration::from_secs(25);

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
