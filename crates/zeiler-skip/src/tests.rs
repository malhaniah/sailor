use std::sync::{Arc, Mutex};

use super::*;

#[test]
fn video_ids_route_to_their_service() {
    assert_eq!(
        route("kitsu:1", "kitsu:1:5"),
        Some(Lookup::Kitsu { id: 1, episode: 5 })
    );
    assert_eq!(
        route("kitsu:7442", "kitsu:7442"),
        Some(Lookup::Kitsu {
            id: 7442,
            episode: 1
        }),
        "an anime movie is episode 1"
    );
    assert_eq!(
        route("mal:21", "mal:21:1000"),
        Some(Lookup::Mal {
            id: 21,
            episode: 1000
        })
    );
    assert_eq!(
        route("tt0903747", "tt0903747:1:2"),
        Some(Lookup::Imdb {
            id: "tt0903747".into(),
            season: Some(1),
            episode: Some(2)
        })
    );
    assert_eq!(
        route("tt0111161", ""),
        Some(Lookup::Imdb {
            id: "tt0111161".into(),
            season: None,
            episode: None
        }),
        "no video id: the title's"
    );
    assert_eq!(
        route("tmdb:1396", "tmdb:1396:1:1"),
        Some(Lookup::Tmdb {
            id: 1396,
            season: Some(1),
            episode: Some(1)
        })
    );
    for (meta, video) in [
        ("yt_id:UC1", "yt_id:UC1:abc"),
        ("anilist:1", "anilist:1:1"),
        ("kitsu:x", "kitsu:x:1"),
        ("kitsu:1", "kitsu:1:0"),
        ("kitsu:1", "kitsu:1:-1"),
        ("tt12", "tt12:1"),
        ("tt12", "tt12:a:b"),
        ("ttx", "ttx"),
        ("tt", "tt"),
        ("kitsu:1", "kitsu:1:99999999999999"),
        ("", ""),
    ] {
        assert_eq!(route(meta, video), None, "{video}");
    }
}

fn raw(kind: SegmentKind, start: Option<f64>, end: Option<f64>) -> RawSegment {
    RawSegment {
        kind,
        start,
        end,
        source: SegmentSource::IntroDb,
    }
}

#[test]
fn segments_are_clamped_to_the_video_and_bad_ones_dropped() {
    let got = finalize(
        vec![
            raw(SegmentKind::Outro, Some(1300.0), None), // open end: the duration
            raw(SegmentKind::Recap, None, Some(30.0)),   // open start: 0
            raw(SegmentKind::Intro, Some(90.0), Some(60.0)), // reversed
            raw(SegmentKind::Intro, Some(40.0), Some(40.0)), // empty
            raw(SegmentKind::Outro, Some(1500.0), Some(1600.0)), // past the end
            raw(SegmentKind::Intro, Some(100.0), Some(190.0)),
            raw(SegmentKind::Outro, Some(1350.0), Some(9999.0)), // clamped
            raw(SegmentKind::Intro, Some(f64::NAN), Some(5.0)),
        ],
        1420.0,
    );
    let spans: Vec<_> = got
        .iter()
        .map(|s| (s.kind, s.start_sec, s.end_sec))
        .collect();
    assert_eq!(
        spans,
        [
            (SegmentKind::Recap, 0.0, 30.0),
            (SegmentKind::Intro, 100.0, 190.0),
            (SegmentKind::Outro, 1300.0, 1420.0),
            (SegmentKind::Outro, 1350.0, 1420.0),
        ]
    );
    // no known duration: an open end cannot be placed, the rest stays as given
    let unknown = finalize(
        vec![
            raw(SegmentKind::Outro, Some(1300.0), None),
            raw(SegmentKind::Intro, Some(10.0), Some(20.0)),
        ],
        0.0,
    );
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].kind, SegmentKind::Intro);
}

#[test]
fn segments_serialize_for_the_ui() {
    let segment = Segment {
        kind: SegmentKind::Outro,
        start_sec: 1.5,
        end_sec: 2.0,
        source: SegmentSource::AniSkip,
    };
    assert_eq!(
        serde_json::to_value(segment).unwrap(),
        serde_json::json!({"kind": "outro", "startSec": 1.5, "endSec": 2.0, "source": "aniskip"})
    );
}

#[test]
fn theintrodb_is_asked_on_v4() {
    assert_eq!(
        Endpoints::default().introdb.join("media").unwrap().as_str(),
        "https://api.theintrodb.org/v4/media",
        "v2 and v3 are frozen and being retired"
    );
}

/// A loopback HTTP server: answers a request whose path and query start with a route's
/// prefix with that route's status and body (404 otherwise), and logs every request.
struct Loopback {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Loopback {
    async fn start(routes: &[(&str, u16, &str)]) -> Loopback {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: Vec<(String, u16, String)> = routes
            .iter()
            .map(|(prefix, status, body)| ((*prefix).to_owned(), *status, (*body).to_owned()))
            .collect();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = Arc::clone(&requests);
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let routes = routes.clone();
                let log = Arc::clone(&log);
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
                    let (status, body) = routes
                        .iter()
                        .find(|(prefix, _, _)| path.starts_with(prefix.as_str()))
                        .map_or((404, String::new()), |(_, status, body)| {
                            (*status, body.clone())
                        });
                    let response = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Loopback { base, requests }
    }

    fn client(&self) -> SkipClient {
        let url = |path: &str| Url::parse(&format!("{}/{path}", self.base)).unwrap();
        SkipClient::with_endpoints(Endpoints {
            aniskip: url("aniskip/v2/"),
            kitsu: url("kitsu/api/edge/"),
            introdb: url("introdb/v4/"),
        })
        .unwrap()
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

const MAPPINGS: &str =
    r#"{"data":[{"attributes":{"externalSite":"myanimelist/anime","externalId":"21"}}]}"#;
const SKIP_TIMES: &str = r#"{"found":true,"results":[
    {"interval":{"startTime":13.891,"endTime":123.834},"skipType":"op"},
    {"interval":{"startTime":1380,"endTime":1500},"skipType":"ed"}]}"#;

#[tokio::test]
async fn a_kitsu_episode_is_asked_of_aniskip_by_its_myanimelist_id_once() {
    let server = Loopback::start(&[
        ("/kitsu/api/edge/anime/12/mappings?", 200, MAPPINGS),
        ("/aniskip/v2/skip-times/21/5?", 200, SKIP_TIMES),
    ])
    .await;
    let client = server.client();
    let lookup = route("kitsu:12", "kitsu:12:5").unwrap();
    let segments = client.segments(&lookup, 1420.25).await;
    assert_eq!(
        segments,
        [
            Segment {
                kind: SegmentKind::Intro,
                start_sec: 13.891,
                end_sec: 123.834,
                source: SegmentSource::AniSkip
            },
            Segment {
                kind: SegmentKind::Outro,
                start_sec: 1380.0,
                end_sec: 1420.25,
                source: SegmentSource::AniSkip
            },
        ]
    );
    let requests = server.requests();
    assert_eq!(
        requests,
        [
            "/kitsu/api/edge/anime/12/mappings?filter%5BexternalSite%5D=myanimelist%2Fanime",
            "/aniskip/v2/skip-times/21/5?types=op&types=ed&types=mixed-op&types=mixed-ed&types=recap&episodeLength=1420.250",
        ]
    );
    // the same episode again, and the next one: the mapping and the answer are cached
    assert_eq!(client.segments(&lookup, 1420.4).await.len(), 2);
    let next = route("kitsu:12", "kitsu:12:6").unwrap();
    assert!(client.segments(&next, 1420.0).await.is_empty(), "404");
    assert!(client.segments(&next, 1420.0).await.is_empty());
    assert_eq!(
        server.requests()[2..],
        [
            "/aniskip/v2/skip-times/21/6?types=op&types=ed&types=mixed-op&types=mixed-ed&types=recap&episodeLength=1420.000"
        ],
        "one request for episode 6 (its 404 is cached), none for the mapping"
    );
}

#[tokio::test]
async fn failures_are_empty_and_not_cached() {
    let server = Loopback::start(&[
        ("/introdb/v4/media?imdb_id=tt1&", 503, "busy"),
        ("/introdb/v4/media?imdb_id=tt2", 200, "not json"),
        ("/kitsu/api/edge/anime/3/", 200, r#"{"data":[]}"#),
    ])
    .await;
    let client = server.client();
    let broken = route("tt1", "tt1:1:1").unwrap();
    assert!(client.segments(&broken, 3000.0).await.is_empty());
    assert!(client.segments(&broken, 3000.0).await.is_empty());
    let garbled = route("tt2", "tt2").unwrap();
    assert!(client.segments(&garbled, 3000.0).await.is_empty());
    // Kitsu knows no MyAnimeList id: AniSkip is never asked, and that is remembered
    let unmapped = route("kitsu:3", "kitsu:3:1").unwrap();
    assert!(client.segments(&unmapped, 1400.0).await.is_empty());
    assert!(client.segments(&unmapped, 1400.0).await.is_empty());
    assert_eq!(
        server.requests(),
        [
            "/introdb/v4/media?imdb_id=tt1&season=1&episode=1&duration_ms=3000000",
            "/introdb/v4/media?imdb_id=tt1&season=1&episode=1&duration_ms=3000000",
            "/introdb/v4/media?imdb_id=tt2&duration_ms=3000000",
            "/kitsu/api/edge/anime/3/mappings?filter%5BexternalSite%5D=myanimelist%2Fanime",
        ]
    );
}

#[tokio::test]
async fn theintrodb_answers_for_imdb_and_tmdb_ids() {
    let server = Loopback::start(&[
        (
            "/introdb/v4/media?imdb_id=tt0903747&season=1&episode=1",
            200,
            r#"{"tvdb_id":81189,"type":"tv","season":1,"episode":1,
                "intro":[{"start_ms":228664,"end_ms":246143}],
                "credits":[{"start_ms":3431000,"end_ms":null}]}"#,
        ),
        (
            "/introdb/v4/media?tmdb_id=278",
            200,
            r#"{"tvdb_id":190,"type":"movie","credits":[{"start_ms":8300000,"end_ms":null}]}"#,
        ),
    ])
    .await;
    let client = server.client();
    let breaking_bad = route("tt0903747", "tt0903747:1:1").unwrap();
    let episode = client.segments(&breaking_bad, 3480.0).await;
    let spans: Vec<_> = episode
        .iter()
        .map(|s| (s.kind, s.start_sec, s.end_sec, s.source))
        .collect();
    assert_eq!(
        spans,
        [
            (SegmentKind::Intro, 228.664, 246.143, SegmentSource::IntroDb),
            (SegmentKind::Outro, 3431.0, 3480.0, SegmentSource::IntroDb),
        ]
    );
    let shawshank = route("tmdb:278", "tmdb:278").unwrap();
    let movie = client.segments(&shawshank, 8520.0).await;
    assert_eq!(movie.len(), 1);
    assert_eq!((movie[0].start_sec, movie[0].end_sec), (8300.0, 8520.0));
    // the same length again is cached; another cut (or none known) is asked anew
    client.segments(&breaking_bad, 3480.4).await;
    client.segments(&shawshank, 8820.0).await;
    client.segments(&shawshank, 0.0).await;
    assert_eq!(
        server.requests(),
        [
            "/introdb/v4/media?imdb_id=tt0903747&season=1&episode=1&duration_ms=3480000",
            "/introdb/v4/media?tmdb_id=278&duration_ms=8520000",
            "/introdb/v4/media?tmdb_id=278&duration_ms=8820000",
            "/introdb/v4/media?tmdb_id=278",
        ],
        "an unknown length sends no duration_ms (0 would pick the version of unknown length)"
    );
}
