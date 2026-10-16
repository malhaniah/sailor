use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{DefaultOnError, DefaultOnNull, VecSkipError, serde_as};

/// Accepts a JSON string or number and yields a string; anything else becomes `None`.
/// Real addons return `imdbRating`, `runtime` and `releaseInfo` as either type.
fn opt_number_or_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    })
}

/// Accepts a JSON string and yields it unless empty; anything else becomes `None`.
/// Addons routinely send `""` or `null` for image URLs.
fn opt_url_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s),
        _ => None,
    })
}

/// `released` is an ISO-8601 string or an epoch-milliseconds number in the wild;
/// normalize to an RFC 3339 string so `new Date(value)` always works in the UI.
fn opt_released<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s),
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|d| d.to_rfc3339()),
        _ => None,
    })
}

/// Embedded video streams: the key is `streams` or legacy `stream`, holding either an
/// array or a single stream object; unsupported entries are skipped, never fatal.
fn one_or_many_streams<'de, D>(deserializer: D) -> Result<Vec<Stream>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        Some(value @ serde_json::Value::Object(_)) => {
            serde_json::from_value(value).ok().into_iter().collect()
        }
        _ => Vec::new(),
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PosterShape {
    #[default]
    Poster,
    Square,
    Landscape,
}

/// A catalog item. Catalog responses from real addons range from slim previews to full
/// meta objects — unknown fields are simply ignored.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaPreview {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde_as(deserialize_as = "DefaultOnNull")]
    #[serde(default)]
    pub name: String,
    #[serde(default, deserialize_with = "opt_url_string")]
    pub poster: Option<String>,
    #[serde(default, deserialize_with = "opt_url_string")]
    pub background: Option<String>,
    #[serde(default, deserialize_with = "opt_url_string")]
    pub logo: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub poster_shape: PosterShape,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "opt_number_or_string")]
    pub release_info: Option<String>,
    #[serde(default, deserialize_with = "opt_number_or_string")]
    pub imdb_rating: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub genres: Vec<String>,
}

/// A full meta object from `/meta/{type}/{id}.json`.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaItem {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde_as(deserialize_as = "DefaultOnNull")]
    #[serde(default)]
    pub name: String,
    #[serde(default, deserialize_with = "opt_url_string")]
    pub poster: Option<String>,
    #[serde(default, deserialize_with = "opt_url_string")]
    pub background: Option<String>,
    #[serde(default, deserialize_with = "opt_url_string")]
    pub logo: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub poster_shape: PosterShape,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "opt_number_or_string")]
    pub release_info: Option<String>,
    #[serde(default, deserialize_with = "opt_number_or_string")]
    pub imdb_rating: Option<String>,
    #[serde(default, deserialize_with = "opt_number_or_string")]
    pub runtime: Option<String>,
    #[serde(default, deserialize_with = "opt_released")]
    pub released: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub website: Option<String>,
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub links: Vec<MetaLink>,
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub videos: Vec<Video>,
    #[serde(default)]
    pub behavior_hints: MetaBehaviorHints,
}

impl MetaItem {
    /// The video id to request streams for without the user picking an episode:
    /// `behaviorHints.defaultVideoId`; else the meta id for items with no episode list
    /// (movies); else the meta id for live items (streams for tv/EPG channels are always
    /// requested with the channel id, never a programme id). Episodic content returns
    /// None — the user picks a video.
    pub fn default_video_id(&self) -> Option<&str> {
        if let Some(id) = self.behavior_hints.default_video_id.as_deref() {
            return Some(id);
        }
        if self.videos.is_empty() || self.is_live() {
            Some(&self.id)
        } else {
            None
        }
    }

    pub fn is_live(&self) -> bool {
        self.behavior_hints.is_live || self.r#type == "tv"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaLink {
    pub name: String,
    pub category: String,
    pub url: String,
}

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaBehaviorHints {
    #[serde(default)]
    pub default_video_id: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub is_live: bool,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub has_scheduled_videos: bool,
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// One episode / video of a series or channel. Cinemeta uses `name` instead of the
/// documented `title`, hence the alias.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    pub id: String,
    #[serde(default, alias = "name")]
    pub title: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub season: Option<i64>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub episode: Option<i64>,
    #[serde(default, deserialize_with = "opt_released")]
    pub released: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default, deserialize_with = "opt_url_string")]
    pub thumbnail: Option<String>,
    /// Streams embedded directly on the video; when present, clients use them instead of
    /// asking stream addons.
    #[serde(default, alias = "stream", deserialize_with = "one_or_many_streams")]
    pub streams: Vec<Stream>,
}

/// A playable stream offer. Exactly one source group applies; the untagged order makes
/// `url` win when an addon sends both `url` and `infoHash`, matching the reference client.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    #[serde(default)]
    pub name: Option<String>,
    /// `title` is the deprecated alias of `description`.
    #[serde(default, alias = "title")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub source: StreamSource,
    #[serde_as(deserialize_as = "DefaultOnError<VecSkipError<_>>")]
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
    #[serde(default)]
    pub behavior_hints: StreamBehaviorHints,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StreamSource {
    Url {
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    YouTube {
        yt_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Torrent {
        /// 40-char hex; validated at playback time, not parse time.
        info_hash: String,
        #[serde(default)]
        file_idx: Option<u32>,
        /// Tracker/DHT hints; `announce` and `sources` are aliases in the wild.
        #[serde_as(deserialize_as = "DefaultOnNull")]
        #[serde(default, alias = "sources")]
        announce: Vec<String>,
        #[serde_as(deserialize_as = "DefaultOnNull")]
        #[serde(default)]
        file_must_include: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    PlayerFrame {
        player_frame_url: String,
    },
    #[serde(rename_all = "camelCase")]
    External {
        external_url: String,
    },
}

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamBehaviorHints {
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub not_web_ready: bool,
    #[serde(default)]
    pub binge_group: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub video_hash: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub video_size: Option<u64>,
    #[serde(default)]
    pub proxy_headers: Option<ProxyHeaders>,
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyHeaders {
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub request: BTreeMap<String, String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub response: BTreeMap<String, String>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtitle {
    #[serde(default)]
    pub id: Option<String>,
    pub url: String,
    #[serde_as(deserialize_as = "DefaultOnNull")]
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub label: Option<String>,
}

// ---- response envelopes -------------------------------------------------------------

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogResponse {
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub metas: Vec<MetaPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaResponse {
    pub meta: MetaItem,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamsResponse {
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub streams: Vec<Stream>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitlesResponse {
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub subtitles: Vec<Subtitle>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddonCatalogResponse {
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub addons: Vec<crate::manifest::Descriptor>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_url_variant() {
        let s: Stream = serde_json::from_str(
            r#"{ "url": "https://cdn.example/v.mp4", "name": "1080p",
                 "behaviorHints": { "notWebReady": true,
                                    "proxyHeaders": { "request": { "User-Agent": "Stremio" } },
                                    "filename": "v.mp4", "videoSize": 1000000,
                                    "bingeGroup": "x-1080p" } }"#,
        )
        .unwrap();
        match &s.source {
            StreamSource::Url { url } => assert_eq!(url, "https://cdn.example/v.mp4"),
            other => panic!("expected Url, got {other:?}"),
        }
        assert!(s.behavior_hints.not_web_ready);
        assert_eq!(s.behavior_hints.video_size, Some(1_000_000));
        assert_eq!(
            s.behavior_hints
                .proxy_headers
                .as_ref()
                .unwrap()
                .request
                .get("User-Agent")
                .map(String::as_str),
            Some("Stremio")
        );
    }

    #[test]
    fn stream_torrent_variant_with_sources_alias() {
        let s: Stream = serde_json::from_str(
            r#"{ "infoHash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                 "fileIdx": 1,
                 "sources": ["tracker:udp://tracker.opentrackr.org:1337/announce", "dht:aaaa"],
                 "title": "Some.Release.1080p" }"#,
        )
        .unwrap();
        match &s.source {
            StreamSource::Torrent {
                info_hash,
                file_idx,
                announce,
                ..
            } => {
                assert_eq!(info_hash, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
                assert_eq!(*file_idx, Some(1));
                assert_eq!(announce.len(), 2);
            }
            other => panic!("expected Torrent, got {other:?}"),
        }
        // deprecated `title` lands in description
        assert_eq!(s.description.as_deref(), Some("Some.Release.1080p"));
    }

    #[test]
    fn url_wins_over_info_hash_when_both_present() {
        let s: Stream = serde_json::from_str(
            r#"{ "url": "https://direct.example/v.mkv",
                 "infoHash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }"#,
        )
        .unwrap();
        assert!(matches!(s.source, StreamSource::Url { .. }));
    }

    #[test]
    fn youtube_and_external_variants() {
        let y: Stream =
            serde_json::from_str(r#"{ "ytId": "aqz-KE-bpKQ", "title": "Big Buck Bunny" }"#)
                .unwrap();
        assert!(matches!(y.source, StreamSource::YouTube { .. }));
        let e: Stream =
            serde_json::from_str(r#"{ "externalUrl": "https://www.netflix.com/title/70136120" }"#)
                .unwrap();
        assert!(matches!(e.source, StreamSource::External { .. }));
    }

    #[test]
    fn unsupported_stream_shapes_are_skipped_in_lists() {
        let r: StreamsResponse = serde_json::from_str(
            r#"{ "streams": [
                { "url": "https://ok.example/a.mp4" },
                { "nzbUrl": "https://nzb.example/x.nzb" },
                { "bogus": true },
                { "infoHash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }
            ] }"#,
        )
        .unwrap();
        assert_eq!(r.streams.len(), 2);
    }

    #[test]
    fn cinemeta_video_uses_name_and_number() {
        let v: Video = serde_json::from_str(
            r#"{ "name": "Pilot", "season": 1, "number": 1, "episode": 1,
                 "firstAired": "1994-09-22T00:00:00.000Z",
                 "id": "tt0108778:1:1",
                 "released": "1994-09-22T00:00:00.000Z",
                 "thumbnail": "https://episodes.metahub.space/tt0108778/1/1/w780.jpg" }"#,
        )
        .unwrap();
        assert_eq!(v.title.as_deref(), Some("Pilot"));
        assert_eq!(v.season, Some(1));
        assert_eq!(v.episode, Some(1));
    }

    #[test]
    fn meta_item_lenient_parsing() {
        let m: MetaResponse = serde_json::from_str(
            r#"{ "meta": {
                "id": "tt0108778", "type": "series", "name": "Friends",
                "imdbRating": 8.9, "runtime": "22m", "releaseInfo": 1994,
                "director": null, "poster": "",
                "behaviorHints": { "defaultVideoId": null, "hasScheduledVideos": true },
                "videos": [
                    { "id": "tt0108778:1:1", "name": "Pilot", "season": 1, "episode": 1 },
                    { "bad": "entry" }
                ],
                "links": [{ "name": "IMDb", "category": "imdb", "url": "https://imdb.com/title/tt0108778" }]
            } }"#,
        )
        .unwrap();
        let meta = m.meta;
        assert_eq!(meta.imdb_rating.as_deref(), Some("8.9"));
        assert_eq!(meta.release_info.as_deref(), Some("1994"));
        assert_eq!(meta.poster, None);
        assert_eq!(meta.videos.len(), 1);
        assert_eq!(meta.default_video_id(), None);
        assert!(meta.behavior_hints.has_scheduled_videos);
    }

    #[test]
    fn movie_meta_default_video_id_is_meta_id() {
        let m: MetaItem = serde_json::from_str(
            r#"{ "id": "tt1254207", "type": "movie", "name": "Big Buck Bunny" }"#,
        )
        .unwrap();
        assert_eq!(m.default_video_id(), Some("tt1254207"));
    }

    #[test]
    fn catalog_response_skips_bad_items_and_keeps_full_meta_objects() {
        let r: CatalogResponse = serde_json::from_str(
            r#"{ "metas": [
                { "id": "tt1", "type": "movie", "name": "A", "poster": "https://x/p.jpg",
                  "awards": "many", "popularity": 12.3,
                  "trailerStreams": [{"title": "T", "ytId": "x"}] },
                { "type": "movie" }
            ], "cacheMaxAge": 300 }"#,
        )
        .unwrap();
        assert_eq!(r.metas.len(), 1);
        assert_eq!(r.metas[0].name, "A");
    }

    #[test]
    fn subtitles_response_with_extra_fields() {
        let r: SubtitlesResponse = serde_json::from_str(
            r#"{ "subtitles": [
                { "id": "3710297", "url": "https://subs5.strem.io/x.vtt", "lang": "eng",
                  "SubEncoding": "UTF-8", "m": "x", "season": 1 }
            ] }"#,
        )
        .unwrap();
        assert_eq!(r.subtitles.len(), 1);
        assert_eq!(r.subtitles[0].lang, "eng");
    }
}
