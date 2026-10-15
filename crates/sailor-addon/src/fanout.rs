use std::time::Duration;

use serde::Serialize;
use url::Url;

use crate::client::AddonClient;
use crate::manifest::Descriptor;
use crate::request::ResourcePath;
use crate::resource::{MetaItem, Stream};

/// One addon's answer to a stream fan-out, attributed for display.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonStreams {
    pub transport_url: Url,
    pub addon_name: String,
    pub streams: Vec<Stream>,
    pub error: Option<String>,
}

/// Ask every supporting addon for streams in parallel. Results come back in addon order,
/// one entry per supporting addon, each independently succeeding or failing.
///
/// NOTE: this returns once every addon has answered or timed out. Progressive per-addon
/// delivery to the UI (Tauri events, reference Loading/Ready/Err semantics) is planned
/// alongside the embedded player milestone.
pub async fn aggregate_streams(
    client: &AddonClient,
    addons: &[Descriptor],
    r#type: &str,
    video_id: &str,
    per_addon_timeout: Duration,
) -> Vec<AddonStreams> {
    let path = ResourcePath::new("stream", r#type, video_id);
    let tasks = addons
        .iter()
        .filter(|d| d.manifest.is_resource_supported(&path))
        .map(|d| {
            let path = path.clone();
            async move {
                let outcome = tokio::time::timeout(
                    per_addon_timeout,
                    client.streams(&d.transport_url, &path),
                )
                .await;
                let (streams, error) = match outcome {
                    Ok(Ok(response)) => (response.streams, None),
                    Ok(Err(e)) => (Vec::new(), Some(e.to_string())),
                    Err(_) => (Vec::new(), Some("timed out".to_string())),
                };
                AddonStreams {
                    transport_url: d.transport_url.clone(),
                    addon_name: d.manifest.name.clone(),
                    streams,
                    error,
                }
            }
        });
    futures::future::join_all(tasks).await
}

/// Ask every supporting addon for the meta object in parallel and return the first
/// success in addon order (install order is the user's priority order). All requests
/// start concurrently, but the result is taken as soon as the highest-priority addon
/// that will succeed has answered — a slow or dead lower-priority addon never delays it.
pub async fn first_meta(
    client: &AddonClient,
    addons: &[Descriptor],
    r#type: &str,
    id: &str,
    per_addon_timeout: Duration,
) -> Option<MetaItem> {
    let path = ResourcePath::new("meta", r#type, id);
    let handles: Vec<_> = addons
        .iter()
        .filter(|d| d.manifest.is_resource_supported(&path))
        .map(|d| {
            let client = client.clone();
            let transport_url = d.transport_url.clone();
            let path = path.clone();
            tokio::spawn(async move {
                tokio::time::timeout(per_addon_timeout, client.meta(&transport_url, &path))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|r| r.meta)
            })
        })
        .collect();

    let mut handles = handles;
    let mut found = None;
    for handle in handles.iter_mut() {
        if found.is_none() {
            found = handle.await.ok().flatten();
        } else {
            handle.abort();
        }
    }
    found
}
