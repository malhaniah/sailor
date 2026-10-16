//! Zeiler's local library: Stremio-compatible library items, watch progress and the
//! watched-episodes bitfield.
//!
//! Everything here is pure data and arithmetic — no I/O, no clock, no network. Callers
//! pass `now` in, and the Tauri shell owns storage (`src-tauri/src/library/`). The types
//! mirror stremio-core field for field (lenient in, strict out), so the same items can
//! later be synced with a Stremio account without loss; the progress rules copy
//! stremio-core's player model, so Continue Watching and watched marks behave like
//! Stremio's own apps.
//!
//! - [`types`]: `LibraryItem`, `LibraryItemState`, behaviour hints, `LibraryItemModified`.
//! - [`watched`]: the `{anchorVideoId}:{anchorLength}:{base64(zlib(bits))}` codec.
//! - [`library`]: pure functions over items (add/remove, progress, watched marks,
//!   Continue Watching, sort orders).
//!
//! Semantics are adapted from stremio-core and stremio-watched-bitfield (MIT, Copyright
//! (c) 2019 SmartCode OOD); see THIRD-PARTY-NOTICES.md.

pub mod library;
pub mod types;
pub mod watched;

pub use library::{
    CREDITS_THRESHOLD_COEF, LibraryFilter, LibrarySort, MAX_OTHER_HINTS_BYTES, MetaHints,
    MetaSummary, ResumePoint, VideoRef, WATCHED_THRESHOLD_COEF, advance_to_video,
    apply_playback_progress, apply_seek, clear_progress, finish_playback, is_in_continue_watching,
    mark_season_watched, mark_video_watched, mark_videos_watched, mark_watched, new_item_from_meta,
    next_video_id, ordered_video_ids, prepare_playback, refresh_from_meta, resume_point, select,
    set_in_library, touch, watched_video_ids,
};
pub use types::{
    LibraryItem, LibraryItemBehaviorHints, LibraryItemModified, LibraryItemState, PosterShape,
    format_time, now_from_system, parse_time,
};
pub use watched::{BitField8, WatchedBitField, WatchedError, WatchedField};
