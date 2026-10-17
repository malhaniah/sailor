//! Playback progress, tracked in Rust from the player's own events.
//!
//! `player_start` (and `player_load`) take a [`PlaybackContext`]: the title, the video
//! being played and the title's video list. The shell feeds every event of that
//! session to a [`SessionProgress`], whose [`Tracker`] turns the event stream into
//! batches of position reports ([`Flush`]) that [`apply`] writes to the library with
//! stremio-core's arithmetic (`zeiler_account`):
//!
//! - **Positions.** `time-pos` reports become `apply_playback_progress` calls; a jump
//!   (a `seek` event, a position that moved further than the wall clock allows, or a
//!   step back) becomes `apply_seek` instead, so skipping ahead never counts as time
//!   watched. Reports wait for `start-file`/`file-loaded` (anything earlier belongs to
//!   the previous file) and for a known duration (live streams have none). Only the
//!   file this playback loaded is tracked (and what it redirects to): another file
//!   opened in the same player, as external mpv allows, ends the session.
//! - **Counting.** Time watched is the distance between consecutive reports of one
//!   run, never "position minus stored offset" alone: when the stored position moved
//!   underneath (the user dismissed the title from Continue Watching or marked it
//!   watched while it plays, or a write failed), the run first re-anchors there.
//! - **Cadence.** A batch is written every 15 s while playing, on pause, at the end of
//!   the file, and when the session ends: replaced, stopped, shut down, or the app
//!   exits. Consecutive reports collapse into one (the arithmetic is cumulative), so a
//!   batch is small.
//! - **The end.** The session's last batch also runs the end-of-playback rule: stopped
//!   in the credits, the resume position is dropped and Continue Watching moves on to
//!   the next episode.
//! - **Nothing for nothing.** A session that never reported a position writes nothing,
//!   so a stream that fails to open leaves the library untouched. Live TV is never
//!   tracked.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use zeiler_account::{
    MetaSummary, VideoRef, apply_playback_progress, apply_seek, finish_playback,
    new_item_from_meta, next_video_id, prepare_playback, touch,
};
use zeiler_player::PlayerEvent;

use super::{Job, LibraryFile, check_meta, check_video_id, check_videos, refresh};
use crate::error::IpcError;

/// A batch is written at least this often while the position moves.
const FLUSH_EVERY: Duration = Duration::from_secs(15);
/// A position this far behind the last one is a seek back, not jitter.
const BACKWARD_TOLERANCE_MS: u64 = 1_500;
/// Slack on top of the wall-clock bound before a forward move counts as a seek.
const JUMP_SLACK_MS: f64 = 2_500.0;

/// What a playback is, for the library (`player_start`'s and `player_load`'s
/// `context`). Sent by the UI from the launch it plays.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackContext {
    pub meta: MetaSummary,
    /// The video played: an episode id, or the movie's own video id.
    pub video_id: String,
    /// The title's videos (empty for a movie), for the watched marks and the next
    /// episode.
    #[serde(default)]
    pub videos: Vec<VideoRef>,
}

impl PlaybackContext {
    pub(crate) fn check(&self) -> Result<(), IpcError> {
        check_meta(&self.meta)?;
        check_video_id(&self.video_id)?;
        check_videos(&self.videos)
    }
}

/// One position report, in ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    /// Playback moved on from `from` (the report before this run) to `time`: the
    /// time between counts as watched.
    Progress { from: u64, time: u64, duration: u64 },
    /// The position jumped to `time` (seek, or the first report of the file).
    Seek { time: u64, duration: u64 },
}

/// A batch of reports for [`apply`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Flush {
    /// The session's first batch: refresh the title and clear a resume position that
    /// belongs to another video before applying.
    pub first: bool,
    pub ops: Vec<Op>,
    /// The session's last batch: run the end-of-playback rule after applying.
    pub finish: bool,
}

/// Tells a playback's own file from another one opened in the same player
/// (drag-and-drop, the user's own bindings in external mpv): a `start-file` after the
/// file this playback loaded started is another file, unless the file before it ended
/// in a redirect (a playlist URL resolving to its entry is still this playback).
/// The tracker ends there, and the shell treats it as a new file (`commands::mpv`).
#[derive(Debug, Default)]
pub(crate) struct OwnFile {
    /// This playback's file started.
    started: bool,
    /// Why the last file ended (`end-file`'s reason).
    last_end: Option<&'static str>,
}

impl OwnFile {
    /// Take one player event; true when it is the `start-file` of another file.
    pub(crate) fn another_file(&mut self, event: &PlayerEvent) -> bool {
        match event {
            PlayerEvent::StartFile => {
                if self.started && self.last_end != Some("redirect") {
                    return true;
                }
                self.started = true;
            }
            // (a missed start-file)
            PlayerEvent::FileLoaded => self.started = true,
            PlayerEvent::EndFile { reason, .. } => self.last_end = Some(*reason),
            _ => {}
        }
        false
    }
}

/// Turns one session's player events into [`Flush`]es. Pure: the caller passes the
/// time in.
#[derive(Debug)]
pub(crate) struct Tracker {
    /// A file has started: positions from now on are this file's.
    armed: bool,
    /// Which `start-file` is another file.
    own: OwnFile,
    closed: bool,
    /// A batch with reports went out (later batches are not `first`).
    started: bool,
    /// A `seek` event came: the next position is the seek's target.
    seek_pending: bool,
    /// The file's duration in ms (0 while unknown).
    duration: u64,
    speed: f64,
    /// The last position (ms) and when it came.
    last: Option<(u64, Instant)>,
    ops: Vec<Op>,
    last_flush: Option<Instant>,
}

impl Default for Tracker {
    fn default() -> Self {
        Self {
            armed: false,
            own: OwnFile::default(),
            closed: false,
            started: false,
            seek_pending: false,
            duration: 0,
            speed: 1.0,
            last: None,
            ops: Vec::new(),
            last_flush: None,
        }
    }
}

/// mpv seconds as ms (`null` and nonsense: none).
fn millis(value: &Value) -> Option<u64> {
    value
        .as_f64()
        .filter(|s| s.is_finite() && *s >= 0.0)
        // `as` saturates, so even an absurd value cannot wrap
        .map(|s| (s * 1000.0).round() as u64)
}

impl Tracker {
    /// Take one player event; returns a batch when one is due.
    pub(crate) fn on_event(&mut self, event: &PlayerEvent, now: Instant) -> Option<Flush> {
        if self.closed {
            return None;
        }
        if self.own.another_file(event) {
            // another file in the same player (drag-and-drop, the user's own
            // bindings in external mpv): this playback is over
            return self.close();
        }
        match event {
            PlayerEvent::StartFile => {
                self.armed = true;
                self.last = None;
                self.duration = 0;
                self.seek_pending = false;
                self.last_flush.get_or_insert(now);
                None
            }
            PlayerEvent::FileLoaded => {
                // (a missed start-file)
                self.armed = true;
                self.last_flush.get_or_insert(now);
                None
            }
            PlayerEvent::Seek => {
                self.seek_pending = true;
                None
            }
            PlayerEvent::EndFile { .. } => self.flush(now),
            PlayerEvent::Shutdown => self.close(),
            PlayerEvent::Property { name, value } => match name.as_str() {
                "time-pos" => self.position(value, now),
                "duration" => {
                    self.duration = millis(value).unwrap_or(0);
                    None
                }
                "speed" => {
                    if let Some(speed) = value.as_f64().filter(|s| s.is_finite() && *s > 0.0) {
                        self.speed = speed;
                    }
                    None
                }
                "pause" | "eof-reached" if value == &Value::Bool(true) => self.flush(now),
                _ => None,
            },
            _ => None,
        }
    }

    fn position(&mut self, value: &Value, now: Instant) -> Option<Flush> {
        if !self.armed || self.duration == 0 {
            return None;
        }
        let time = millis(value)?;
        let previous = self.last.map(|(previous, _)| previous);
        let jumped = match self.last {
            None => true,
            Some((previous, at)) => {
                let elapsed = now.saturating_duration_since(at).as_secs_f64() * 1000.0;
                let allowed = elapsed * self.speed.max(1.0) * 1.25 + JUMP_SLACK_MS;
                self.seek_pending
                    || time.saturating_add(BACKWARD_TOLERANCE_MS) < previous
                    || time.saturating_sub(previous) as f64 > allowed
            }
        };
        self.seek_pending = false;
        self.last = Some((time, now));
        let duration = self.duration;
        // Collapse runs: a run's reports are cumulative (each counts the time since the
        // one before), so the furthest one stands for the run, from where it started;
        // of consecutive seeks only the last matters.
        match (self.ops.last_mut(), jumped) {
            (
                Some(Op::Seek {
                    time: t,
                    duration: d,
                }),
                true,
            ) => (*t, *d) = (time, duration),
            (
                Some(Op::Progress {
                    time: t,
                    duration: d,
                    ..
                }),
                false,
            ) => {
                if time >= *t {
                    (*t, *d) = (time, duration);
                }
            }
            (_, true) => self.ops.push(Op::Seek { time, duration }),
            (_, false) => self.ops.push(Op::Progress {
                from: previous.unwrap_or(time),
                time,
                duration,
            }),
        }
        if self
            .last_flush
            .is_none_or(|at| now.saturating_duration_since(at) >= FLUSH_EVERY)
        {
            self.flush(now)
        } else {
            None
        }
    }

    fn flush(&mut self, now: Instant) -> Option<Flush> {
        self.last_flush = Some(now);
        if self.ops.is_empty() {
            return None;
        }
        Some(self.batch(false))
    }

    /// The session is over: the last batch (with the end-of-playback rule), unless it
    /// never reported a position. Later events and calls are ignored.
    pub(crate) fn close(&mut self) -> Option<Flush> {
        if std::mem::replace(&mut self.closed, true) {
            return None;
        }
        if !self.started && self.ops.is_empty() {
            return None;
        }
        Some(self.batch(true))
    }

    fn batch(&mut self, finish: bool) -> Flush {
        let first = !std::mem::replace(&mut self.started, true);
        Flush {
            first,
            ops: std::mem::take(&mut self.ops),
            finish,
        }
    }
}

/// Write one batch of `context`'s session into the library. Returns the item id when
/// the item changed (and so must be persisted and announced).
pub(crate) fn apply(
    file: &mut LibraryFile,
    context: &PlaybackContext,
    flush: &Flush,
    now: DateTime<Utc>,
) -> Option<String> {
    let meta = &context.meta;
    let video_id = context.video_id.as_str();
    let previous = file.items.get(&meta.id);
    if previous.is_none() && flush.ops.is_empty() {
        return None; // nothing was ever written for this session
    }
    let mut item = match previous {
        Some(previous) => {
            let mut item = previous.clone();
            if flush.first {
                refresh(&mut item, meta);
            }
            item
        }
        None => new_item_from_meta(meta, now),
    };
    if item.is_live() {
        return None;
    }
    if flush.first {
        prepare_playback(&mut item, video_id);
    } else if item.state.video_id.as_deref() != Some(video_id) {
        // The first batch made this session's video the item's current one; since
        // then a later playback (or the end of this one) moved it on. A stale batch
        // must not take it back. (The shell closes a session before starting the next,
        // so this is a safety net.)
        return None;
    }
    for op in &flush.ops {
        match *op {
            Op::Progress {
                from,
                time,
                duration,
            } => {
                // The stored position is not where this run started: it was moved
                // underneath (dismissed from Continue Watching or marked watched while
                // playing, or an earlier batch failed to save). Re-anchor at the run's
                // start, so only what was played since counts as watched.
                if item.state.time_offset.abs_diff(from) > BACKWARD_TOLERANCE_MS {
                    apply_seek(&mut item, video_id, from, duration, &context.videos, now);
                }
                apply_playback_progress(&mut item, video_id, time, duration, &context.videos, now);
            }
            Op::Seek { time, duration } => {
                apply_seek(&mut item, video_id, time, duration, &context.videos, now);
            }
        }
    }
    // only while the item still points at this session's video (a later playback of
    // another episode owns it otherwise)
    if flush.finish && item.state.video_id.as_deref() == Some(video_id) {
        let next = next_video_id(&context.videos, video_id, now);
        finish_playback(&mut item, next.as_deref());
    }
    if !touch(previous, &mut item, now) {
        return None;
    }
    file.items.insert(meta.id.clone(), item);
    Some(meta.id.clone())
}

/// Progress tracking for one playback session: events in, batches out to the library
/// worker, in order.
pub(crate) struct SessionProgress {
    context: Arc<PlaybackContext>,
    tracker: Mutex<Tracker>,
    jobs: mpsc::UnboundedSender<Job>,
    /// Batches sent and not yet applied (shared with the worker, which counts down).
    pending: Arc<AtomicUsize>,
    /// Set by the worker once one of this session's batches is in the store.
    committed: Arc<AtomicBool>,
}

impl SessionProgress {
    /// None for live TV, which is never tracked.
    pub(crate) fn new(
        context: PlaybackContext,
        jobs: mpsc::UnboundedSender<Job>,
        pending: Arc<AtomicUsize>,
    ) -> Option<Arc<Self>> {
        if context.meta.is_live() {
            return None;
        }
        Some(Arc::new(Self {
            context: Arc::new(context),
            tracker: Mutex::new(Tracker::default()),
            jobs,
            pending,
            committed: Arc::new(AtomicBool::new(false)),
        }))
    }

    pub(crate) fn on_event(&self, event: &PlayerEvent) {
        // sent under the tracker's lock, so a concurrent `close` cannot overtake it
        let mut tracker = self.tracker.lock().unwrap();
        if let Some(flush) = tracker.on_event(event, Instant::now()) {
            self.send(flush, None);
        }
    }

    /// End tracking (the session was replaced, stopped, or the app exits): the last
    /// batch goes out. The receiver resolves once it is written; `None` when there was
    /// nothing to write (or tracking had already ended).
    pub(crate) fn close(&self) -> Option<oneshot::Receiver<()>> {
        let mut tracker = self.tracker.lock().unwrap();
        let flush = tracker.close()?;
        let (done, written) = oneshot::channel();
        self.send(flush, Some(done));
        Some(written)
    }

    fn send(&self, flush: Flush, done: Option<oneshot::Sender<()>>) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        let sent = self.jobs.send(Job {
            context: self.context.clone(),
            flush,
            committed: self.committed.clone(),
            done,
        });
        if sent.is_err() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zeiler_account::{LibraryItem, is_in_continue_watching, watched_video_ids};

    use crate::library::{LibraryQuery, LibraryState};
    use crate::persist::tests::temp_dir;

    fn prop(name: &str, value: Value) -> PlayerEvent {
        PlayerEvent::Property {
            name: name.into(),
            value,
        }
    }

    fn pos(seconds: f64) -> PlayerEvent {
        prop("time-pos", json!(seconds))
    }

    /// Drives a tracker on a fake clock.
    struct Clock {
        tracker: Tracker,
        start: Instant,
        at: Duration,
        flushes: Vec<Flush>,
    }

    impl Clock {
        fn new() -> Self {
            Self {
                tracker: Tracker::default(),
                start: Instant::now(),
                at: Duration::ZERO,
                flushes: Vec::new(),
            }
        }

        fn send(&mut self, event: PlayerEvent) {
            if let Some(flush) = self.tracker.on_event(&event, self.start + self.at) {
                self.flushes.push(flush);
            }
        }

        fn wait(&mut self, ms: u64) {
            self.at += Duration::from_millis(ms);
        }

        fn started(duration: f64) -> Self {
            let mut clock = Self::new();
            clock.send(PlayerEvent::StartFile);
            clock.send(prop("duration", json!(duration)));
            clock.send(PlayerEvent::FileLoaded);
            clock
        }

        /// Play from `from` to `to` seconds, reporting every 200 ms of real time.
        fn play(&mut self, from: f64, to: f64) {
            let mut t = from;
            while t <= to + 1e-9 {
                self.send(pos(t));
                self.wait(200);
                t += 0.2;
            }
        }
    }

    fn movie_context() -> PlaybackContext {
        serde_json::from_value(json!({
            "meta": {"id": "tt1", "type": "movie", "name": "A Movie",
                     "poster": "https://img.example/tt1.jpg"},
            "videoId": "tt1",
            "videos": []
        }))
        .unwrap()
    }

    fn series_context(episode: u32) -> PlaybackContext {
        let videos: Vec<Value> = (1..=3)
            .map(|e| json!({"id": format!("tt9:1:{e}"), "season": 1, "episode": e}))
            .collect();
        serde_json::from_value(json!({
            "meta": {"id": "tt9", "type": "series", "name": "A Show"},
            "videoId": format!("tt9:1:{episode}"),
            "videos": videos
        }))
        .unwrap()
    }

    fn now() -> DateTime<Utc> {
        zeiler_account::parse_time("2024-06-01T20:00:00Z").unwrap()
    }

    fn apply_all(file: &mut LibraryFile, context: &PlaybackContext, flushes: &[Flush]) {
        for flush in flushes {
            apply(file, context, flush, now());
        }
    }

    // --- tracker ------------------------------------------------------------------------

    #[test]
    fn steady_playback_is_one_baseline_then_progress_every_15_seconds() {
        let mut clock = Clock::started(600.0);
        clock.play(0.0, 40.0);
        // batches at 15 s and 30 s of wall time
        assert_eq!(clock.flushes.len(), 2);
        assert!(clock.flushes[0].first && !clock.flushes[1].first);
        assert_eq!(
            clock.flushes[0].ops[0],
            Op::Seek {
                time: 0,
                duration: 600_000
            }
        );
        assert!(matches!(
            clock.flushes[0].ops[..],
            [Op::Seek { .. }, Op::Progress { .. }]
        ));
        assert!(matches!(clock.flushes[1].ops[..], [Op::Progress { .. }]));
        let last = clock.tracker.close().unwrap();
        assert!(last.finish && !last.first);
        assert!(matches!(last.ops[..], [Op::Progress { time: 40_000, .. }]));
        assert_eq!(clock.tracker.close(), None, "closed once");
        clock.send(pos(41.0));
        assert_eq!(clock.flushes.len(), 2, "events after close are ignored");
    }

    #[test]
    fn jumps_are_seeks_even_without_a_seek_event() {
        let mut clock = Clock::started(600.0);
        clock.play(10.0, 12.0);
        clock.send(pos(300.0)); // +288 s within 200 ms of wall time
        clock.wait(200);
        clock.send(pos(300.2));
        clock.wait(200);
        clock.send(pos(100.0)); // back
        clock.wait(200);
        clock.send(pos(100.2));
        let flush = clock.tracker.close().unwrap();
        assert_eq!(
            flush.ops,
            [
                Op::Seek {
                    time: 10_000,
                    duration: 600_000
                },
                Op::Progress {
                    from: 10_000,
                    time: 12_000,
                    duration: 600_000
                },
                Op::Seek {
                    time: 300_000,
                    duration: 600_000
                },
                Op::Progress {
                    from: 300_000,
                    time: 300_200,
                    duration: 600_000
                },
                Op::Seek {
                    time: 100_000,
                    duration: 600_000
                },
                Op::Progress {
                    from: 100_000,
                    time: 100_200,
                    duration: 600_000
                },
            ]
        );
    }

    #[test]
    fn a_seek_event_marks_even_a_small_jump_and_pause_flushes() {
        let mut clock = Clock::started(600.0);
        clock.play(0.0, 1.0);
        clock.send(PlayerEvent::Seek);
        clock.send(pos(2.0)); // +1 s: under the wall-clock bound, but a seek
        clock.send(PlayerEvent::PlaybackRestart);
        clock.send(prop("pause", json!(true)));
        assert_eq!(clock.flushes.len(), 1);
        assert_eq!(
            clock.flushes[0].ops.last(),
            Some(&Op::Seek {
                time: 2_000,
                duration: 600_000
            })
        );
        clock.send(prop("pause", json!(false)));
        assert_eq!(clock.flushes.len(), 1, "unpausing writes nothing");
    }

    #[test]
    fn faster_speeds_are_not_mistaken_for_seeks() {
        let mut clock = Clock::started(600.0);
        clock.send(prop("speed", json!(2.0)));
        for i in 0..=50 {
            clock.send(pos(f64::from(i) * 0.4)); // 2x: 0.4 s of video per 200 ms
            clock.wait(200);
        }
        let flush = clock.tracker.close().unwrap();
        assert!(matches!(
            flush.ops[..],
            [Op::Seek { time: 0, .. }, Op::Progress { time: 20_000, .. }]
        ));
    }

    #[test]
    fn nothing_counts_before_the_file_starts_or_its_duration_is_known() {
        let mut clock = Clock::new();
        // a late position of the previous file (player_load)
        clock.send(prop("duration", json!(500.0)));
        clock.send(pos(400.0));
        clock.send(PlayerEvent::StartFile);
        clock.send(pos(0.0)); // duration reset by start-file: unknown
        assert_eq!(
            clock.tracker.close(),
            None,
            "never reported: nothing to write"
        );

        // live: no duration ever
        let mut live = Clock::new();
        live.send(PlayerEvent::StartFile);
        live.send(prop("duration", Value::Null));
        live.play(0.0, 2.0);
        assert_eq!(live.tracker.close(), None);
    }

    #[test]
    fn small_steps_back_are_jitter_and_never_undo_progress() {
        let mut clock = Clock::started(600.0);
        clock.play(0.0, 5.0);
        clock.send(pos(4.5)); // 0.5 s back: jitter
        let flush = clock.tracker.close().unwrap();
        assert!(matches!(
            flush.ops[..],
            [Op::Seek { .. }, Op::Progress { time: 5_000, .. }]
        ));
    }

    #[test]
    fn eof_end_file_and_shutdown_write_what_is_pending() {
        let mut clock = Clock::started(60.0);
        clock.play(0.0, 3.0);
        clock.send(prop("eof-reached", json!(true)));
        assert_eq!(clock.flushes.len(), 1);
        clock.play(3.2, 4.0);
        clock.send(PlayerEvent::EndFile {
            reason: "stop",
            file_error: None,
        });
        assert_eq!(clock.flushes.len(), 2);
        clock.send(PlayerEvent::Shutdown);
        assert_eq!(clock.flushes.len(), 3);
        let last = clock.flushes.last().unwrap();
        assert!(last.finish && last.ops.is_empty());
    }

    // --- applying batches ---------------------------------------------------------------

    #[test]
    fn a_movie_watched_to_the_credits_is_watched_and_leaves_continue_watching() {
        let context = movie_context();
        let mut file = LibraryFile::default();
        let mut clock = Clock::started(600.0);
        clock.play(0.0, 300.0);
        apply_all(&mut file, &context, &clock.flushes);
        let item = &file.items["tt1"];
        assert!(item.removed && item.temp, "played, not added");
        assert!(is_in_continue_watching(item));
        assert_eq!(
            item.poster.as_ref().unwrap().as_str(),
            "https://img.example/tt1.jpg"
        );

        clock.flushes.clear();
        clock.play(300.2, 580.0);
        clock.flushes.extend(clock.tracker.close());
        apply_all(&mut file, &context, &clock.flushes);
        let item = &file.items["tt1"];
        assert_eq!(item.state.times_watched, 1, "past 70%");
        assert_eq!(item.state.time_offset, 0, "stopped in the credits");
        assert!(!is_in_continue_watching(item));
    }

    #[test]
    fn skipping_ahead_does_not_make_a_movie_watched() {
        let context = movie_context();
        let mut file = LibraryFile::default();
        let mut clock = Clock::started(600.0);
        clock.play(0.0, 10.0);
        clock.send(pos(500.0));
        clock.wait(200);
        clock.play(500.2, 520.0);
        clock.flushes.extend(clock.tracker.close());
        apply_all(&mut file, &context, &clock.flushes);
        let item = &file.items["tt1"];
        assert_eq!(item.state.times_watched, 0);
        assert!(
            item.state.time_watched < 40_000,
            "{}",
            item.state.time_watched
        );
        assert_eq!(item.state.time_offset, 520_000);
    }

    #[test]
    fn stopping_an_episode_in_the_credits_moves_on_to_the_next() {
        let context = series_context(1);
        let mut file = LibraryFile::default();
        let mut clock = Clock::started(100.0);
        clock.play(0.0, 95.0);
        clock.flushes.extend(clock.tracker.close());
        apply_all(&mut file, &context, &clock.flushes);
        let item = &file.items["tt9"];
        assert_eq!(item.state.video_id.as_deref(), Some("tt9:1:2"));
        assert_eq!(item.state.time_offset, 1, "next episode, from the start");
        assert!(is_in_continue_watching(item));
        assert_eq!(watched_video_ids(item, &context.videos), ["tt9:1:1"]);
    }

    /// The shell closes a session before the next starts, so this cannot happen in the
    /// app; `apply` still refuses to let a stale session take the item back.
    #[test]
    fn a_late_batch_does_not_touch_the_episode_played_after_it() {
        let mut file = LibraryFile::default();
        let first = series_context(1);
        let mut one = Clock::started(100.0);
        one.play(0.0, 95.0);
        apply_all(&mut file, &first, &one.flushes);

        // episode 3 starts and reports before episode 1's final batch arrives
        let third = series_context(3);
        let mut three = Clock::started(100.0);
        three.play(0.0, 16.0);
        apply_all(&mut file, &third, &three.flushes);
        apply(&mut file, &first, &one.tracker.close().unwrap(), now());

        let item = &file.items["tt9"];
        assert_eq!(item.state.video_id.as_deref(), Some("tt9:1:3"));
        assert!(
            item.state.time_offset >= 15_000,
            "episode 3 keeps its position"
        );
    }

    #[test]
    fn resuming_another_episode_clears_the_old_position_first() {
        let mut file = LibraryFile::default();
        let mut one = Clock::started(100.0);
        one.play(0.0, 40.0);
        one.flushes.extend(one.tracker.close());
        apply_all(&mut file, &series_context(1), &one.flushes);
        assert_eq!(file.items["tt9"].state.time_offset, 40_000);

        let mut two = Clock::started(100.0);
        two.play(0.0, 16.0);
        apply_all(&mut file, &series_context(2), &two.flushes);
        let item = &file.items["tt9"];
        assert_eq!(item.state.video_id.as_deref(), Some("tt9:1:2"));
        assert!(item.state.time_offset < 20_000);
    }

    #[test]
    fn live_and_unchanged_batches_write_nothing() {
        let mut live = movie_context();
        live.meta.r#type = "tv".into();
        let mut file = LibraryFile::default();
        let flush = Flush {
            first: true,
            ops: vec![Op::Seek {
                time: 0,
                duration: 1_000,
            }],
            finish: false,
        };
        assert_eq!(apply(&mut file, &live, &flush, now()), None);
        assert!(file.items.is_empty());

        let context = movie_context();
        assert_eq!(
            apply(&mut file, &context, &flush, now()).as_deref(),
            Some("tt1")
        );
        let finish_only = Flush {
            first: false,
            ops: vec![],
            finish: true,
        };
        assert_eq!(
            apply(&mut file, &context, &finish_only, now()),
            None,
            "not in the credits"
        );
        assert_eq!(
            apply(&mut LibraryFile::default(), &context, &finish_only, now()),
            None,
            "never written"
        );
    }

    #[test]
    fn contexts_are_checked() {
        assert!(movie_context().check().is_ok());
        let mut bad = movie_context();
        bad.video_id = String::new();
        assert!(bad.check().is_err());
        let mut bad = series_context(1);
        bad.videos[0].id = "a\nb".into();
        assert!(bad.check().is_err());
    }

    /// The exact `context` the Player sends (`playbackContext` in
    /// src/lib/playback-handoff.ts, captured from the UI): the contract between them.
    #[test]
    fn the_players_context_payload_reads_as_sent() {
        let context: PlaybackContext = serde_json::from_value(json!({
            "meta": {
                "id": "tt0944947", "type": "series", "name": "Game of Thrones",
                "poster": "https://images.example/got.jpg", "posterShape": "poster",
                "behaviorHints": {
                    "defaultVideoId": null, "hasScheduledVideos": false, "isLive": false
                }
            },
            "videoId": "tt0944947:1:1",
            "videos": [
                {"id": "tt0944947:1:2", "season": 1, "episode": 2,
                 "released": "2011-04-17T00:00:00.000Z"},
                {"id": "tt0944947:1:1", "season": 1, "episode": 1,
                 "released": "2011-04-17T00:00:00.000Z"},
                {"id": "tt0944947:0:1", "season": 0, "episode": 1,
                 "released": "2011-04-17T00:00:00.000Z"}
            ]
        }))
        .unwrap();
        assert!(context.check().is_ok());
        assert_eq!(context.meta.id, "tt0944947");
        assert_eq!(context.meta.r#type, "series");
        assert!(context.meta.poster.is_some());
        assert!(!context.meta.is_live());
        assert_eq!(context.video_id, "tt0944947:1:1");
        assert_eq!(context.videos.len(), 3);
        assert_eq!(context.videos[2].season, Some(0));
        assert!(context.videos[0].released.is_some());
        assert_eq!(
            next_video_id(&context.videos, "tt0944947:1:1", now()).as_deref(),
            Some("tt0944947:1:2"),
            "in the meta's order the special sorts last, so episode 1 is followed by episode 2"
        );
    }

    // --- a real libmpv trace -----------------------------------------------------------

    /// The event as the shell receives it, from its serialized form in a trace.
    fn player_event(value: &Value) -> PlayerEvent {
        let text = |key: &str| value[key].as_str().map(String::from);
        match value["type"].as_str().expect("an event type") {
            "property" => PlayerEvent::Property {
                name: text("name").expect("a property name"),
                value: value["value"].clone(),
            },
            "start-file" => PlayerEvent::StartFile,
            "file-loaded" => PlayerEvent::FileLoaded,
            "seek" => PlayerEvent::Seek,
            "playback-restart" => PlayerEvent::PlaybackRestart,
            "end-file" => PlayerEvent::EndFile {
                reason: match value["reason"].as_str() {
                    Some("eof") => "eof",
                    Some("stop") => "stop",
                    Some("quit") => "quit",
                    Some("error") => "error",
                    Some("redirect") => "redirect",
                    _ => "unknown",
                },
                file_error: text("fileError"),
            },
            "error" => PlayerEvent::Error {
                message: text("message").unwrap_or_default(),
            },
            "client-message" => PlayerEvent::ClientMessage { args: Vec::new() },
            "shutdown" => PlayerEvent::Shutdown,
            other => panic!("unknown event type {other}"),
        }
    }

    /// A real event trace of the embedded player (libmpv, headless), recorded by
    /// zeiler-player's `record_a_progress_trace` and replayed here with its real
    /// timing: a 30 s clip (29.9 s as mpv reports it) with 4 s played, a skip ahead to 0:15, 2 s, a pause, a skip
    /// back to 0:08 while paused, 2 s, 2 s at double speed (4 s of video), a skip to
    /// 0:28 and the rest to the end, then quit. Only what was played counts as watched
    /// (about 14 s: under 70%, so not watched), and ending at the end drops the resume
    /// position.
    #[test]
    fn a_real_libmpv_trace_counts_only_what_was_played() {
        let trace: Vec<(u64, Value)> =
            serde_json::from_str(include_str!("fixtures/libmpv-trace.json")).unwrap();
        let start = Instant::now();
        let mut tracker = Tracker::default();
        let mut flushes = Vec::new();
        for (ms, event) in &trace {
            let at = start + Duration::from_millis(*ms);
            flushes.extend(tracker.on_event(&player_event(event), at));
        }
        assert_eq!(
            tracker.close(),
            None,
            "the trace ends with shutdown, which closed it"
        );
        let last = flushes.last().expect("batches were written");
        assert!(last.finish, "{flushes:#?}");
        let seeks = flushes
            .iter()
            .flat_map(|f| &f.ops)
            .filter(|op| matches!(op, Op::Seek { .. }))
            .count();
        assert!(
            seeks >= 4,
            "the start and three skips are seeks: {flushes:#?}"
        );

        let context = movie_context();
        let mut file = LibraryFile::default();
        let mut mid = None;
        for flush in &flushes {
            apply(&mut file, &context, flush, now());
            if !flush.finish {
                mid = file.items.get("tt1").map(|item| item.state.time_offset);
            }
        }
        let item = &file.items["tt1"];
        // about 13.7 s were played; each skip can drop the last 200 ms report before it
        assert!(
            (12_500..=14_500).contains(&item.state.time_watched),
            "time watched {} ms; batches {flushes:#?}",
            item.state.time_watched
        );
        assert_eq!(item.state.times_watched, 0, "14 s of 30 s is not watched");
        assert_eq!(
            item.state.duration, 29_900,
            "lavfi reports the last frame's end"
        );
        assert!(
            mid.is_some_and(|offset| offset >= 29_000),
            "at the end before closing: {mid:?}"
        );
        assert_eq!(
            item.state.time_offset, 0,
            "finished at the end: nothing to resume"
        );
        assert!(!is_in_continue_watching(item));
    }

    // --- end to end through the store ---------------------------------------------------

    #[tokio::test]
    async fn a_session_writes_through_the_worker_and_announces_it() {
        let dir = temp_dir("library-session");
        let changes = Arc::new(Mutex::new(Vec::new()));
        let sink = changes.clone();
        let library = LibraryState::load(&dir, move |ids| sink.lock().unwrap().extend(ids))
            .await
            .unwrap();
        assert!(
            library
                .session({
                    let mut live = movie_context();
                    live.meta.behavior_hints.is_live = true;
                    live
                })
                .is_none()
        );

        let session = library.session(movie_context()).unwrap();
        session.on_event(&PlayerEvent::StartFile);
        session.on_event(&prop("duration", json!(600.0)));
        session.on_event(&pos(30.0));
        session.on_event(&pos(30.2));
        session
            .close()
            .expect("something to write")
            .await
            .expect("the worker answers");
        assert!(session.close().is_none());

        assert_eq!(*changes.lock().unwrap(), ["tt1"]);
        let listed = library
            .list(&LibraryQuery {
                filter: zeiler_account::LibraryFilter::ContinueWatching,
                sort: Default::default(),
                r#type: None,
                limit: None,
            })
            .await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].time_offset, 30_200);
        let on_disk: LibraryFile =
            serde_json::from_slice(&std::fs::read(dir.join("library.json")).unwrap()).unwrap();
        let item: &LibraryItem = &on_disk.items["tt1"];
        assert_eq!(item.state.video_id.as_deref(), Some("tt1"));
    }

    // --- review fixes -------------------------------------------------------------------

    #[test]
    fn another_file_in_the_same_player_ends_the_session() {
        let mut clock = Clock::started(600.0);
        clock.play(0.0, 5.0);
        clock.send(PlayerEvent::EndFile {
            reason: "stop",
            file_error: None,
        });
        // a file dropped into the external mpv window
        clock.send(PlayerEvent::StartFile);
        assert!(
            clock.flushes.last().unwrap().finish,
            "the playback ended there"
        );
        let before = clock.flushes.len();
        clock.send(prop("duration", json!(7200.0)));
        clock.send(PlayerEvent::FileLoaded);
        clock.play(0.0, 20.0);
        assert_eq!(
            clock.flushes.len(),
            before,
            "nothing of the other file is tracked"
        );

        // a redirect (a playlist URL resolving to its entry) is still this playback
        let mut redirect = Clock::new();
        redirect.send(PlayerEvent::StartFile);
        redirect.send(PlayerEvent::EndFile {
            reason: "redirect",
            file_error: None,
        });
        redirect.send(PlayerEvent::StartFile);
        redirect.send(prop("duration", json!(600.0)));
        redirect.play(0.0, 2.0);
        let last = redirect.tracker.close().unwrap();
        assert!(matches!(
            last.ops[..],
            [Op::Seek { .. }, Op::Progress { time: 2_000, .. }]
        ));
    }

    /// The rule the tracker and the shell's subtitle adds share: which `start-file` is
    /// another file than the one the playback loaded.
    #[test]
    fn only_a_start_file_after_the_own_file_started_is_another_file() {
        let end = |reason| PlayerEvent::EndFile {
            reason,
            file_error: None,
        };
        let run = |events: Vec<PlayerEvent>| {
            let mut own = OwnFile::default();
            events
                .iter()
                .map(|e| own.another_file(e))
                .collect::<Vec<_>>()
        };

        // the launched file's own start-file, and its own events after it
        assert_eq!(
            run(vec![
                PlayerEvent::StartFile,
                PlayerEvent::FileLoaded,
                pos(1.0),
                PlayerEvent::Seek,
            ]),
            [false; 4]
        );
        // an end-file before the first start-file (the file before a player_load) is
        // not this playback's
        assert_eq!(
            run(vec![end("stop"), PlayerEvent::StartFile]),
            [false, false]
        );
        // after the own file ended, however it ended, a start-file is another file; and
        // so is each one after that
        for reason in ["eof", "stop", "quit", "error", "unknown"] {
            assert_eq!(
                run(vec![
                    PlayerEvent::StartFile,
                    end(reason),
                    PlayerEvent::StartFile,
                    end("eof"),
                    PlayerEvent::StartFile,
                ]),
                [false, false, true, false, true],
                "{reason}"
            );
        }
        // a redirect is still this playback, and so is a redirect of another file
        assert_eq!(
            run(vec![
                PlayerEvent::StartFile,
                end("redirect"),
                PlayerEvent::StartFile,
                end("stop"),
                PlayerEvent::StartFile,
                end("redirect"),
                PlayerEvent::StartFile,
            ]),
            [false, false, false, false, true, false, false]
        );
        // a missed start-file: file-loaded says the own file started
        assert_eq!(
            run(vec![
                PlayerEvent::FileLoaded,
                end("stop"),
                PlayerEvent::StartFile
            ]),
            [false, false, true]
        );
    }

    /// External mpv plays on after Back, so the user can dismiss the title from Continue
    /// Watching, or mark it watched, while it is tracked: the next batch must count only
    /// what was played since, not the whole position again.
    #[test]
    fn a_position_moved_underneath_counts_only_what_was_played() {
        let context = movie_context();
        let mut file = LibraryFile::default();
        let mut clock = Clock::started(6000.0);
        clock.play(0.0, 3600.0);
        apply_all(&mut file, &context, &clock.flushes);
        let before = file.items["tt1"].state.time_watched;
        assert!(before >= 3_590_000, "{before}");

        zeiler_account::clear_progress(file.items.get_mut("tt1").unwrap());
        clock.flushes.clear();
        clock.play(3600.2, 3615.0);
        apply_all(&mut file, &context, &clock.flushes);
        let item = &file.items["tt1"];
        let added = item.state.time_watched - before;
        assert!(added <= 16_000, "about 15 s were played, not {added} ms");
        assert_eq!(item.state.times_watched, 0, "60% is not watched");
        assert!(
            item.state.time_offset >= 3_610_000,
            "back where playback is"
        );

        zeiler_account::mark_watched(file.items.get_mut("tt1").unwrap(), true, now());
        clock.flushes.clear();
        clock.play(3615.2, 3631.0);
        clock.flushes.extend(clock.tracker.close());
        apply_all(&mut file, &context, &clock.flushes);
        let item = &file.items["tt1"];
        assert_eq!(
            item.state.times_watched, 1,
            "the mark counts once, the replay not at all"
        );
        assert!(item.state.time_watched - before <= 32_000);
    }

    #[tokio::test]
    async fn a_session_whose_first_write_failed_is_still_saved() {
        let dir = temp_dir("library-session-retry");
        let library = LibraryState::load(&dir, |_| {}).await.unwrap();
        // a directory where the temp file goes makes every write fail
        let blocker = dir.join("library.json.tmp");
        std::fs::create_dir_all(&blocker).unwrap();

        let session = library.session(series_context(2)).unwrap();
        session.on_event(&PlayerEvent::StartFile);
        session.on_event(&prop("duration", json!(600.0)));
        session.on_event(&pos(100.0));
        session.on_event(&pos(100.2));
        // the first batch: it fails, and fails again on the retry
        session.on_event(&prop("pause", json!(true)));
        tokio::time::sleep(Duration::from_millis(1_000)).await;
        assert!(library.get("tt9", &[]).await.is_none());

        std::fs::remove_dir_all(&blocker).unwrap();
        session.on_event(&prop("pause", json!(false)));
        session.on_event(&pos(100.4));
        session.close().unwrap().await.unwrap();
        let entry = library
            .get("tt9", &[])
            .await
            .expect("saved after all")
            .entry;
        assert_eq!(entry.video_id.as_deref(), Some("tt9:1:2"));
        assert_eq!(entry.time_offset, 100_400);
    }
}
