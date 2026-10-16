use std::time::Duration;

use serde::Serialize;
use url::Url;

use crate::client::AddonClient;
use crate::manifest::Descriptor;
use crate::request::{ExtraValue, ResourcePath};
use crate::resource::{MetaItem, Stream, Subtitle};

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

/// One addon's answer to a subtitles fan-out, attributed for display.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonSubtitles {
    pub transport_url: Url,
    pub addon_name: String,
    pub subtitles: Vec<Subtitle>,
    pub error: Option<String>,
}

/// Ask every addon that serves subtitles for this type and video id, in parallel, like
/// [`aggregate_streams`]. `extra` carries what the stream says about the file
/// (`videoHash`, `videoSize`, `filename`, in that order, as Stremio's apps send them);
/// it goes into each request's extra segment. Results come back in addon order, one
/// entry per supporting addon.
pub async fn aggregate_subtitles(
    client: &AddonClient,
    addons: &[Descriptor],
    r#type: &str,
    video_id: &str,
    extra: &[ExtraValue],
    per_addon_timeout: Duration,
) -> Vec<AddonSubtitles> {
    let mut path = ResourcePath::new("subtitles", r#type, video_id);
    path.extra = extra.to_vec();
    let tasks = addons
        .iter()
        .filter(|d| d.manifest.is_resource_supported(&path))
        .map(|d| {
            let path = path.clone();
            async move {
                let outcome = tokio::time::timeout(
                    per_addon_timeout,
                    client.subtitles(&d.transport_url, &path),
                )
                .await;
                let (subtitles, error) = match outcome {
                    Ok(Ok(response)) => (response.subtitles, None),
                    Ok(Err(e)) => (Vec::new(), Some(e.to_string())),
                    Err(_) => (Vec::new(), Some("timed out".to_string())),
                };
                AddonSubtitles {
                    transport_url: d.transport_url.clone(),
                    addon_name: d.manifest.name.clone(),
                    subtitles,
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
    /// The transport URL of the addon that answered (not sent to the UI).
    #[serde(skip)]
    pub source: Option<Url>,
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
    let addons: Vec<_> = addons.iter().collect();
    meta_attempt(client, &addons, r#type, id, per_addon_timeout, false)
        .await
        .lookup
}

/// Id prefixes of the anime databases (Kitsu, MyAnimeList, AniList, AniDB) that anime
/// metadata addons serve.
const ANIME_ID_PREFIXES: [&str; 4] = ["kitsu:", "mal:", "anilist:", "anidb:"];
/// The types an anime id's meta is also looked up under, in this order. Anime addons file
/// a title as `series` or `movie` while catalogs list it as `anime` (or the other way
/// round), and a catalog item's type need not be one its meta addon declares.
const ANIME_META_TYPES: [&str; 3] = ["series", "movie", "anime"];

/// The types to look an id's meta up under, in order: the requested type, then for an
/// anime id each other type of [`ANIME_META_TYPES`].
pub fn meta_types_to_try<'a>(r#type: &'a str, id: &str) -> Vec<&'a str> {
    let mut types = vec![r#type];
    if ANIME_ID_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix))
    {
        types.extend(ANIME_META_TYPES.into_iter().filter(|t| *t != r#type));
    }
    types
}

/// [`first_meta`] under each type of [`meta_types_to_try`] in turn, until one finds the
/// meta (for a non-anime id that is just the requested type). `asked` lists every addon
/// asked under any type, and the errors of a retry name the type. An addon that could
/// not be reached (no connection, or no answer in time) is not asked again, so a dead
/// addon costs one timeout, not one per type.
pub async fn find_meta(
    client: &AddonClient,
    addons: &[Descriptor],
    r#type: &str,
    id: &str,
    per_addon_timeout: Duration,
) -> MetaLookup {
    let mut candidates: Vec<_> = addons.iter().collect();
    let mut lookup = MetaLookup {
        meta: None,
        asked: Vec::new(),
        errors: Vec::new(),
        source: None,
    };
    for (attempt, meta_type) in meta_types_to_try(r#type, id).into_iter().enumerate() {
        let MetaAttempt {
            lookup: found,
            unreachable,
        } = meta_attempt(
            client,
            &candidates,
            meta_type,
            id,
            per_addon_timeout,
            attempt > 0,
        )
        .await;
        for name in found.asked {
            if !lookup.asked.contains(&name) {
                lookup.asked.push(name);
            }
        }
        lookup.errors.extend(found.errors);
        if found.meta.is_some() {
            lookup.meta = found.meta;
            lookup.source = found.source;
            break;
        }
        candidates.retain(|d| !unreachable.contains(&d.transport_url));
    }
    lookup
}

/// One meta lookup under one type, plus the addons that could not be reached.
struct MetaAttempt {
    lookup: MetaLookup,
    unreachable: Vec<Url>,
}

enum MetaFailure {
    /// No connection, or no answer in time.
    Unreachable(String),
    /// The addon answered, with an error.
    Failed(String),
}

/// The body of [`first_meta`]. `name_the_type` labels each error `<addon> (<type>)`, for
/// the retries of [`find_meta`].
async fn meta_attempt(
    client: &AddonClient,
    addons: &[&Descriptor],
    r#type: &str,
    id: &str,
    per_addon_timeout: Duration,
    name_the_type: bool,
) -> MetaAttempt {
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
                    Ok(Err(e)) if e.is_unreachable() => {
                        Err(MetaFailure::Unreachable(e.to_string()))
                    }
                    Ok(Err(e)) => Err(MetaFailure::Failed(e.to_string())),
                    Err(_) => Err(MetaFailure::Unreachable("timed out".to_string())),
                }
            })
        })
        .collect();

    let mut attempt = MetaAttempt {
        lookup: MetaLookup {
            meta: None,
            asked: supporting.iter().map(|d| d.manifest.name.clone()).collect(),
            errors: Vec::new(),
            source: None,
        },
        unreachable: Vec::new(),
    };
    for (handle, addon) in handles.iter_mut().zip(&supporting) {
        if attempt.lookup.meta.is_some() {
            handle.abort();
            continue;
        }
        let name = if name_the_type {
            format!("{} ({})", addon.manifest.name, r#type)
        } else {
            addon.manifest.name.clone()
        };
        match handle.await {
            Ok(Ok(meta)) => {
                attempt.lookup.meta = Some(meta);
                attempt.lookup.source = Some(addon.transport_url.clone());
            }
            Ok(Err(MetaFailure::Failed(reason))) => {
                attempt.lookup.errors.push(format!("{name}: {reason}"));
            }
            Ok(Err(MetaFailure::Unreachable(reason))) => {
                attempt.lookup.errors.push(format!("{name}: {reason}"));
                attempt.unreachable.push(addon.transport_url.clone());
            }
            Err(_) => attempt
                .lookup
                .errors
                .push(format!("{name}: request was cancelled")),
        }
    }
    attempt
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

    #[test]
    fn anime_ids_are_retried_under_the_other_anime_types() {
        assert_eq!(
            meta_types_to_try("anime", "kitsu:1"),
            ["anime", "series", "movie"]
        );
        assert_eq!(
            meta_types_to_try("series", "mal:5114"),
            ["series", "movie", "anime"]
        );
        assert_eq!(
            meta_types_to_try("movie", "anilist:1"),
            ["movie", "series", "anime"]
        );
        assert_eq!(
            meta_types_to_try("other", "anidb:1"),
            ["other", "series", "movie", "anime"]
        );
        assert_eq!(meta_types_to_try("series", "tt0108778"), ["series"]);
        assert_eq!(meta_types_to_try("movie", "kitsu"), ["movie"], "not an id");
    }

    /// A loopback addon for offline tests. It answers `GET` with the body `routes` gives
    /// the meta path for (404 otherwise), or never answers when `hang` is set, and logs
    /// every requested path.
    struct Loopback {
        transport_url: Url,
        requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Loopback {
        async fn start(routes: &[(&str, &str, &str)], hang: bool) -> Loopback {
            let routes: Vec<_> = routes
                .iter()
                .map(|(r#type, id, body)| (ResourcePath::new("meta", *r#type, *id), *body))
                .collect();
            Self::serve(&routes, hang).await
        }

        /// Answers each of `routes` (any resource) with its body, 404 otherwise.
        async fn serve(routes: &[(ResourcePath, &str)], hang: bool) -> Loopback {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let transport_url = Url::parse(&format!(
                "http://{}/manifest.json",
                listener.local_addr().unwrap()
            ))
            .unwrap();
            let routes: Vec<(String, String)> = routes
                .iter()
                .map(|(path, body)| {
                    let url = crate::request::build_resource_url(&transport_url, path).unwrap();
                    (url.path().to_owned(), (*body).to_owned())
                })
                .collect();
            let requests: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
            let log = std::sync::Arc::clone(&requests);
            tokio::spawn(async move {
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let routes = routes.clone();
                    let log = std::sync::Arc::clone(&log);
                    tokio::spawn(async move {
                        let mut head = Vec::new();
                        let mut chunk = [0u8; 1024];
                        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                            match socket.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => head.extend_from_slice(&chunk[..n]),
                            }
                        }
                        let head = String::from_utf8_lossy(&head);
                        let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
                        log.lock().unwrap().push(path.clone());
                        if hang {
                            std::future::pending::<()>().await;
                        }
                        let (status, body) = routes
                            .iter()
                            .find(|(p, _)| *p == path)
                            .map_or(("404 Not Found", ""), |(_, body)| ("200 OK", body));
                        let response = format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n\
                             content-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                    });
                }
            });
            Loopback {
                transport_url,
                requests,
            }
        }

        fn addon(&self, name: &str, types: &str) -> Descriptor {
            serde_json::from_str(&format!(
                r#"{{ "manifest": {{ "id": "{name}", "name": "{name}", "types": {types},
                     "idPrefixes": ["kitsu"], "resources": ["meta"], "catalogs": [] }},
                     "transportUrl": "{}" }}"#,
                self.transport_url
            ))
            .unwrap()
        }

        /// The requested meta types, in order.
        fn requested_types(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|path| path.split('/').nth(2).unwrap_or("").to_owned())
                .collect()
        }
    }

    const BEBOP: &str =
        r#"{ "meta": { "id": "kitsu:1", "type": "series", "name": "Cowboy Bebop" } }"#;

    #[tokio::test]
    async fn an_anime_meta_is_found_under_the_type_its_addon_serves() {
        let server = Loopback::start(&[("movie", "kitsu:1", BEBOP)], false).await;
        // declares series and movie, not the catalog's "anime"; 404s for series
        let addons = vec![server.addon("Anime", r#"["series", "movie"]"#)];
        let client = AddonClient::for_loopback_tests();

        let lookup = find_meta(&client, &addons, "anime", "kitsu:1", Duration::from_secs(5)).await;

        assert_eq!(lookup.meta.unwrap().name, "Cowboy Bebop");
        assert_eq!(server.requested_types(), ["series", "movie"]);
        assert_eq!(lookup.asked, ["Anime"], "listed once");
        assert_eq!(lookup.errors.len(), 1);
        assert!(
            lookup.errors[0].starts_with("Anime (series): "),
            "{:?}",
            lookup.errors
        );
    }

    #[tokio::test]
    async fn the_requested_type_is_tried_first_and_a_hit_stops_the_retries() {
        let server = Loopback::start(&[("anime", "kitsu:1", BEBOP)], false).await;
        let addons = vec![server.addon("Anime", r#"["anime", "series", "movie"]"#)];
        let client = AddonClient::for_loopback_tests();

        let lookup = find_meta(&client, &addons, "anime", "kitsu:1", Duration::from_secs(5)).await;

        assert!(lookup.meta.is_some());
        assert_eq!(server.requested_types(), ["anime"]);
        assert!(lookup.errors.is_empty());
    }

    #[tokio::test]
    async fn an_addon_that_timed_out_is_not_asked_again() {
        let dead = Loopback::start(&[], true).await;
        let alive = Loopback::start(&[("series", "kitsu:1", BEBOP)], false).await;
        let addons = vec![
            dead.addon("Dead", r#"["anime", "series", "movie"]"#),
            alive.addon("Alive", r#"["anime", "series", "movie"]"#),
        ];
        let client = AddonClient::for_loopback_tests();

        let lookup = find_meta(&client, &addons, "anime", "kitsu:1", Duration::from_secs(1)).await;

        assert!(lookup.meta.is_some());
        assert_eq!(
            dead.requested_types(),
            ["anime"],
            "one timeout, not one per type"
        );
        assert_eq!(alive.requested_types(), ["anime", "series"]);
        assert_eq!(lookup.asked, ["Dead", "Alive"]);
        assert_eq!(lookup.errors[0], "Dead: timed out");
    }

    /// A connection error counts like a timeout: the client's own connect timeout (10 s)
    /// fires before the lookup's, and a host that refuses now refuses for every type.
    #[tokio::test]
    async fn an_addon_that_cannot_be_connected_to_is_not_asked_again() {
        let closed = {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap()
        }; // dropped: connections are refused
        let gone: Descriptor = serde_json::from_str(&format!(
            r#"{{ "manifest": {{ "id": "gone", "name": "Gone", "types": ["anime", "series", "movie"],
                 "idPrefixes": ["kitsu"], "resources": ["meta"], "catalogs": [] }},
                 "transportUrl": "http://{closed}/manifest.json" }}"#
        ))
        .unwrap();
        let client = AddonClient::for_loopback_tests();

        let lookup = find_meta(&client, &[gone], "anime", "kitsu:1", Duration::from_secs(5)).await;

        assert!(lookup.meta.is_none());
        assert_eq!(lookup.asked, ["Gone"]);
        assert_eq!(lookup.errors.len(), 1, "asked once: {:?}", lookup.errors);
    }

    fn subtitles_addon(name: &str, transport_url: &Url) -> Descriptor {
        serde_json::from_str(&format!(
            r#"{{ "manifest": {{ "id": "{name}", "name": "{name}", "types": ["movie", "series"],
                 "idPrefixes": ["tt"], "resources": ["subtitles"], "catalogs": [] }},
                 "transportUrl": "{transport_url}" }}"#
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn subtitles_are_asked_with_the_file_extras_and_attributed_per_addon() {
        let extra = [
            ExtraValue {
                name: "videoHash".into(),
                value: "8e245d9679d31e12".into(),
            },
            ExtraValue {
                name: "videoSize".into(),
                value: "1351620608".into(),
            },
            ExtraValue {
                name: "filename".into(),
                value: "Show S01E01 1080p.mkv".into(),
            },
        ];
        let mut path = ResourcePath::new("subtitles", "series", "tt1:1:1");
        path.extra = extra.to_vec();
        // one entry lacks its url: skipped, not fatal
        let good = Loopback::serve(
            &[(
                path,
                r#"{ "subtitles": [
                    { "id": "1", "url": "https://subs.example/1.srt", "lang": "eng" },
                    { "id": "2", "lang": "ara" },
                    { "id": "3", "url": "https://subs.example/3.srt", "lang": null, "SubEncoding": "UTF-8" }
                ] }"#,
            )],
            false,
        )
        .await;
        let broken = Loopback::serve(&[], false).await;
        let mut meta_only = good.addon("Meta", r#"["series"]"#);
        meta_only.manifest.id_prefixes = Some(vec!["tt".into()]);
        let addons = vec![
            subtitles_addon("Good", &good.transport_url),
            meta_only,
            subtitles_addon("Broken", &broken.transport_url),
        ];
        let client = AddonClient::for_loopback_tests();

        let results = aggregate_subtitles(
            &client,
            &addons,
            "series",
            "tt1:1:1",
            &extra,
            Duration::from_secs(5),
        )
        .await;

        let names: Vec<_> = results.iter().map(|r| r.addon_name.as_str()).collect();
        assert_eq!(names, ["Good", "Broken"], "only subtitle addons, in order");
        assert_eq!(results[0].error, None);
        let urls: Vec<_> = results[0]
            .subtitles
            .iter()
            .map(|s| s.url.as_str())
            .collect();
        assert_eq!(
            urls,
            ["https://subs.example/1.srt", "https://subs.example/3.srt"]
        );
        assert_eq!(
            results[0].subtitles[1].lang, "",
            "a null lang reads as empty"
        );
        assert!(results[1].subtitles.is_empty());
        assert!(
            results[1]
                .error
                .as_deref()
                .is_some_and(|e| e.contains("404"))
        );
        assert_eq!(
            good.requests.lock().unwrap().as_slice(),
            [
                "/subtitles/series/tt1%3A1%3A1/videoHash=8e245d9679d31e12&videoSize=1351620608&filename=Show%20S01E01%201080p.mkv.json"
            ]
        );
    }

    #[tokio::test]
    async fn no_subtitle_request_for_an_id_no_addon_serves() {
        let server = Loopback::serve(&[], false).await;
        let addons = vec![subtitles_addon("OpenSubtitles", &server.transport_url)];
        let client = AddonClient::for_loopback_tests();
        let results = aggregate_subtitles(
            &client,
            &addons,
            "anime",
            "kitsu:1:1",
            &[],
            Duration::from_secs(5),
        )
        .await;
        assert!(results.is_empty());
        assert!(server.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_non_anime_id_is_asked_under_its_type_only() {
        let server = Loopback::start(&[], false).await;
        let mut addon = server.addon("Movies", r#"["movie", "series"]"#);
        addon.manifest.id_prefixes = Some(vec!["tt".into()]);
        let client = AddonClient::for_loopback_tests();

        let lookup = find_meta(
            &client,
            &[addon],
            "movie",
            "tt0111161",
            Duration::from_secs(5),
        )
        .await;

        assert!(lookup.meta.is_none());
        assert_eq!(server.requested_types(), ["movie"]);
        assert_eq!(lookup.asked, ["Movies"]);
        assert_eq!(lookup.errors.len(), 1);
    }
}
