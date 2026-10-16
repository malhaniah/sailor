//! The embedded (libmpv) actor, end to end on a real core with no video or audio
//! output (`Surface::Headless`): media comes from mpv's own lavfi sources or a tiny
//! WAV served over loopback HTTP, so nothing touches the network, a window or an audio
//! device.
//!
//! Needs libmpv at link and run time: Linux CI installs `libmpv-dev`; Windows CI runs
//! `node scripts/fetch-libmpv.mjs` and puts `src-tauri/libmpv` on LIB and PATH.
//!
//! ```text
//! cargo test -p zeiler-player --features embedded
//! ```
#![cfg(feature = "embedded")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zeiler_player::PlaybackRequest;
use zeiler_player::PlayerEvent;
use zeiler_player::embedded::{EmbeddedOptions, EmbeddedPlayer, Surface};

/// Generous: CI runners are slow, and lavfi sources take a moment to probe.
const TIMEOUT: Duration = Duration::from_secs(20);

fn spawn() -> (EmbeddedPlayer, Receiver<PlayerEvent>) {
    let (tx, rx) = mpsc::channel();
    let player = EmbeddedPlayer::spawn(EmbeddedOptions::new(Surface::Headless), move |event| {
        let _ = tx.send(event);
    })
    .expect("a headless libmpv core initializes");
    (player, rx)
}

fn request(url: &str) -> PlaybackRequest {
    PlaybackRequest {
        url: url.into(),
        ..PlaybackRequest::default()
    }
}

/// Wait for the first event `want` matches (other events are skipped); panics with
/// the events seen when none arrives in time.
fn wait_for<T>(
    rx: &Receiver<PlayerEvent>,
    what: &str,
    mut want: impl FnMut(&PlayerEvent) -> Option<T>,
) -> T {
    let deadline = Instant::now() + TIMEOUT;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(event) => {
                if let Some(found) = want(&event) {
                    return found;
                }
                seen.push(event);
            }
            Err(_) => panic!("no {what} within {TIMEOUT:?}; saw {seen:#?}"),
        }
    }
}

/// Every event up to and including the first one `done` matches.
fn collect_until(
    rx: &Receiver<PlayerEvent>,
    what: &str,
    mut done: impl FnMut(&PlayerEvent) -> bool,
) -> Vec<PlayerEvent> {
    let mut events = Vec::new();
    wait_for(rx, what, |e| {
        events.push(e.clone());
        done(e).then_some(())
    });
    events
}

fn property<'a>(event: &'a PlayerEvent, wanted: &str) -> Option<&'a Value> {
    match event {
        PlayerEvent::Property { name, value } if name == wanted => Some(value),
        _ => None,
    }
}

/// (a) A short lavfi tone: the file starts, loads, reports its duration and plays to
/// the end — `eof-reached` with the default keep-open (Zeiler's replay state), and an
/// `end-file` with reason `eof` once keep-open is off.
#[test]
fn a_lavfi_tone_loads_reports_its_duration_and_ends() {
    let (player, rx) = spawn();
    player
        .load(request("av://lavfi:sine=frequency=440:duration=1"))
        .unwrap();
    let events = collect_until(&rx, "eof-reached", |e| {
        property(e, "eof-reached") == Some(&json!(true))
    });
    let at = |wanted: &PlayerEvent| events.iter().position(|e| e == wanted);
    let started = at(&PlayerEvent::StartFile).expect("start-file");
    let loaded = at(&PlayerEvent::FileLoaded).expect("file-loaded");
    assert!(started < loaded, "{events:#?}");
    // lavfi may refine the duration while it plays: the last report counts
    let duration = events
        .iter()
        .filter_map(|e| property(e, "duration").and_then(Value::as_f64))
        .next_back()
        .expect("a duration");
    assert!((duration - 1.0).abs() < 0.1, "duration {duration}");

    player
        .set_property("keep-open".into(), json!("no"))
        .unwrap();
    player
        .load(request("av://lavfi:sine=frequency=440:duration=0.5"))
        .unwrap();
    wait_for(&rx, "file-loaded", |e| {
        (*e == PlayerEvent::FileLoaded).then_some(())
    });
    let reason = wait_for(&rx, "end-file", |e| match e {
        PlayerEvent::EndFile { reason, .. } if *reason != "stop" => Some(*reason),
        _ => None,
    });
    assert_eq!(reason, "eof");
    player.quit();
}

/// (b) Node properties arrive as parsed JSON: the track list of a test pattern is an
/// array holding its video track.
#[test]
fn the_track_list_arrives_as_a_json_array() {
    let (player, rx) = spawn();
    player
        .load(request("av://lavfi:testsrc=duration=2"))
        .unwrap();
    let tracks = wait_for(&rx, "a non-empty track-list", |e| {
        property(e, "track-list")
            .and_then(Value::as_array)
            .filter(|tracks| !tracks.is_empty())
            .cloned()
    });
    assert!(
        tracks.iter().any(|t| t["type"] == "video"),
        "a video track: {tracks:?}"
    );
    player.quit();
}

/// (c) Writes and commands reach the core: pausing reports `pause = true`, and an
/// absolute seek reports a seek (or the playback restart that follows it).
#[test]
fn pause_and_seek_are_reported() {
    let (player, rx) = spawn();
    player
        .load(request("av://lavfi:testsrc=duration=5"))
        .unwrap();
    wait_for(&rx, "file-loaded", |e| {
        (*e == PlayerEvent::FileLoaded).then_some(())
    });

    player.set_property("pause".into(), json!(true)).unwrap();
    wait_for(&rx, "pause = true", |e| {
        (property(e, "pause") == Some(&json!(true))).then_some(())
    });

    player
        .command(vec!["seek".into(), "1".into(), "absolute".into()])
        .unwrap();
    wait_for(&rx, "seek or playback-restart", |e| {
        matches!(e, PlayerEvent::Seek | PlayerEvent::PlaybackRestart).then_some(())
    });
    player.quit();
}

/// (d) Per-stream HTTP headers, user agent and referrer reach the server — and do not
/// leak into the next stream, which is loaded without them.
#[test]
fn request_headers_apply_to_one_stream_only() {
    let server = WavServer::start();
    let (player, rx) = spawn();

    player
        .load(PlaybackRequest {
            url: server.url("first.wav"),
            headers: vec![("X-Zeiler-Test".into(), "token, with comma".into())],
            user_agent: Some("ZeilerTest/1.0".into()),
            referrer: Some("https://referrer.example/page".into()),
            ..PlaybackRequest::default()
        })
        .unwrap();
    wait_for(&rx, "the first file to load", |e| {
        (*e == PlayerEvent::FileLoaded).then_some(())
    });
    let first = server.requests_for("/first.wav");
    assert!(!first.is_empty(), "the first stream was requested");
    for head in &first {
        assert_eq!(
            header(head, "x-zeiler-test"),
            Some("token, with comma"),
            "{head}"
        );
        assert_eq!(header(head, "user-agent"), Some("ZeilerTest/1.0"), "{head}");
        assert_eq!(
            header(head, "referer"),
            Some("https://referrer.example/page"),
            "{head}"
        );
    }

    player.load(request(&server.url("second.wav"))).unwrap();
    wait_for(&rx, "the second file to load", |e| {
        (*e == PlayerEvent::FileLoaded).then_some(())
    });
    let second = server.requests_for("/second.wav");
    assert!(!second.is_empty(), "the second stream was requested");
    for head in &second {
        assert_eq!(header(head, "x-zeiler-test"), None, "{head}");
        // libmpv's own user agent again — never the previous stream's, never empty
        let user_agent = header(head, "user-agent").unwrap_or_default();
        assert!(
            !user_agent.is_empty() && user_agent != "ZeilerTest/1.0",
            "{head}"
        );
        assert_eq!(header(head, "referer"), None, "{head}");
    }
    player.quit();
}

/// (e) Quitting shuts the core down and says so.
#[test]
fn quit_reports_shutdown() {
    let (player, rx) = spawn();
    player.quit();
    wait_for(&rx, "shutdown", |e| {
        (*e == PlayerEvent::Shutdown).then_some(())
    });
}

/// Records the event trace that the shell's library progress tests replay
/// (`src-tauri/src/library/fixtures/libmpv-trace.json`): a 30 s test pattern played in
/// real time, with what a viewer does — play 4 s, skip ahead to 0:15, play 2 s, pause,
/// skip back to 0:08 while paused, resume for 2 s, 2 s at double speed, skip to 0:28,
/// play to the end (keep-open: `eof-reached`), quit. Each entry is
/// `[ms since the load, event]`.
///
/// ```text
/// ZEILER_TRACE_OUT=src-tauri/src/library/fixtures/libmpv-trace.json \
///   cargo test -p zeiler-player --features embedded --test embedded -- --ignored record
/// ```
#[test]
#[ignore = "records a fixture: set ZEILER_TRACE_OUT"]
fn record_a_progress_trace() {
    let out = std::env::var("ZEILER_TRACE_OUT").expect("ZEILER_TRACE_OUT names the output file");
    let (player, rx) = spawn();
    let start = Instant::now();
    let mut trace: Vec<Value> = Vec::new();
    let pump = |trace: &mut Vec<Value>, wait: Duration| {
        let until = Instant::now() + wait;
        while let Ok(event) = rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
            let shutdown = event == PlayerEvent::Shutdown;
            trace.push(json!([start.elapsed().as_millis() as u64, event]));
            if shutdown {
                break;
            }
        }
    };
    let set = |name: &str, value: Value| player.set_property(name.into(), value).unwrap();
    let seek = |to: &str| {
        player
            .command(vec!["seek".into(), to.into(), "absolute".into()])
            .unwrap()
    };

    player
        .load(request(
            "av://lavfi:testsrc=duration=30:size=160x120:rate=10",
        ))
        .unwrap();
    pump(&mut trace, Duration::from_millis(4_000));
    seek("15");
    pump(&mut trace, Duration::from_millis(2_000));
    set("pause", json!(true));
    pump(&mut trace, Duration::from_millis(1_000));
    seek("8");
    pump(&mut trace, Duration::from_millis(500));
    set("pause", json!(false));
    pump(&mut trace, Duration::from_millis(2_000));
    set("speed", json!(2.0));
    pump(&mut trace, Duration::from_millis(2_000));
    set("speed", json!(1.0));
    seek("28");
    pump(&mut trace, Duration::from_millis(3_500));
    player.quit();
    pump(&mut trace, TIMEOUT);

    let text = serde_json::to_string_pretty(&trace).unwrap();
    std::fs::write(&out, text + "\n").unwrap();
}

// --- a tiny loopback HTTP server --------------------------------------------------------

/// Serves the same short silent WAV at every path and records each request head.
struct WavServer {
    port: u16,
    heads: Arc<Mutex<Vec<String>>>,
}

impl WavServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&heads);
        std::thread::spawn(move || {
            let body = silent_wav();
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let recorded = Arc::clone(&recorded);
                let body = body.clone();
                std::thread::spawn(move || serve(stream, &body, &recorded));
            }
        });
        Self { port, heads }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}/{path}", self.port)
    }

    /// The request heads received for `path` so far.
    fn requests_for(&self, path: &str) -> Vec<String> {
        let prefix = format!("GET {path} ");
        self.heads
            .lock()
            .unwrap()
            .iter()
            .filter(|head| head.starts_with(&prefix))
            .cloned()
            .collect()
    }
}

fn serve(mut stream: TcpStream, body: &[u8], heads: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return,
        }
    }
    heads
        .lock()
        .unwrap()
        .push(String::from_utf8_lossy(&head).into_owned());
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(body);
}

/// The value of header `name` (case-insensitive) in a request head.
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then_some(value.trim())
    })
}

/// Half a second of 8 kHz mono 16-bit silence as a RIFF/WAVE file.
fn silent_wav() -> Vec<u8> {
    const RATE: u32 = 8000;
    let data_len: u32 = RATE; // 0.5 s × 2 bytes per sample
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.resize(44 + data_len as usize, 0);
    wav
}
