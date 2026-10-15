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

/// Outcome of a meta lookup. `asked` is empty when no installed addon serves meta for
/// this type/id — the common case of catalog-only + stream-only addon setups.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaLookup {
    pub meta: Option<MetaItem>,
    /// Names of the addons that were asked, in priority order.
    pub asked: Vec<String>,
    /// `"<addon>: <reason>"` for each asked addon that failed before a success.
    pub errors: Vec<String>,
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
) -> MetaLookup {
    let path = ResourcePath::new("meta", r#type, id);
    let supporting: Vec<_> = addons
        .iter()
        .filter(|d| d.manifest.is_resource_supported(&path))
        .collect();
    let mut handles: Vec<_> = supporting
        .iter()
        .map(|d| {
            let client = client.clone();
            let transport_url = d.transport_url.clone();
            let path = path.clone();
            tokio::spawn(async move {
                match tokio::time::timeout(per_addon_timeout, client.meta(&transport_url, &path))
                    .await
                {
                    Ok(Ok(response)) => Ok(response.meta),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("timed out".to_string()),
                }
            })
        })
        .collect();

    let mut lookup = MetaLookup {
        meta: None,
        asked: supporting.iter().map(|d| d.manifest.name.clone()).collect(),
        errors: Vec::new(),
    };
    for (handle, addon) in handles.iter_mut().zip(&supporting) {
        if lookup.meta.is_some() {
            handle.abort();
            continue;
        }
        match handle.await {
            Ok(Ok(meta)) => lookup.meta = Some(meta),
            Ok(Err(reason)) => lookup
                .errors
                .push(format!("{}: {reason}", addon.manifest.name)),
            Err(_) => lookup
                .errors
                .push(format!("{}: request was cancelled", addon.manifest.name)),
        }
    }
    lookup
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(manifest: &str) -> Descriptor {
        serde_json::from_str(&format!(
            r#"{{ "manifest": {manifest}, "transportUrl": "https://addon.invalid/manifest.json" }}"#
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn meta_lookup_reports_when_no_addon_serves_meta() {
        let addons = vec![
            descriptor(
                r#"{ "id": "catalogs", "name": "Catalogs", "types": ["movie"],
                     "idPrefixes": ["tt"], "resources": ["catalog"],
                     "catalogs": [{ "type": "movie", "id": "top" }] }"#,
            ),
            descriptor(
                r#"{ "id": "streams", "name": "Streams", "types": ["movie"],
                     "resources": [{ "name": "stream", "types": ["movie"], "idPrefixes": ["tt"] }],
                     "catalogs": [] }"#,
            ),
        ];
        let client = AddonClient::new().unwrap();
        // no addon supports meta, so nothing is requested (the .invalid host is never hit)
        let lookup = first_meta(
            &client,
            &addons,
            "movie",
            "tt0111161",
            Duration::from_secs(1),
        )
        .await;
        assert!(lookup.meta.is_none());
        assert!(lookup.asked.is_empty());
        assert!(lookup.errors.is_empty());
    }

    #[tokio::test]
    async fn meta_lookup_records_failures_of_asked_addons() {
        let addons = vec![descriptor(
            r#"{ "id": "meta", "name": "Broken Meta", "types": ["movie"],
                 "idPrefixes": ["tt"], "resources": ["meta"], "catalogs": [] }"#,
        )];
        let client = AddonClient::new().unwrap();
        let lookup = first_meta(
            &client,
            &addons,
            "movie",
            "tt0111161",
            Duration::from_secs(5),
        )
        .await;
        assert!(lookup.meta.is_none());
        assert_eq!(lookup.asked, vec!["Broken Meta"]);
        assert_eq!(lookup.errors.len(), 1);
        assert!(lookup.errors[0].starts_with("Broken Meta: "));
    }
}
