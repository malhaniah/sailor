//! Pure library logic: creating items, adding/removing, playback progress, watched
//! marks, Continue Watching and the sort orders of Stremio's library views.
//!
//! Adapted from stremio-core (<https://github.com/Stremio/stremio-core>: `models/player.rs`
//! `TimeChanged`/`Seek` and `item_state_update`, `models/ctx/update_library.rs`,
//! `types/library/library_item.rs`, `types/resource/meta_item.rs` (video order,
//! `next_video`), `models/meta_details.rs` (marking videos watched),
//! `models/library_with_filters.rs`), MIT License,
//! Copyright (c) 2019 SmartCode OOD — see THIRD-PARTY-NOTICES.md for the license text.
//!
//! Nothing here reads a clock: `now` is always a parameter. None of the mutating
//! functions touch `_mtime`; the caller runs [`touch`] once per committed change, which
//! sets it only when something other than `_mtime` changed (stremio-core's
//! `UpdateLibraryItem`).
//!
//! Zeiler-specific rules (documented deviations):
//! - Live items ([`LibraryItem::is_live`]) are never tracked: progress, seeks and the
//!   end-of-playback rule leave them untouched (stremio-core tracks them under the
//!   channel id).
//! - The watched bitfield is only written when the caller knows the meta's videos;
//!   for a movie (no videos) `state.watched` stays `null` as stremio-core documents it,
//!   and a stored field whose anchor is missing from the given list is left alone
//!   rather than replaced by a blank one (a partial list must never erase marks).
//! - The credits rule at the end of playback needs a known duration.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use url::Url;

use crate::types::{
    LibraryItem, LibraryItemBehaviorHints, LibraryItemState, MAX_URL_LEN, PosterShape, lenient,
};
use crate::watched::{WatchedBitField, WatchedField};

/// A video counts as watched once more than this share of its duration was watched.
pub const WATCHED_THRESHOLD_COEF: f64 = 0.7;
/// Stopping past this share of the duration counts as finished: the resume position is
/// cleared (and a series moves on to the next episode).
pub const CREDITS_THRESHOLD_COEF: f64 = 0.9;

/// What a new or refreshed library item needs from a meta (stremio-core builds items
/// from a `MetaItemPreview`). The UI sends the slice of its `MetaItem` it has.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetaSummary {
    pub id: String,
    pub r#type: String,
    pub name: String,
    #[serde(deserialize_with = "lenient_url")]
    pub poster: Option<Url>,
    pub poster_shape: PosterShape,
    #[serde(deserialize_with = "lenient::object")]
    pub behavior_hints: MetaHints,
}

impl MetaSummary {
    /// Live TV / channels, which are never tracked: the same rule as
    /// [`LibraryItem::is_live`].
    pub fn is_live(&self) -> bool {
        self.behavior_hints.is_live || self.r#type == "tv"
    }
}

/// The meta's behaviour hints, all of which a library item keeps (stremio-core copies
/// them wholesale). Read leniently: they are addon data.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetaHints {
    #[serde(deserialize_with = "lenient::opt_string")]
    pub default_video_id: Option<String>,
    #[serde(deserialize_with = "lenient::opt_string")]
    pub featured_video_id: Option<String>,
    #[serde(deserialize_with = "lenient::boolean")]
    pub has_scheduled_videos: bool,
    #[serde(deserialize_with = "lenient::boolean")]
    pub is_live: bool,
    /// The hints this version does not model.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// Unmodelled hints larger than this (serialized) are not stored: a misbehaving addon
/// must not bloat every write of the library.
pub const MAX_OTHER_HINTS_BYTES: usize = 4096;

/// A poster is only ever an http(s) image of sane length: anything else (unparseable,
/// `data:`, `javascript:`, `file:`, longer than [`MAX_URL_LEN`]) reads as none.
fn lenient_url<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Url>, D::Error> {
    Ok(match Value::deserialize(d)? {
        // percent-encoding can make the stored form longer than the input
        Value::String(s) if s.len() <= MAX_URL_LEN => Url::parse(&s).ok().filter(|url| {
            matches!(url.scheme(), "http" | "https") && url.as_str().len() <= MAX_URL_LEN
        }),
        _ => None,
    })
}

/// One video of a meta, as much as the bitfield order and "next episode" need.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoRef {
    pub id: String,
    /// Anything but an integer in `0..=u32::MAX` reads as none (addons send negative and
    /// huge numbers; stremio-core then has no series info for the video either).
    #[serde(deserialize_with = "lenient_u32")]
    pub season: Option<u32>,
    #[serde(deserialize_with = "lenient_u32")]
    pub episode: Option<u32>,
    #[serde(deserialize_with = "lenient_time", serialize_with = "out_time")]
    pub released: Option<DateTime<Utc>>,
}

impl VideoRef {
    /// stremio-core's `SeriesInfo`: both season and episode, or neither.
    fn series(&self) -> Option<(u32, u32)> {
        self.season.zip(self.episode)
    }
}

fn lenient_u32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    Ok(Value::deserialize(d)?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok()))
}

fn lenient_time<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => crate::types::parse_time(&s),
        _ => None,
    })
}

fn out_time<S: serde::Serializer>(t: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match t {
        Some(t) => s.serialize_str(&crate::types::format_time(t)),
        None => s.serialize_none(),
    }
}

/// The meta's videos as stremio-core holds them (`MetaItem.videos`, its
/// `VideoSortedVecAdapter`): later duplicates of an id dropped, then episodes before
/// everything else, by season with the specials (season 0) last, then episode; videos
/// without series info by release date, oldest first in a series and newest first
/// otherwise, undated ones last. The sort is stable, so ties keep the addon's order.
fn meta_order(videos: &[VideoRef]) -> Vec<&VideoRef> {
    let mut seen = HashSet::with_capacity(videos.len());
    let mut unique: Vec<&VideoRef> = videos
        .iter()
        .filter(|video| seen.insert(video.id.as_str()))
        .collect();
    let is_series = unique.iter().any(|video| video.series().is_some());
    let season = |s: u32| if s == 0 { u32::MAX } else { s };
    unique.sort_by(|a, b| match (a.series(), b.series()) {
        (Some((sa, ea)), Some((sb, eb))) => season(sa).cmp(&season(sb)).then(ea.cmp(&eb)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => match (a.released, b.released) {
            (Some(ra), Some(rb)) if is_series => ra.cmp(&rb),
            (Some(ra), Some(rb)) => rb.cmp(&ra),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
    });
    unique
}

/// Video ids in bitfield order (stremio-core `LibraryItemState::watched_bitfield`): the
/// meta's order ([`meta_order`]) stably re-sorted by season, then episode (both or
/// neither), then release date, missing values first.
pub fn ordered_video_ids(videos: &[VideoRef]) -> Vec<String> {
    ordered(videos).into_iter().map(|v| v.id.clone()).collect()
}

fn ordered(videos: &[VideoRef]) -> Vec<&VideoRef> {
    let mut sorted = meta_order(videos);
    sorted.sort_by_key(|video| {
        let (season, episode) = video
            .series()
            .map_or((i64::MIN, i64::MIN), |(s, e)| (i64::from(s), i64::from(e)));
        let released = video.released.map_or(i64::MIN, |r| r.timestamp_millis());
        (season, episode, released)
    });
    sorted
}

/// The video after `current` in the meta's order ([`meta_order`]), if it is released by
/// `now` and does not cross into the specials (season 0) from another season
/// (stremio-core `MetaItem::next_video`).
pub fn next_video_id(videos: &[VideoRef], current: &str, now: DateTime<Utc>) -> Option<String> {
    let order = meta_order(videos);
    let index = order.iter().position(|video| video.id == current)?;
    next_index(&order, index, now).map(|next| order[next].id.clone())
}

/// The index of the video after `order[index]` in the meta's order, by
/// `MetaItem::next_video`'s rules.
fn next_index(order: &[&VideoRef], index: usize, now: DateTime<Utc>) -> Option<usize> {
    let (current, next) = (order.get(index)?, order.get(index + 1)?);
    let season = |video: &VideoRef| video.series().map_or(0, |(season, _)| season);
    let released = next.released.is_none_or(|released| released <= now);
    ((season(next) != 0 || season(current) == season(next)) && released).then_some(index + 1)
}

/// A new item for a meta that was played (or marked) but not added: `removed` and
/// `temp`, so it shows only in Continue Watching (stremio-core `From<&MetaItemPreview>`).
pub fn new_item_from_meta(meta: &MetaSummary, now: DateTime<Utc>) -> LibraryItem {
    LibraryItem {
        id: meta.id.clone(),
        name: meta.name.clone(),
        r#type: meta.r#type.clone(),
        poster: meta.poster.clone(),
        poster_shape: meta.poster_shape,
        removed: true,
        temp: true,
        ctime: Some(now),
        mtime: now,
        state: LibraryItemState {
            last_watched: Some(now),
            ..LibraryItemState::default()
        },
        behavior_hints: hints_from_meta(&meta.behavior_hints, &meta.r#type, None),
        extra: Default::default(),
    }
}

/// The item's hints are the meta's (stremio-core `..meta_item.behavior_hints`), with
/// `isLive` also set for the `tv` type. An oversized set of unmodelled hints is not
/// stored; the item keeps the ones it had.
fn hints_from_meta(
    hints: &MetaHints,
    r#type: &str,
    previous: Option<&LibraryItemBehaviorHints>,
) -> LibraryItemBehaviorHints {
    let fits =
        serde_json::to_string(&hints.other).is_ok_and(|json| json.len() <= MAX_OTHER_HINTS_BYTES);
    LibraryItemBehaviorHints {
        is_live: hints.is_live || r#type == "tv",
        default_video_id: hints.default_video_id.clone(),
        featured_video_id: hints.featured_video_id.clone(),
        has_scheduled_videos: hints.has_scheduled_videos,
        extra: if fits {
            hints.other.clone()
        } else {
            previous.map(|p| p.extra.clone()).unwrap_or_default()
        },
    }
}

/// Refresh an existing item's meta fields (name, type, poster, hints), keeping its
/// state, flags and times (stremio-core `From<(&MetaItemPreview, &LibraryItem)>`).
pub fn refresh_from_meta(item: &mut LibraryItem, meta: &MetaSummary) {
    item.name = meta.name.clone();
    item.r#type = meta.r#type.clone();
    item.poster = meta.poster.clone();
    item.poster_shape = meta.poster_shape;
    item.behavior_hints = hints_from_meta(
        &meta.behavior_hints,
        &meta.r#type,
        Some(&item.behavior_hints),
    );
}

/// Add to (`removed = false`) or remove from (`removed = true`) the library; `temp` is
/// cleared either way, so a removed item also leaves Continue Watching.
pub fn set_in_library(item: &mut LibraryItem, in_library: bool) {
    item.removed = !in_library;
    item.temp = false;
}

/// Commit rule: when `item` differs from `previous` in anything but `_mtime` (or is
/// new), stamp `_mtime = now` and return true; otherwise restore the previous `_mtime`
/// and return false (nothing to write).
pub fn touch(previous: Option<&LibraryItem>, item: &mut LibraryItem, now: DateTime<Utc>) -> bool {
    match previous {
        Some(previous) if item.eq_no_mtime(previous) => {
            item.mtime = previous.mtime;
            false
        }
        _ => {
            item.mtime = now;
            true
        }
    }
}

/// Before playing `video_id`: when the item last played another video, its resume
/// position belongs to that video and is cleared (stremio-core
/// `library_item_state_update`). Live items are left alone.
pub fn prepare_playback(item: &mut LibraryItem, video_id: &str) {
    if item.is_live() {
        return;
    }
    if item
        .state
        .video_id
        .as_deref()
        .is_some_and(|current| current != video_id)
    {
        item.state.time_offset = 0;
    }
}

/// A playback position report (stremio-core `TimeChanged`): `time` and `duration` in
/// ms. Returns true when this report made the video count as watched.
///
/// - A different video than last time starts a fresh count: the time watched so far
///   folds into `overallTimeWatched`, `timeWatched` and `flaggedWatched` reset.
/// - Otherwise the time since the last report (`time - timeOffset`) is added to
///   `timeWatched` and `overallTimeWatched`.
/// - `timeOffset`/`duration` only move forward (a backwards jump must be reported with
///   [`apply_seek`]).
/// - Watched once `timeWatched > 0.7 × duration`: `flaggedWatched = 1`,
///   `timesWatched += 1`, and the video's bit is set when `videos` is known.
/// - A `temp` item never watched stays out of the library (`removed`), and a removed
///   item stays `temp` (so it shows in Continue Watching).
pub fn apply_playback_progress(
    item: &mut LibraryItem,
    video_id: &str,
    time: u64,
    duration: u64,
    videos: &[VideoRef],
    now: DateTime<Utc>,
) -> bool {
    if item.is_live() {
        return false;
    }
    let state = &mut item.state;
    state.last_watched = Some(now);
    if state.video_id.as_deref() != Some(video_id) {
        state.video_id = Some(video_id.to_string());
        state.overall_time_watched = state
            .overall_time_watched
            .saturating_add(state.time_watched);
        state.time_watched = 0;
        state.flagged_watched = 0;
    } else {
        let watched = time.saturating_sub(state.time_offset);
        state.time_watched = state.time_watched.saturating_add(watched);
        state.overall_time_watched = state.overall_time_watched.saturating_add(watched);
    }

    // a forward seek makes `time` jump; only `apply_seek` may move the offset back
    if time > state.time_offset {
        state.time_offset = time;
        state.duration = duration;
    }

    let newly_watched = state.flagged_watched == 0
        && state.duration > 0
        && state.time_watched as f64 > state.duration as f64 * WATCHED_THRESHOLD_COEF;
    if newly_watched {
        state.flagged_watched = 1;
        state.times_watched = state.times_watched.saturating_add(1);
        set_watched_bit(state, videos, video_id, true);
    }

    if item.temp && item.state.times_watched == 0 {
        item.removed = true;
    }
    if item.removed {
        item.temp = true;
    }
    newly_watched
}

/// A seek (stremio-core `Seek`): the position jumps to `time` without counting as time
/// watched. When the item last played another video, this is the first report for this
/// one and is handled as [`apply_playback_progress`] (which switches the video).
pub fn apply_seek(
    item: &mut LibraryItem,
    video_id: &str,
    time: u64,
    duration: u64,
    videos: &[VideoRef],
    now: DateTime<Utc>,
) {
    if item.is_live() {
        return;
    }
    if item.state.video_id.as_deref() != Some(video_id) {
        apply_playback_progress(item, video_id, time, duration, videos, now);
        return;
    }
    item.state.last_watched = Some(now);
    item.state.time_offset = time;
    item.state.duration = duration;
}

/// Playback of the current video ended (stopped, finished, or replaced): past the
/// credits (`timeOffset > 0.9 × duration`) there is nothing left to resume, so the
/// offset is cleared and, when there is a next episode, the item moves on to it
/// (stremio-core `item_state_update`). Returns true when it changed anything.
pub fn finish_playback(item: &mut LibraryItem, next_video_id: Option<&str>) -> bool {
    if item.is_live() {
        return false;
    }
    let state = &item.state;
    if state.duration == 0
        || (state.time_offset as f64) <= state.duration as f64 * CREDITS_THRESHOLD_COEF
    {
        return false;
    }
    item.state.time_offset = 0;
    if let Some(next) = next_video_id {
        advance_to_video(item, next);
    }
    true
}

/// Point Continue Watching at `video_id` from its beginning (stremio-core
/// `advance_to_video`; `timeOffset = 1` keeps the item in Continue Watching).
pub fn advance_to_video(item: &mut LibraryItem, video_id: &str) {
    let state = &mut item.state;
    state.video_id = Some(video_id.to_string());
    state.overall_time_watched = state
        .overall_time_watched
        .saturating_add(state.time_watched);
    state.time_watched = 0;
    state.flagged_watched = 0;
    state.time_offset = 1;
}

/// Mark the whole item watched or unwatched (stremio-core `LibraryItem::mark_as_watched`):
/// watched increments `timesWatched`, stamps `lastWatched` and clears the resume
/// position; unwatched resets `timesWatched`.
pub fn mark_watched(item: &mut LibraryItem, watched: bool, now: DateTime<Utc>) {
    if watched {
        item.state.times_watched = item.state.times_watched.saturating_add(1);
        item.state.last_watched = Some(now);
        item.state.time_offset = 0;
    } else {
        item.state.times_watched = 0;
    }
}

/// Mark one video watched or unwatched in the bitfield (stremio-core
/// `LibraryItem::mark_video_as_watched`); `videos` is the meta's full video list.
/// Marking watched moves `lastWatched` up to the video's release date when that is
/// later. Returns false when `video_id` is not in `videos` (nothing changes). Leaves
/// Continue Watching alone; [`mark_videos_watched`] is the user action.
pub fn mark_video_watched(
    item: &mut LibraryItem,
    videos: &[VideoRef],
    video_id: &str,
    watched: bool,
) -> bool {
    set_marks(item, videos, &[video_id], watched).is_some()
}

/// The user marks videos watched or unwatched (stremio-core MetaDetails
/// `MarkVideoAsWatched` for one video); `videos` is the meta's full video list and ids
/// not in it are ignored. The marks are set in one pass, and `lastWatched` moves up to
/// the release date of the last marked video (in the meta's order) when that is later.
/// When marking watched takes in the video Continue Watching points at, it moves on:
/// for one video to the next one, for several past the already watched ones (as for a
/// season); with nothing left, the resume position is dropped. Returns false when no
/// id is in `videos` (nothing changes).
pub fn mark_videos_watched(
    item: &mut LibraryItem,
    videos: &[VideoRef],
    video_ids: &[&str],
    watched: bool,
    now: DateTime<Utc>,
) -> bool {
    let Some(bits) = set_marks(item, videos, video_ids, watched) else {
        return false;
    };
    if watched {
        let walk = if video_ids.len() == 1 {
            Walk::Next
        } else {
            Walk::PastWatched
        };
        let marked: HashSet<&str> = video_ids.iter().copied().collect();
        move_on(item, videos, &bits, &marked, walk, now);
    }
    true
}

/// The user marks a whole season watched or unwatched (stremio-core MetaDetails
/// `MarkSeasonAsWatched`): the season's videos are marked as by
/// [`mark_videos_watched`], and when Continue Watching points into the season it moves
/// past the watched videos (`reconcile_series_resume_after_watched_change`), however
/// many the season has. Returns false when the season has no videos.
pub fn mark_season_watched(
    item: &mut LibraryItem,
    videos: &[VideoRef],
    season: u32,
    watched: bool,
    now: DateTime<Utc>,
) -> bool {
    let ids: Vec<&str> = meta_order(videos)
        .into_iter()
        .filter(|video| video.series().is_some_and(|(s, _)| s == season))
        .map(|video| video.id.as_str())
        .collect();
    let Some(bits) = set_marks(item, videos, &ids, watched) else {
        return false;
    };
    if watched {
        let marked: HashSet<&str> = ids.into_iter().collect();
        move_on(item, videos, &bits, &marked, Walk::PastWatched, now);
    }
    true
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Walk {
    /// To the next video (`MarkVideoAsWatched`).
    Next,
    /// To the first video after it that is not watched (a season).
    PastWatched,
}

/// After marks: when Continue Watching points at one of the `marked` videos, move it
/// on (`advance_to_video`), or drop the resume position when nothing is left to watch.
/// Walks the meta's order once.
fn move_on(
    item: &mut LibraryItem,
    videos: &[VideoRef],
    bits: &WatchedBitField,
    marked: &HashSet<&str>,
    walk: Walk,
    now: DateTime<Utc>,
) {
    if item.is_live() {
        return;
    }
    let Some(current) = item.state.video_id.clone() else {
        return;
    };
    if !marked.contains(current.as_str()) {
        return;
    }
    let order = meta_order(videos);
    let Some(mut index) = order.iter().position(|video| video.id == current) else {
        return;
    };
    let bit: HashMap<&str, usize> = bits
        .video_ids()
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let is_watched = |video: &VideoRef| bit.get(video.id.as_str()).is_some_and(|&i| bits.get(i));
    loop {
        match next_index(&order, index, now) {
            Some(next) if walk == Walk::PastWatched && is_watched(order[next]) => index = next,
            Some(next) => {
                let next = order[next].id.clone();
                advance_to_video(item, &next);
                return;
            }
            None => {
                item.state.time_offset = 0;
                return;
            }
        }
    }
}

/// Set the marks of `video_ids` in one pass and return the new bitfield; `None` (and
/// nothing changed) when none of them is in `videos`.
fn set_marks(
    item: &mut LibraryItem,
    videos: &[VideoRef],
    video_ids: &[&str],
    watched: bool,
) -> Option<WatchedBitField> {
    let wanted: HashSet<&str> = video_ids.iter().copied().collect();
    let marked: Vec<&VideoRef> = meta_order(videos)
        .into_iter()
        .filter(|video| wanted.contains(video.id.as_str()))
        .collect();
    let last = marked.last()?;
    let released = last.released;
    let mut bits = bitfield(&item.state, videos);
    let index: HashMap<&str, usize> = bits
        .video_ids()
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let positions: Vec<usize> = marked
        .iter()
        .filter_map(|video| index.get(video.id.as_str()).copied())
        .collect();
    for i in positions {
        bits.set(i, watched);
    }
    item.state.watched = Some(WatchedField::from(bits.clone()).to_string());
    if watched {
        item.state.last_watched = match (item.state.last_watched, released) {
            (Some(last), Some(released)) if last < released => Some(released),
            (None, released) => released,
            (last, _) => last,
        };
    }
    Some(bits)
}

/// The ids of `videos` marked watched in the item's bitfield (none when it has no
/// bitfield, or its anchor is not among `videos`).
pub fn watched_video_ids(item: &LibraryItem, videos: &[VideoRef]) -> Vec<String> {
    if videos.is_empty() {
        return Vec::new();
    }
    let bits = bitfield(&item.state, videos);
    bits.video_ids()
        .iter()
        .enumerate()
        .filter(|&(i, _)| bits.get(i))
        .map(|(_, id)| id.clone())
        .collect()
}

/// Drop the resume position (stremio-core `RewindLibraryItem`); the item leaves
/// Continue Watching.
pub fn clear_progress(item: &mut LibraryItem) {
    item.state.time_offset = 0;
    if item.is_live() {
        item.state.last_watched = None;
    }
}

/// stremio-core `is_in_continue_watching`: not `other`, in the library or played
/// (`temp`), and something to resume.
pub fn is_in_continue_watching(item: &LibraryItem) -> bool {
    item.r#type != "other" && (!item.removed || item.temp) && item.state.time_offset > 0
}

/// Where to resume `video_id` (or whatever was last played, when `None`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePoint {
    pub video_id: String,
    pub time_offset: u64,
    pub duration: u64,
}

/// The stored resume position when it belongs to `video_id`. `timeOffset = 1` (moved on
/// to the next episode) means "from the beginning", which is no resume point.
pub fn resume_point(item: &LibraryItem, video_id: Option<&str>) -> Option<ResumePoint> {
    if item.is_live() || item.state.time_offset <= 1 {
        return None;
    }
    let stored = item.state.video_id.as_deref()?;
    if video_id.is_some_and(|v| v != stored) {
        return None;
    }
    Some(ResumePoint {
        video_id: stored.to_string(),
        time_offset: item.state.time_offset,
        duration: item.state.duration,
    })
}

/// The views the UI lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryFilter {
    /// [`is_in_continue_watching`], most recently changed first (stremio-core's
    /// Continue Watching orders by `_mtime`).
    ContinueWatching,
    /// Items in the library (`!removed`), in the requested [`LibrarySort`].
    Library,
    /// Everything ever played (not `other`, with a `lastWatched` and some watch time
    /// or a watched mark), including items never added — most recent first. Zeiler's
    /// own view; Stremio has none.
    History,
}

/// stremio-core's library sort orders (`models/library_with_filters.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LibrarySort {
    #[default]
    LastWatched,
    Name,
    NameReverse,
    TimesWatched,
    Watched,
    NotWatched,
}

/// The items of one view, sorted. `r#type` narrows the library view to one type. Ties
/// are broken by id so the order is stable.
pub fn select<'a>(
    items: impl IntoIterator<Item = &'a LibraryItem>,
    filter: LibraryFilter,
    sort: LibrarySort,
    r#type: Option<&str>,
) -> Vec<&'a LibraryItem> {
    let mut selected: Vec<&LibraryItem> = items
        .into_iter()
        .filter(|item| r#type.is_none_or(|t| item.r#type == t))
        .filter(|item| match filter {
            LibraryFilter::ContinueWatching => is_in_continue_watching(item),
            LibraryFilter::Library => !item.removed,
            LibraryFilter::History => {
                item.r#type != "other"
                    && item.state.last_watched.is_some()
                    && (item.state.overall_time_watched > 0
                        || item.state.time_watched > 0
                        || item.state.time_offset > 0
                        || item.state.times_watched > 0)
            }
        })
        .collect();
    match filter {
        LibraryFilter::ContinueWatching => {
            selected.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.id.cmp(&b.id)));
        }
        LibraryFilter::History => {
            selected.sort_by(|a, b| {
                b.state
                    .last_watched
                    .cmp(&a.state.last_watched)
                    .then_with(|| a.id.cmp(&b.id))
            });
        }
        LibraryFilter::Library => {
            selected.sort_by(|a, b| {
                let name = |i: &LibraryItem| i.name.to_lowercase();
                match sort {
                    LibrarySort::LastWatched => b.state.last_watched.cmp(&a.state.last_watched),
                    LibrarySort::Name => name(a).cmp(&name(b)),
                    LibrarySort::NameReverse => name(b).cmp(&name(a)),
                    LibrarySort::TimesWatched => b.state.times_watched.cmp(&a.state.times_watched),
                    LibrarySort::Watched => b
                        .watched()
                        .cmp(&a.watched())
                        .then_with(|| b.state.last_watched.cmp(&a.state.last_watched))
                        .then_with(|| b.ctime.cmp(&a.ctime)),
                    LibrarySort::NotWatched => a
                        .watched()
                        .cmp(&b.watched())
                        .then_with(|| a.state.last_watched.cmp(&b.state.last_watched))
                        .then_with(|| a.ctime.cmp(&b.ctime)),
                }
                .then_with(|| a.id.cmp(&b.id))
            });
        }
    }
    selected
}

/// The item's bitfield aligned to `videos` (blank when it has none or it is malformed).
fn bitfield(state: &LibraryItemState, videos: &[VideoRef]) -> WatchedBitField {
    let ids = ordered_video_ids(videos);
    match state
        .watched
        .as_deref()
        .and_then(|w| w.parse::<WatchedField>().ok())
    {
        Some(field) => WatchedBitField::construct_with_videos(field, ids),
        None => WatchedBitField::construct_from_array(&[], ids),
    }
}

/// Set one bit for the player's watched rule; leaves the field alone without a video
/// list, or when the stored anchor is not in the list but marks exist (a partial list
/// must never erase marks).
fn set_watched_bit(state: &mut LibraryItemState, videos: &[VideoRef], video_id: &str, value: bool) {
    if videos.is_empty() || !videos.iter().any(|v| v.id == video_id) {
        return;
    }
    if let Some(field) = state
        .watched
        .as_deref()
        .and_then(|w| w.parse::<WatchedField>().ok())
        && field.bitfield.last_index_of(true).is_some()
        && !videos.iter().any(|v| v.id == field.anchor_video)
    {
        return;
    }
    let mut bits = bitfield(state, videos);
    bits.set_video(video_id, value);
    state.watched = Some(WatchedField::from(bits).to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::parse_time;

    fn t(s: &str) -> DateTime<Utc> {
        parse_time(s).unwrap()
    }

    fn movie() -> MetaSummary {
        MetaSummary {
            id: "tt0133093".into(),
            r#type: "movie".into(),
            name: "The Matrix".into(),
            poster: Some(Url::parse("https://images.example/tt0133093.jpg").unwrap()),
            ..MetaSummary::default()
        }
    }

    fn episodes(n: u32) -> Vec<VideoRef> {
        // deliberately out of order: the bitfield order is by season/episode
        (1..=n)
            .rev()
            .map(|e| VideoRef {
                id: format!("tt0944947:1:{e}"),
                season: Some(1),
                episode: Some(e),
                released: Some(t("2011-04-17T00:00:00Z")),
            })
            .collect()
    }

    /// A new movie played to the end, reported every 10 s like the shell does.
    #[test]
    fn a_movie_played_through_is_watched_once_and_finishes() {
        let now = t("2024-01-01T20:00:00Z");
        let mut item = new_item_from_meta(&movie(), now);
        assert!(item.removed && item.temp, "played, not added");
        assert!(!is_in_continue_watching(&item));

        let duration = 600_000;
        let mut watched_at = None;
        for second in (0..=600).step_by(10) {
            let time = second * 1000;
            if apply_playback_progress(&mut item, "tt0133093", time, duration, &[], now) {
                watched_at = Some(time);
            }
            assert!(item.removed && item.temp);
        }
        // 0.7 × 600 s = 420 s; time watched must exceed it
        assert_eq!(watched_at, Some(430_000));
        assert_eq!(item.state.times_watched, 1);
        assert_eq!(item.state.flagged_watched, 1);
        assert_eq!(item.state.time_watched, 600_000);
        assert_eq!(item.state.watched, None, "a movie has no bitfield");
        assert!(is_in_continue_watching(&item));

        assert!(finish_playback(&mut item, None));
        assert_eq!(item.state.time_offset, 0);
        assert!(!is_in_continue_watching(&item), "finished → out of CW");
    }

    #[test]
    fn a_forward_seek_is_not_time_watched() {
        let now = t("2024-01-01T20:00:00Z");
        let mut item = new_item_from_meta(&movie(), now);
        apply_playback_progress(&mut item, "tt0133093", 10_000, 600_000, &[], now);
        apply_seek(&mut item, "tt0133093", 500_000, 600_000, &[], now);
        assert!(!apply_playback_progress(
            &mut item,
            "tt0133093",
            505_000,
            600_000,
            &[],
            now
        ));
        assert_eq!(item.state.time_watched, 5_000);
        assert_eq!(item.state.time_offset, 505_000);
        // a backwards seek moves the offset back
        apply_seek(&mut item, "tt0133093", 60_000, 600_000, &[], now);
        assert_eq!(item.state.time_offset, 60_000);
        assert_eq!(item.state.time_watched, 5_000);
    }

    #[test]
    fn without_a_seek_a_backwards_report_never_moves_the_offset_back() {
        let now = t("2024-01-01T20:00:00Z");
        let mut item = new_item_from_meta(&movie(), now);
        apply_playback_progress(&mut item, "tt0133093", 100_000, 600_000, &[], now);
        apply_playback_progress(&mut item, "tt0133093", 50_000, 600_000, &[], now);
        assert_eq!(item.state.time_offset, 100_000);
    }

    #[test]
    fn a_series_episode_sets_its_bit_and_the_next_one_starts_fresh() {
        let now = t("2024-01-01T20:00:00Z");
        let videos = episodes(9);
        let meta = MetaSummary {
            id: "tt0944947".into(),
            r#type: "series".into(),
            name: "Game of Thrones".into(),
            ..MetaSummary::default()
        };
        let mut item = new_item_from_meta(&meta, now);
        set_in_library(&mut item, true);

        let duration = 3_600_000;
        for minute in 0..=55 {
            apply_playback_progress(
                &mut item,
                "tt0944947:1:5",
                minute * 60_000,
                duration,
                &videos,
                now,
            );
        }
        assert_eq!(item.state.times_watched, 1);
        assert!(
            !item.removed && !item.temp,
            "added items stay in the library"
        );
        let watched = item.state.watched.clone().unwrap();
        assert!(watched.starts_with("tt0944947:1:5:5:"), "{watched}");
        let bits = bitfield(&item.state, &videos);
        assert!(bits.get_video("tt0944947:1:5"));
        assert!(!bits.get_video("tt0944947:1:4"));

        // stopping in the credits moves Continue Watching to the next episode
        let next = next_video_id(&videos, "tt0944947:1:5", now);
        assert_eq!(next.as_deref(), Some("tt0944947:1:6"));
        assert!(finish_playback(&mut item, next.as_deref()));
        assert_eq!(item.state.video_id.as_deref(), Some("tt0944947:1:6"));
        assert_eq!(item.state.time_offset, 1);
        assert_eq!(item.state.time_watched, 0);
        // the episode's time is in `overallTimeWatched` twice: TimeChanged adds every
        // report to it, and moving on folds `timeWatched` in again (stremio-core does the
        // same; ported as-is so both clients agree)
        assert_eq!(item.state.overall_time_watched, 6_600_000);
        assert!(is_in_continue_watching(&item));
        assert_eq!(resume_point(&item, None), None, "from the start");

        // playing episode 6 counts from zero
        prepare_playback(&mut item, "tt0944947:1:6");
        apply_seek(&mut item, "tt0944947:1:6", 0, duration, &videos, now);
        apply_playback_progress(&mut item, "tt0944947:1:6", 60_000, duration, &videos, now);
        assert_eq!(item.state.time_watched, 60_000);
        assert_eq!(item.state.flagged_watched, 0);
        assert_eq!(
            resume_point(&item, Some("tt0944947:1:6")),
            Some(ResumePoint {
                video_id: "tt0944947:1:6".into(),
                time_offset: 60_000,
                duration
            })
        );
        assert_eq!(resume_point(&item, Some("tt0944947:1:5")), None);
    }

    #[test]
    fn switching_videos_folds_the_count_and_clears_the_old_offset() {
        let now = t("2024-01-01T20:00:00Z");
        let mut item = new_item_from_meta(&movie(), now);
        item.state.video_id = Some("a".into());
        item.state.time_offset = 900_000;
        item.state.time_watched = 120_000;
        item.state.flagged_watched = 1;

        prepare_playback(&mut item, "b");
        assert_eq!(item.state.time_offset, 0);
        apply_playback_progress(&mut item, "b", 5_000, 1_000_000, &[], now);
        assert_eq!(item.state.video_id.as_deref(), Some("b"));
        assert_eq!(item.state.time_watched, 0);
        assert_eq!(item.state.overall_time_watched, 120_000);
        assert_eq!(item.state.flagged_watched, 0);
        assert_eq!(item.state.time_offset, 5_000);
    }

    #[test]
    fn live_items_are_never_tracked() {
        let now = t("2024-01-01T20:00:00Z");
        let meta = MetaSummary {
            id: "tv:1".into(),
            r#type: "tv".into(),
            name: "Channel".into(),
            ..MetaSummary::default()
        };
        let mut item = new_item_from_meta(&meta, now);
        assert!(item.is_live() && item.behavior_hints.is_live);
        let before = item.clone();
        apply_playback_progress(&mut item, "tv:1", 99_000, 0, &[], now);
        apply_seek(&mut item, "tv:1", 5_000, 0, &[], now);
        assert!(!finish_playback(&mut item, None));
        assert_eq!(item, before);
        assert!(!is_in_continue_watching(&item));
    }

    #[test]
    fn mark_watched_mirrors_stremio_core() {
        let now = t("2024-01-01T20:00:00Z");
        let mut item = new_item_from_meta(&movie(), t("2023-01-01T00:00:00Z"));
        item.state.time_offset = 1234;
        mark_watched(&mut item, true, now);
        assert_eq!(item.state.times_watched, 1);
        assert_eq!(item.state.time_offset, 0);
        assert_eq!(item.state.last_watched, Some(now));
        mark_watched(&mut item, true, now);
        assert_eq!(item.state.times_watched, 2);
        mark_watched(&mut item, false, now);
        assert_eq!(item.state.times_watched, 0);
    }

    #[test]
    fn mark_video_watched_sets_bits_and_moves_last_watched_to_the_release() {
        let mut videos = episodes(3);
        videos[0].released = Some(t("2030-01-01T00:00:00Z")); // episode 3, in the future
        let mut item = new_item_from_meta(&movie(), t("2024-01-01T00:00:00Z"));

        assert!(mark_video_watched(
            &mut item,
            &videos,
            "tt0944947:1:2",
            true
        ));
        assert_eq!(item.state.last_watched, Some(t("2024-01-01T00:00:00Z")));
        assert!(mark_video_watched(
            &mut item,
            &videos,
            "tt0944947:1:3",
            true
        ));
        assert_eq!(item.state.last_watched, Some(t("2030-01-01T00:00:00Z")));
        let bits = bitfield(&item.state, &videos);
        assert!(!bits.get_video("tt0944947:1:1"));
        assert!(bits.get_video("tt0944947:1:2") && bits.get_video("tt0944947:1:3"));

        assert!(mark_video_watched(
            &mut item,
            &videos,
            "tt0944947:1:3",
            false
        ));
        let bits = bitfield(&item.state, &videos);
        assert!(bits.get_video("tt0944947:1:2") && !bits.get_video("tt0944947:1:3"));
        assert!(
            item.state
                .watched
                .as_deref()
                .unwrap()
                .starts_with("tt0944947:1:2:2:")
        );

        assert!(!mark_video_watched(
            &mut item,
            &videos,
            "tt0944947:9:9",
            true
        ));
    }

    #[test]
    fn a_partial_video_list_never_erases_marks() {
        let now = t("2024-01-01T20:00:00Z");
        let mut item = new_item_from_meta(&movie(), now);
        let all = episodes(9);
        assert!(mark_video_watched(&mut item, &all, "tt0944947:1:8", true));
        let stored = item.state.watched.clone();

        // the player only knows episodes 1-2: the anchor (1:8) is not among them
        let partial: Vec<VideoRef> = all
            .iter()
            .filter(|v| v.episode <= Some(2))
            .cloned()
            .collect();
        item.state.video_id = Some("tt0944947:1:1".into());
        for minute in 0..=50 {
            apply_playback_progress(
                &mut item,
                "tt0944947:1:1",
                minute * 60_000,
                3_000_000,
                &partial,
                now,
            );
        }
        assert_eq!(item.state.times_watched, 1);
        assert_eq!(item.state.watched, stored);
    }

    #[test]
    fn touch_stamps_mtime_only_for_real_changes() {
        let then = t("2024-01-01T00:00:00Z");
        let now = t("2024-01-02T00:00:00Z");
        let item = new_item_from_meta(&movie(), then);
        let mut same = item.clone();
        same.mtime = now;
        assert!(!touch(Some(&item), &mut same, now));
        assert_eq!(same.mtime, then);
        let mut changed = item.clone();
        set_in_library(&mut changed, true);
        assert!(touch(Some(&item), &mut changed, now));
        assert_eq!(changed.mtime, now);
        let mut fresh = item.clone();
        assert!(touch(None, &mut fresh, now));
    }

    fn v(id: &str, season: Option<u32>, episode: Option<u32>, released: Option<&str>) -> VideoRef {
        VideoRef {
            id: id.into(),
            season,
            episode,
            released: released.map(t),
        }
    }

    /// stremio-core's `MetaItem::next_video` over its sorted `MetaItem.videos`: the
    /// specials sort last, so nothing crosses into them from a regular season, and a
    /// special leads only to the next special.
    #[test]
    fn next_video_follows_stremio_cores_meta_order() {
        let now = t("2024-01-01T00:00:00Z");
        let videos = vec![
            v("s:0:1", Some(0), Some(1), None),
            v("s:0:2", Some(0), Some(2), None),
            v("s:2:1", Some(2), Some(1), None),
            v("s:1:2", Some(1), Some(2), Some("2025-01-01T00:00:00Z")),
            v("s:1:1", Some(1), Some(1), None),
        ];
        let next = |id: &str| next_video_id(&videos, id, now);
        assert_eq!(next("s:1:1"), None, "s:1:2 is not released yet");
        let released_later = t("2025-06-01T00:00:00Z");
        assert_eq!(
            next_video_id(&videos, "s:1:1", released_later).as_deref(),
            Some("s:1:2")
        );
        assert_eq!(next("s:1:2").as_deref(), Some("s:2:1"));
        assert_eq!(
            next("s:2:1"),
            None,
            "the last episode does not cross into the specials"
        );
        assert_eq!(next("s:0:1").as_deref(), Some("s:0:2"));
        assert_eq!(next("s:0:2"), None, "the last special is the last video");
        assert_eq!(next("missing"), None);
        // the bitfield order is different: by season (specials first), then episode
        assert_eq!(
            ordered_video_ids(&videos),
            ["s:0:1", "s:0:2", "s:1:1", "s:1:2", "s:2:1"]
        );
    }

    #[test]
    fn videos_without_series_info_follow_their_release_dates() {
        let now = t("2030-01-01T00:00:00Z");
        // a channel-like meta: newest first, so "next" is the next older video
        let uploads = vec![
            v("a", None, None, Some("2024-01-01T00:00:00Z")),
            v("c", None, None, Some("2024-03-01T00:00:00Z")),
            v("b", None, None, Some("2024-02-01T00:00:00Z")),
            v("undated", None, None, None),
        ];
        assert_eq!(next_video_id(&uploads, "c", now).as_deref(), Some("b"));
        assert_eq!(
            next_video_id(&uploads, "a", now).as_deref(),
            Some("undated")
        );
        // in a series, extras come after every episode, oldest first
        let series = vec![
            v("extra2", None, None, Some("2024-02-01T00:00:00Z")),
            v("extra1", None, None, Some("2024-01-01T00:00:00Z")),
            v("e1", Some(1), Some(1), None),
        ];
        assert_eq!(
            next_video_id(&series, "e1", now),
            None,
            "season 1 → extras: season 0"
        );
        assert_eq!(
            next_video_id(&series, "extra1", now).as_deref(),
            Some("extra2")
        );
    }

    #[test]
    fn duplicates_and_half_series_info_are_read_like_stremio_core() {
        // a duplicate id is dropped (the first stays); a video with only one of
        // season/episode has no series info at all
        let videos = vec![
            v("e1", Some(1), Some(1), None),
            v("e2", Some(1), Some(2), None),
            v("e2", Some(1), Some(2), None),
            v("half", Some(1), None, None),
            v("e3", Some(1), Some(3), None),
        ];
        assert_eq!(ordered_video_ids(&videos), ["half", "e1", "e2", "e3"]);
        let now = t("2024-01-01T00:00:00Z");
        assert_eq!(next_video_id(&videos, "e2", now).as_deref(), Some("e3"));

        // the marks land where stremio-core reads them
        let mut item = new_item_from_meta(&movie(), now);
        assert!(mark_video_watched(&mut item, &videos, "e3", true));
        assert!(mark_video_watched(&mut item, &videos, "e2", true));
        let field: WatchedField = item.state.watched.as_deref().unwrap().parse().unwrap();
        assert_eq!(
            (field.anchor_video.as_str(), field.anchor_length),
            ("e3", 4)
        );
        assert_eq!(watched_video_ids(&item, &videos), ["e2", "e3"]);
    }

    #[test]
    fn out_of_range_episode_numbers_read_as_no_series_info() {
        let videos: Vec<VideoRef> = serde_json::from_value(serde_json::json!([
            {"id": "a", "season": -1, "episode": 2},
            {"id": "b", "season": 1, "episode": 4_294_967_296_u64},
            {"id": "c", "season": 1.5, "episode": "3"},
            {"id": "d", "season": 1, "episode": 1}
        ]))
        .unwrap();
        assert_eq!(videos[0].season, None);
        assert_eq!(videos[0].episode, Some(2));
        assert_eq!(videos[1].episode, None);
        assert_eq!((videos[2].season, videos[2].episode), (None, None));
        assert_eq!((videos[3].season, videos[3].episode), (Some(1), Some(1)));
    }

    #[test]
    fn marking_the_current_episode_moves_continue_watching_on() {
        let now = t("2024-01-01T20:00:00Z");
        let videos = episodes(5);
        let mut item = new_item_from_meta(&movie(), now);
        item.state.video_id = Some("tt0944947:1:2".into());
        item.state.time_offset = 1_200_000;

        // another episode: the resume point stays
        assert!(mark_videos_watched(
            &mut item,
            &videos,
            &["tt0944947:1:4"],
            true,
            now
        ));
        assert_eq!(item.state.video_id.as_deref(), Some("tt0944947:1:2"));
        assert_eq!(item.state.time_offset, 1_200_000);

        // the current one: on to the next, from the start (MarkVideoAsWatched)
        assert!(mark_videos_watched(
            &mut item,
            &videos,
            &["tt0944947:1:2"],
            true,
            now
        ));
        assert_eq!(item.state.video_id.as_deref(), Some("tt0944947:1:3"));
        assert_eq!(item.state.time_offset, 1);

        // several, including the current one: past the watched ones (MarkSeasonAsWatched)
        assert!(mark_videos_watched(
            &mut item,
            &videos,
            &["tt0944947:1:1", "tt0944947:1:3"],
            true,
            now
        ));
        assert_eq!(
            item.state.video_id.as_deref(),
            Some("tt0944947:1:5"),
            "1:4 is watched"
        );

        // the last one: nothing left, the resume position goes
        assert!(mark_videos_watched(
            &mut item,
            &videos,
            &["tt0944947:1:5"],
            true,
            now
        ));
        assert_eq!(item.state.time_offset, 0);
        assert_eq!(watched_video_ids(&item, &videos).len(), 5);

        // unmarking never moves it; unknown ids change nothing
        assert!(mark_videos_watched(
            &mut item,
            &videos,
            &["tt0944947:1:5"],
            false,
            now
        ));
        assert_eq!(item.state.video_id.as_deref(), Some("tt0944947:1:5"));
        assert!(!mark_videos_watched(
            &mut item,
            &videos,
            &["tt0944947:9:9"],
            true,
            now
        ));
    }

    #[test]
    fn marking_several_moves_last_watched_to_the_last_ones_release() {
        let mut videos = episodes(3);
        for (video, day) in videos.iter_mut().zip(["03", "02", "01"]) {
            video.released = Some(t(&format!("2030-01-{day}T00:00:00Z")));
        }
        let now = t("2031-01-01T00:00:00Z");
        let mut item = new_item_from_meta(&movie(), t("2024-01-01T00:00:00Z"));
        assert!(mark_videos_watched(
            &mut item,
            &videos,
            &["tt0944947:1:2", "tt0944947:1:1"],
            true,
            now
        ));
        // in the meta's order episode 2 is the last marked one (released on the 2nd)
        assert_eq!(item.state.last_watched, Some(t("2030-01-02T00:00:00Z")));
    }

    #[test]
    fn views_filter_and_sort_like_stremio() {
        let base = t("2024-01-01T00:00:00Z");
        let at = |h: i64| base + chrono::Duration::hours(h);
        let mk = |id: &str, name: &str, removed: bool, temp: bool, offset: u64, h: i64| {
            let mut item = new_item_from_meta(
                &MetaSummary {
                    id: id.into(),
                    r#type: "movie".into(),
                    name: name.into(),
                    ..MetaSummary::default()
                },
                at(h),
            );
            item.removed = removed;
            item.temp = temp;
            item.state.time_offset = offset;
            item.state.overall_time_watched = offset;
            item
        };
        let mut other = mk("o", "Other", false, false, 5, 9);
        other.r#type = "other".into();
        let mut watched = mk("w", "beta", false, false, 0, 1);
        watched.state.times_watched = 3;
        let items = [
            mk("a", "Alpha", false, false, 10, 2),  // library + CW
            mk("b", "Bravo", true, true, 20, 5),    // played, never added
            mk("c", "charlie", true, false, 30, 7), // removed by the user
            mk("d", "Delta", false, false, 0, 3),   // in library, nothing to resume
            watched,
            other,
        ];
        let ids = |v: Vec<&LibraryItem>| v.into_iter().map(|i| i.id.clone()).collect::<Vec<_>>();

        assert_eq!(
            ids(select(
                &items,
                LibraryFilter::ContinueWatching,
                LibrarySort::default(),
                None
            )),
            ["b", "a"]
        );
        assert_eq!(
            ids(select(
                &items,
                LibraryFilter::Library,
                LibrarySort::LastWatched,
                None
            )),
            ["o", "d", "a", "w"]
        );
        assert_eq!(
            ids(select(
                &items,
                LibraryFilter::Library,
                LibrarySort::Name,
                None
            )),
            ["a", "w", "d", "o"]
        );
        assert_eq!(
            ids(select(
                &items,
                LibraryFilter::Library,
                LibrarySort::NameReverse,
                Some("movie")
            )),
            ["d", "w", "a"]
        );
        assert_eq!(
            ids(select(
                &items,
                LibraryFilter::Library,
                LibrarySort::TimesWatched,
                None
            ))[0],
            "w"
        );
        assert_eq!(
            ids(select(
                &items,
                LibraryFilter::Library,
                LibrarySort::Watched,
                None
            ))[0],
            "w"
        );
        assert_eq!(
            ids(select(
                &items,
                LibraryFilter::History,
                LibrarySort::default(),
                None
            )),
            ["c", "b", "a", "w"]
        );
    }

    #[test]
    fn watched_video_ids_follow_the_bitfield_in_video_order() {
        let videos = episodes(4);
        let mut item = new_item_from_meta(&movie(), t("2024-01-01T00:00:00Z"));
        assert!(watched_video_ids(&item, &videos).is_empty());
        mark_video_watched(&mut item, &videos, "tt0944947:1:3", true);
        mark_video_watched(&mut item, &videos, "tt0944947:1:1", true);
        assert_eq!(
            watched_video_ids(&item, &videos),
            ["tt0944947:1:1", "tt0944947:1:3"]
        );
        assert!(watched_video_ids(&item, &[]).is_empty());
    }

    /// stremio-core's MarkSeasonAsWatched skips watched episodes even for a season of
    /// one; marking that one episode as a video does not.
    #[test]
    fn a_season_mark_moves_past_watched_episodes_even_for_one_episode() {
        let now = t("2024-01-01T20:00:00Z");
        let videos = vec![
            v("s1e1", Some(1), Some(1), None),
            v("s2e1", Some(2), Some(1), None),
            v("s2e2", Some(2), Some(2), None),
        ];
        let start = || {
            let mut item = new_item_from_meta(&movie(), now);
            mark_video_watched(&mut item, &videos, "s2e1", true);
            item.state.video_id = Some("s1e1".into());
            item.state.time_offset = 600_000;
            item
        };

        let mut season = start();
        assert!(mark_season_watched(&mut season, &videos, 1, true, now));
        assert_eq!(season.state.video_id.as_deref(), Some("s2e2"));
        assert_eq!(season.state.time_offset, 1);

        let mut video = start();
        assert!(mark_videos_watched(
            &mut video,
            &videos,
            &["s1e1"],
            true,
            now
        ));
        assert_eq!(
            video.state.video_id.as_deref(),
            Some("s2e1"),
            "MarkVideoAsWatched"
        );

        // a season the pointer is not in leaves it; an empty season changes nothing
        let mut other = start();
        assert!(mark_season_watched(&mut other, &videos, 2, true, now));
        assert_eq!(other.state.video_id.as_deref(), Some("s1e1"));
        assert_eq!(other.state.time_offset, 600_000);
        assert!(!mark_season_watched(&mut other, &videos, 7, true, now));
    }

    #[test]
    fn a_poster_that_grows_past_the_cap_when_encoded_is_dropped() {
        let long = format!("https://img.example/{}", "ж".repeat(1_500));
        assert!(long.len() <= crate::types::MAX_URL_LEN, "fits as sent");
        let meta: MetaSummary =
            serde_json::from_value(serde_json::json!({"id": "tt1", "poster": long.clone()}))
                .unwrap();
        assert_eq!(meta.poster, None, "{} bytes encoded", 6 * 1_500);
        // the same raw string on a stored (or later synced) item
        let item: LibraryItem =
            serde_json::from_value(serde_json::json!({"_id": "tt1", "poster": long})).unwrap();
        assert_eq!(item.poster, None, "stored items are held to the same cap");
        let short: LibraryItem = serde_json::from_value(serde_json::json!({
            "_id": "tt1", "poster": format!("https://img.example/{}", "ж".repeat(100))
        }))
        .unwrap();
        assert!(short.poster.is_some(), "a normal non-ASCII poster stays");
    }

    /// stremio-core's own vector (`library_with_filters.rs`,
    /// `test_watched_and_not_watched_sort_items_ordering_of_library_items`).
    #[test]
    fn watched_sorts_match_stremio_cores_vector() {
        let now = t("2024-06-01T00:00:00Z");
        let weeks = |n: i64| now - chrono::Duration::weeks(n);
        let mk = |id: &str, kind: &str, ctime, last_watched, times_watched| {
            let mut item = new_item_from_meta(
                &MetaSummary {
                    id: id.into(),
                    r#type: kind.into(),
                    name: id.into(),
                    ..MetaSummary::default()
                },
                now,
            );
            set_in_library(&mut item, true);
            item.ctime = Some(ctime);
            item.state.last_watched = Some(last_watched);
            item.state.times_watched = times_watched;
            item
        };
        let items = [
            mk("gone-girl", "movie", weeks(3), weeks(3), 0),
            mk("ahsoka", "series", now, now, 2),
            mk("lebowski", "movie", weeks(2), weeks(2), 0),
            mk("oppenheimer", "movie", now, weeks(1), 1),
            mk("indiana", "movie", weeks(4), weeks(1), 0),
        ];
        let ids = |sort| {
            select(&items, LibraryFilter::Library, sort, None)
                .into_iter()
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(LibrarySort::Watched),
            ["ahsoka", "oppenheimer", "indiana", "lebowski", "gone-girl"]
        );
        assert_eq!(
            ids(LibrarySort::NotWatched),
            ["gone-girl", "lebowski", "indiana", "oppenheimer", "ahsoka"]
        );
    }

    #[test]
    fn meta_hints_are_kept_whole_and_live_comes_first() {
        let now = t("2024-01-01T00:00:00Z");
        let meta: MetaSummary = serde_json::from_value(serde_json::json!({
            "id": "tv:1", "type": "tv", "name": "Channel",
            "behaviorHints": {
                "featuredVideoId": "yt_id:abc", "defaultVideoId": 7,
                "hasScheduledVideos": "true", "someHint": 1
            }
        }))
        .unwrap();
        let item = new_item_from_meta(&meta, now);
        assert_eq!(
            serde_json::to_string(&item.behavior_hints).unwrap(),
            r#"{"isLive":true,"defaultVideoId":null,"featuredVideoId":"yt_id:abc","hasScheduledVideos":true,"someHint":1}"#
        );

        // an oversized set of unknown hints is not stored; the item keeps its own
        let mut huge = meta.clone();
        huge.behavior_hints.other.insert(
            "blob".into(),
            serde_json::Value::String("x".repeat(MAX_OTHER_HINTS_BYTES)),
        );
        let mut refreshed = item.clone();
        refresh_from_meta(&mut refreshed, &huge);
        assert_eq!(refreshed.behavior_hints.extra, item.behavior_hints.extra);
        assert!(
            new_item_from_meta(&huge, now)
                .behavior_hints
                .extra
                .is_empty()
        );
    }

    #[test]
    fn meta_posters_must_be_http_and_tv_is_live() {
        let meta: MetaSummary = serde_json::from_value(serde_json::json!({
            "id": "tv:1", "type": "tv", "name": "Channel",
            "poster": "javascript:alert(1)", "posterShape": "landscape"
        }))
        .unwrap();
        assert_eq!(meta.poster, None);
        assert_eq!(meta.poster_shape, PosterShape::Landscape);
        assert!(meta.is_live());
        for (poster, kept) in [
            ("https://img.example/p.jpg", true),
            ("http://img.example/p.jpg", true),
            ("data:image/png;base64,AAAA", false),
            ("file:///etc/passwd", false),
            ("not a url", false),
        ] {
            let meta: MetaSummary =
                serde_json::from_value(serde_json::json!({"id": "tt1", "poster": poster})).unwrap();
            assert_eq!(meta.poster.is_some(), kept, "{poster}");
            assert!(!meta.is_live());
        }
    }

    #[test]
    fn clear_progress_leaves_continue_watching() {
        let mut item = new_item_from_meta(&movie(), t("2024-01-01T00:00:00Z"));
        item.state.time_offset = 5000;
        assert!(is_in_continue_watching(&item));
        clear_progress(&mut item);
        assert!(!is_in_continue_watching(&item));
        assert!(item.state.last_watched.is_some(), "kept for non-live items");
    }
}
