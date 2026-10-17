//! Skip segments — intros, recaps and credits — for an episode or a movie, from two
//! community databases:
//!
//! - **AniSkip** (`api.aniskip.com`) for anime, by MyAnimeList id and episode number. A
//!   Kitsu id (the anime metadata addon's) is turned into a MyAnimeList id first through
//!   Kitsu's own mappings (`kitsu.app`).
//! - **TheIntroDB** (`api.theintrodb.org`) for IMDb and TMDB ids, by season and episode
//!   (or none, for a movie).
//!
//! Which one is asked follows from the video id ([`route`]): `kitsu:`/`mal:` ids go to
//! AniSkip, `tt…`/`tmdb:` ids to TheIntroDB; anything else is not looked up. A chapter
//! list in the file itself is the UI's third source (src/lib/skip.ts), used only when
//! these find nothing.
//!
//! Answers are cached (negative ones too) so a title's episodes, or the same episode
//! played again, do not ask twice; a failed request is not cached. Nothing here ever
//! fails loudly: no answer is an empty list, so playback never waits on these services.
//!
//! Ported from Harbor's `src/lib/skip-intro/{aniskip,theintrodb}.ts` (MIT) — see
//! THIRD-PARTY-NOTICES.md.

mod aniskip;
mod cache;
mod introdb;

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tracing::warn;
use url::Url;

use crate::cache::Lru;

/// What a segment covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    Intro,
    Recap,
    /// Credits (and a preview of the next episode after them).
    Outro,
}

/// Where a segment came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentSource {
    AniSkip,
    IntroDb,
}

/// One skippable stretch of the video, in seconds.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub kind: SegmentKind,
    pub start_sec: f64,
    pub end_sec: f64,
    pub source: SegmentSource,
}

/// A segment as a service gave it: a missing start is the beginning, a missing end the
/// end of the video (TheIntroDB leaves credits open-ended).
#[derive(Debug, Clone, PartialEq)]
struct RawSegment {
    kind: SegmentKind,
    start: Option<f64>,
    end: Option<f64>,
    source: SegmentSource,
}

/// The lookup a video id leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// AniSkip, after Kitsu maps the id to MyAnimeList's.
    Kitsu { id: u64, episode: u32 },
    /// AniSkip directly.
    Mal { id: u64, episode: u32 },
    /// TheIntroDB by IMDb id (`tt…`); no season and episode for a movie.
    Imdb {
        id: String,
        season: Option<u32>,
        episode: Option<u32>,
    },
    /// TheIntroDB by TMDB id.
    Tmdb {
        id: u64,
        season: Option<u32>,
        episode: Option<u32>,
    },
}

/// The lookup for a video: its id (`kitsu:<id>[:<ep>]`, `mal:<id>[:<ep>]`,
/// `tt<n>[:<season>:<episode>]`, `tmdb:<id>[:<season>:<episode>]`), or the title's when
/// the video has none (a movie's video id is often the title's). An anime movie, with no
/// episode number, is episode 1, as AniSkip files it.
pub fn route(meta_id: &str, video_id: &str) -> Option<Lookup> {
    let id = if video_id.is_empty() {
        meta_id
    } else {
        video_id
    };
    let parts: Vec<&str> = id.split(':').collect();
    let number = |s: &str| -> Option<u64> {
        if s.is_empty() || s.len() > 12 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let small = |s: &str| number(s).and_then(|n| u32::try_from(n).ok());
    match parts.as_slice() {
        ["kitsu", id] => Some(Lookup::Kitsu {
            id: number(id)?,
            episode: 1,
        }),
        ["kitsu", id, episode] => Some(Lookup::Kitsu {
            id: number(id)?,
            episode: small(episode).filter(|e| *e > 0)?,
        }),
        ["mal", id] => Some(Lookup::Mal {
            id: number(id)?,
            episode: 1,
        }),
        ["mal", id, episode] => Some(Lookup::Mal {
            id: number(id)?,
            episode: small(episode).filter(|e| *e > 0)?,
        }),
        [imdb] if is_imdb(imdb) => Some(Lookup::Imdb {
            id: (*imdb).to_string(),
            season: None,
            episode: None,
        }),
        [imdb, season, episode] if is_imdb(imdb) => Some(Lookup::Imdb {
            id: (*imdb).to_string(),
            season: Some(small(season)?),
            episode: Some(small(episode)?),
        }),
        ["tmdb", id] => Some(Lookup::Tmdb {
            id: number(id)?,
            season: None,
            episode: None,
        }),
        ["tmdb", id, season, episode] => Some(Lookup::Tmdb {
            id: number(id)?,
            season: Some(small(season)?),
            episode: Some(small(episode)?),
        }),
        _ => None,
    }
}

fn is_imdb(id: &str) -> bool {
    id.len() > 2
        && id.len() <= 12
        && id.starts_with("tt")
        && id[2..].bytes().all(|b| b.is_ascii_digit())
}

/// The services' base URLs (tests point them at a loopback server).
#[derive(Debug, Clone)]
pub struct Endpoints {
    /// AniSkip API v2, ending in `/`.
    pub aniskip: Url,
    /// Kitsu's JSON:API, ending in `/`.
    pub kitsu: Url,
    /// TheIntroDB API v4, ending in `/`.
    pub introdb: Url,
}

impl Default for Endpoints {
    fn default() -> Self {
        let url = |s: &str| Url::parse(s).expect("a valid built-in URL");
        Self {
            aniskip: url("https://api.aniskip.com/v2/"),
            kitsu: url("https://kitsu.app/api/edge/"),
            introdb: url("https://api.theintrodb.org/v4/"),
        }
    }
}

/// Per request: the lookups run while the video plays and are never waited on.
const TIMEOUT: Duration = Duration::from_secs(8);
/// No answer from these services is anywhere near this big.
const MAX_BODY_BYTES: usize = 1024 * 1024;
const CACHE_ENTRIES: usize = 256;

/// Asks AniSkip and TheIntroDB, with caches.
pub struct SkipClient {
    http: reqwest::Client,
    endpoints: Endpoints,
    /// Kitsu id → MyAnimeList id (`None`: Kitsu knows no mapping).
    mal_ids: Mutex<Lru<u64, Option<u64>>>,
    /// A lookup's raw segments (`segments_key`).
    segments: Mutex<Lru<String, Vec<RawSegment>>>,
}

/// A request that failed (not cached: the next playback asks again).
#[derive(Debug)]
pub(crate) enum FetchError {
    Http(reqwest::Error),
    Status(u16),
    TooLarge,
    Json(serde_json::Error),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "{e}"),
            Self::Status(status) => write!(f, "HTTP {status}"),
            Self::TooLarge => write!(f, "the answer is too large"),
            Self::Json(e) => write!(f, "unexpected answer: {e}"),
        }
    }
}

impl SkipClient {
    /// A client for the real services, sending `user_agent` (`Zeiler/<version>`).
    pub fn new(user_agent: &str) -> Result<Self, reqwest::Error> {
        Self::build(user_agent, Endpoints::default(), false)
    }

    /// A client for other endpoints that ignores proxy settings (tests).
    pub fn with_endpoints(endpoints: Endpoints) -> Result<Self, reqwest::Error> {
        Self::build("Zeiler/test", endpoints, true)
    }

    fn build(
        user_agent: &str,
        endpoints: Endpoints,
        no_proxy: bool,
    ) -> Result<Self, reqwest::Error> {
        let mut builder = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(TIMEOUT)
            .connect_timeout(TIMEOUT);
        if no_proxy {
            builder = builder.no_proxy();
        }
        Ok(Self {
            http: builder.build()?,
            endpoints,
            mal_ids: Mutex::new(Lru::new(CACHE_ENTRIES)),
            segments: Mutex::new(Lru::new(CACHE_ENTRIES)),
        })
    }

    /// The skip segments for `lookup` in a video `duration_sec` long, sorted by start.
    /// Empty when the services know none, or could not be reached.
    pub async fn segments(&self, lookup: &Lookup, duration_sec: f64) -> Vec<Segment> {
        let key = segments_key(lookup, duration_sec);
        let cached = self.segments.lock().unwrap().get(&key).cloned();
        let raw = match cached {
            Some(raw) => raw,
            None => match self.fetch(lookup, duration_sec).await {
                Ok(raw) => {
                    self.segments.lock().unwrap().put(key, raw.clone());
                    raw
                }
                Err(e) => {
                    warn!(?lookup, error = %e, "skip segment lookup failed");
                    Vec::new()
                }
            },
        };
        finalize(raw, duration_sec)
    }

    async fn fetch(
        &self,
        lookup: &Lookup,
        duration_sec: f64,
    ) -> Result<Vec<RawSegment>, FetchError> {
        match lookup {
            Lookup::Kitsu { id, episode } => match self.mal_id(*id).await? {
                Some(mal) => aniskip::segments(self, mal, *episode, duration_sec).await,
                None => Ok(Vec::new()),
            },
            Lookup::Mal { id, episode } => {
                aniskip::segments(self, *id, *episode, duration_sec).await
            }
            Lookup::Imdb {
                id,
                season,
                episode,
            } => {
                let id = ("imdb_id", id.clone());
                introdb::segments(self, id, *season, *episode, duration_sec).await
            }
            Lookup::Tmdb {
                id,
                season,
                episode,
            } => {
                let id = ("tmdb_id", id.to_string());
                introdb::segments(self, id, *season, *episode, duration_sec).await
            }
        }
    }

    async fn mal_id(&self, kitsu: u64) -> Result<Option<u64>, FetchError> {
        if let Some(known) = self.mal_ids.lock().unwrap().get(&kitsu) {
            return Ok(*known);
        }
        let mal = aniskip::mal_id(self, kitsu).await?;
        self.mal_ids.lock().unwrap().put(kitsu, mal);
        Ok(mal)
    }

    /// GET `url` as JSON. `Ok(None)` is a definite "not found" (404), worth caching.
    pub(crate) async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: Url,
    ) -> Result<Option<T>, FetchError> {
        let mut response = self.http.get(url).send().await.map_err(FetchError::Http)?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_BODY_BYTES as u64)
        {
            return Err(FetchError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(FetchError::Http)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(FetchError::Json)
    }

    pub(crate) fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }
}

/// Both services answer for the video's length (AniSkip matches submissions by episode
/// length, TheIntroDB picks the release version, such as an extended cut), so answers are
/// cached per length (to the second).
fn segments_key(lookup: &Lookup, duration_sec: f64) -> String {
    let length = whole(duration_sec);
    match lookup {
        Lookup::Kitsu { id, episode } => format!("kitsu:{id}:{episode}:{length}"),
        Lookup::Mal { id, episode } => format!("mal:{id}:{episode}:{length}"),
        Lookup::Imdb {
            id,
            season,
            episode,
        } => format!("imdb:{id}:{season:?}:{episode:?}:{length}"),
        Lookup::Tmdb {
            id,
            season,
            episode,
        } => format!("tmdb:{id}:{season:?}:{episode:?}:{length}"),
    }
}

fn whole(duration_sec: f64) -> i64 {
    if duration_sec.is_finite() {
        duration_sec.round() as i64
    } else {
        0
    }
}

/// Usable segments in a video `duration_sec` long (0: not known), sorted by start: an
/// open start is 0 and an open end the duration; ends are clamped to the duration;
/// segments that are empty, reversed, not finite or start past the end are dropped.
fn finalize(raw: Vec<RawSegment>, duration_sec: f64) -> Vec<Segment> {
    let duration = (duration_sec.is_finite() && duration_sec > 0.0).then_some(duration_sec);
    let mut out: Vec<Segment> = raw
        .into_iter()
        .filter_map(|r| {
            let start = r.start.unwrap_or(0.0);
            let mut end = r.end.or(duration)?;
            if !start.is_finite() || !end.is_finite() {
                return None;
            }
            let start = start.max(0.0);
            if let Some(duration) = duration {
                if start >= duration {
                    return None;
                }
                end = end.min(duration);
            }
            (end > start).then_some(Segment {
                kind: r.kind,
                start_sec: start,
                end_sec: end,
                source: r.source,
            })
        })
        .collect();
    out.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    out
}

#[cfg(test)]
mod tests;
