//! External mpv JSON-IPC backend against a real mpv.
//!
//! The real-mpv tests are `#[ignore]` because CI runners have no mpv/ffmpeg. Run
//! them locally with:
//!
//! ```text
//! cargo test -p zeiler-player --test external_mpv -- --ignored
//! ```
//!
//! They need `mpv` and `ffmpeg` on PATH (override with `ZEILER_TEST_MPV`). mpv runs
//! headless (`--vo=null --ao=null --no-config`); the media is generated with ffmpeg's
//! lavfi sources and served by a tiny HTTP server in this file, which also records
//! the request headers so the test can check what mpv sent.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use zeiler_player::external_ipc::{EventSink, ExternalError, ExternalIpcPlayer};
use zeiler_player::{PlaybackRequest, PlayerEvent, SessionError};

const WAIT: Duration = Duration::from_secs(20);

fn mpv() -> PathBuf {
    std::env::var_os("ZEILER_TEST_MPV")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("mpv"))
}

fn headless() -> Vec<String> {
    [
        "--no-config",
        "--vo=null",
        "--ao=null",
        "--msg-level=all=warn",
    ]
    .map(String::from)
    .to_vec()
}

fn sink() -> (EventSink, mpsc::UnboundedReceiver<PlayerEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let sink: EventSink = Arc::new(move |event| {
        let _ = tx.send(event);
    });
    (sink, rx)
}

/// Receive events until `pred` matches one; returns everything seen up to it.
async fn wait_for(
    rx: &mut mpsc::UnboundedReceiver<PlayerEvent>,
    what: &str,
    pred: impl Fn(&PlayerEvent) -> bool,
) -> Vec<PlayerEvent> {
    let mut seen = Vec::new();
    let found = tokio::time::timeout(WAIT, async {
        while let Some(event) = rx.recv().await {
            let hit = pred(&event);
            seen.push(event);
            if hit {
                return true;
            }
        }
        false
    })
    .await;
    assert!(
        matches!(found, Ok(true)),
        "timed out waiting for {what}; saw {seen:?}"
    );
    seen
}

fn is_property(event: &PlayerEvent, name: &str, pred: impl Fn(&Value) -> bool) -> bool {
    matches!(event, PlayerEvent::Property { name: n, value } if n == name && pred(value))
}

/// A 6-second test pattern with a tone, in Matroska.
fn generate_media(dir: &Path) -> PathBuf {
    let path = dir.join("fixture.mkv");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=6:size=320x240:rate=25",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=6",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-metadata:s:a:0",
            "language=eng",
        ])
        .arg(&path)
        .status()
        .expect("ffmpeg runs");
    assert!(status.success(), "ffmpeg failed");
    path
}

/// Serves one file over HTTP/1.1 with Range support; records every request's
/// header lines.
struct Server {
    port: u16,
    requests: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Server {
    fn start(file: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let body = Arc::new(std::fs::read(file).unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let body = Arc::clone(&body);
                let log = Arc::clone(&log);
                std::thread::spawn(move || {
                    let _ = serve(stream, &body, &log);
                });
            }
        });
        Self { port, requests }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/fixture.mkv", self.port)
    }

    fn saw_header(&self, wanted: &str) -> bool {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|line| line.eq_ignore_ascii_case(wanted))
    }
}

fn serve(stream: TcpStream, body: &[u8], log: &Mutex<Vec<Vec<String>>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let line = line.trim_end().to_string();
        if line.is_empty() {
            break;
        }
        lines.push(line);
    }
    let start = lines
        .iter()
        .find_map(|l| {
            let lower = l.to_ascii_lowercase();
            lower
                .strip_prefix("range: bytes=")
                .and_then(|r| r.split('-').next())
                .and_then(|s| s.parse::<usize>().ok())
        })
        .unwrap_or(0)
        .min(body.len());
    let ranged = lines
        .iter()
        .any(|l| l.to_ascii_lowercase().starts_with("range:"));
    log.lock().unwrap().push(lines);
    let mut out = stream;
    let part = &body[start..];
    let head = if ranged {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: video/x-matroska\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {start}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len() - 1,
            body.len(),
            part.len()
        )
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: video/x-matroska\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            part.len()
        )
    };
    out.write_all(head.as_bytes())?;
    out.write_all(part)?;
    // drain so the peer never sees a reset before reading everything
    let _ = out.shutdown(std::net::Shutdown::Write);
    let _ = reader.read_to_end(&mut Vec::new());
    Ok(())
}

fn temp_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeiler-ipc-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs mpv and ffmpeg"]
async fn plays_controls_and_shuts_down_over_json_ipc() {
    let dir = temp_dir("play");
    let server = Server::start(generate_media(&dir));
    let (events, mut rx) = sink();
    let request = PlaybackRequest {
        url: server.url(),
        title: Some("Fixture".into()),
        headers: vec![("X-Zeiler-Test".into(), "yes, with a comma".into())],
        user_agent: Some("ZeilerTest/1".into()),
        referrer: Some("https://referrer.example/".into()),
        start_at_sec: Some(1.0),
        options: vec![("alang".into(), "eng".into())],
    };

    let player = ExternalIpcPlayer::start_with_args(&mpv(), &headless(), &request, events)
        .await
        .expect("mpv starts and accepts the file");

    let seen = wait_for(&mut rx, "file-loaded", |e| *e == PlayerEvent::FileLoaded).await;
    assert!(seen.contains(&PlayerEvent::StartFile), "{seen:?}");
    wait_for(&mut rx, "a 6 s duration", |e| {
        is_property(e, "duration", |v| {
            v.as_f64().is_some_and(|d| (d - 6.0).abs() < 0.5)
        })
    })
    .await;
    wait_for(&mut rx, "a track list as real JSON", |e| {
        is_property(e, "track-list", |v| {
            v.as_array().is_some_and(|tracks| {
                tracks
                    .iter()
                    .any(|t| t["type"] == "audio" && t["lang"] == "eng")
            })
        })
    })
    .await;

    // headers, user agent and referrer went over IPC and reached the server
    assert!(server.saw_header("X-Zeiler-Test: yes, with a comma"));
    assert!(server.saw_header("User-Agent: ZeilerTest/1"));
    assert!(server.saw_header("Referer: https://referrer.example/"));

    // typed reads, in the embedded backend's shapes
    let version = player.get_property("mpv-version").await.unwrap();
    assert!(
        version.as_str().is_some_and(|v| v.starts_with("mpv")),
        "{version}"
    );
    let aid = player.get_property("aid").await.unwrap();
    assert_eq!(aid, json!("1"), "track ids read as mpv's string form");
    assert_eq!(player.get_property("alang").await.unwrap(), json!(["eng"]));
    assert!(matches!(
        player.get_property("no-such-property").await,
        Err(SessionError::Mpv(m)) if m.contains("property")
    ));

    // the start offset applied, and time-pos is throttled
    wait_for(&mut rx, "time-pos past the start offset", |e| {
        is_property(e, "time-pos", |v| v.as_f64().is_some_and(|t| t >= 1.0))
    })
    .await;

    player.set_property("pause", json!(true)).await.unwrap();
    wait_for(&mut rx, "pause=true", |e| {
        is_property(e, "pause", |v| *v == json!(true))
    })
    .await;
    player
        .command(vec!["seek".into(), "4".into(), "absolute".into()])
        .await
        .unwrap();
    wait_for(&mut rx, "a seek", |e| *e == PlayerEvent::Seek).await;
    wait_for(&mut rx, "time-pos at the seek target while paused", |e| {
        is_property(e, "time-pos", |v| {
            v.as_f64().is_some_and(|t| (t - 4.0).abs() < 0.6)
        })
    })
    .await;
    assert!(player.set_property("speed", json!("fast")).await.is_err());

    // a second file in the same window
    let again = PlaybackRequest {
        url: server.url(),
        title: Some("Again".into()),
        ..PlaybackRequest::default()
    };
    player.load(&again).await.unwrap();
    wait_for(&mut rx, "media-title of the second load", |e| {
        is_property(e, "media-title", |v| *v == json!("Again"))
    })
    .await;
    wait_for(&mut rx, "the second file loaded", |e| {
        *e == PlayerEvent::FileLoaded
    })
    .await;

    // mpv exiting on its own (as when the user closes its window) ends the session
    player.command(vec!["quit".into()]).await.ok();
    let tail = wait_for(&mut rx, "shutdown", |e| *e == PlayerEvent::Shutdown).await;
    assert_eq!(
        tail.iter().filter(|e| **e == PlayerEvent::Shutdown).count(),
        1
    );
    // nothing after Shutdown, and the connection is closed for good
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(rx.try_recv().is_err(), "Shutdown is the last event");
    assert!(matches!(
        player.get_property("volume").await,
        Err(SessionError::Closed)
    ));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs mpv"]
async fn a_failing_stream_reports_an_error_and_quit_shuts_down() {
    let (events, mut rx) = sink();
    // nothing listens on port 9 (discard) on a test machine
    let request = PlaybackRequest {
        url: "http://127.0.0.1:9/missing.mkv".into(),
        ..PlaybackRequest::default()
    };
    let player = ExternalIpcPlayer::start_with_args(&mpv(), &headless(), &request, events)
        .await
        .expect("mpv starts; the failure arrives as an event");
    let seen = wait_for(&mut rx, "end-file error", |e| {
        matches!(
            e,
            PlayerEvent::EndFile {
                reason: "error",
                ..
            }
        )
    })
    .await;
    assert!(
        seen.iter().any(|e| matches!(e, PlayerEvent::Error { message } if message.starts_with("the stream could not be played"))),
        "{seen:?}"
    );

    player.quit();
    wait_for(&mut rx, "shutdown after quit", |e| {
        *e == PlayerEvent::Shutdown
    })
    .await;
}

/// No mpv needed: a binary that exits at once never opens the IPC endpoint, which
/// is the case the shell falls back to the stdin launch for.
#[cfg(unix)]
#[tokio::test]
async fn a_player_that_never_listens_is_reported_as_ipc_unavailable() {
    let (events, _rx) = sink();
    let request = PlaybackRequest {
        url: "http://127.0.0.1:9/x.mkv".into(),
        ..PlaybackRequest::default()
    };
    let result = ExternalIpcPlayer::start_with_args(Path::new("true"), &[], &request, events).await;
    assert!(
        matches!(&result, Err(ExternalError::IpcUnavailable(m)) if m.contains("exited")),
        "{:?}",
        result.err()
    );
}

#[tokio::test]
async fn a_missing_binary_fails_to_spawn() {
    let (events, _rx) = sink();
    let result = ExternalIpcPlayer::start_with_args(
        Path::new("/definitely/not/mpv"),
        &[],
        &PlaybackRequest::default(),
        events,
    )
    .await;
    assert!(matches!(result, Err(ExternalError::Spawn(_))));
}
