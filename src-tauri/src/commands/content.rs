use std::time::Duration;

use sailor_addon::{
    AddonStreams, CatalogResponse, ExtraValue, MetaItem, ResourcePath, aggregate_streams,
    first_meta,
};
use serde::Deserialize;
use tauri::State;
use url::Url;

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

/// Fetch the meta object for an item: first success across supporting addons,
/// in install order.
#[tauri::command]
pub async fn meta_get(
    state: State<'_, AppState>,
    r#type: String,
    id: String,
) -> Result<MetaItem, IpcError> {
    let addons = state.addons.read().await.clone();
    first_meta(&state.client, &addons, &r#type, &id, META_TIMEOUT)
        .await
        .ok_or_else(|| {
            IpcError::NotFound(
                "No installed addon provides details for this item. Install a metadata addon that covers it.".into(),
            )
        })
}

/// Fan out a stream request to every supporting addon; per-addon results, in install order.
#[tauri::command]
pub async fn streams_get(
    state: State<'_, AppState>,
    r#type: String,
    id: String,
) -> Result<Vec<AddonStreams>, IpcError> {
    let addons = state.addons.read().await.clone();
    Ok(aggregate_streams(&state.client, &addons, &r#type, &id, STREAMS_TIMEOUT).await)
}
