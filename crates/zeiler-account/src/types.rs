//! Library item types, field for field compatible with stremio-core's
//! `types/library/library_item.rs`.
//!
//! Adapted from stremio-core (<https://github.com/Stremio/stremio-core>), MIT License,
//! Copyright (c) 2019 SmartCode OOD — see THIRD-PARTY-NOTICES.md for the license text.
//!
//! **Lenient in.** Library data has been written by many clients over the years (some of
//! them buggy, e.g. floats or nulls in integer fields), so every field reads leniently:
//! a missing, null or wrongly typed value becomes that field's default instead of
//! failing the whole item; numbers given as floats or strings are truncated to
//! integers; an unparseable `watched` string or poster URL reads as absent.
//!
//! **Strict out.** What we write is exactly what stremio-core writes: the same field
//! names (including the snake_case `video_id` among camelCase siblings), the same field
//! order, integers for every numeric field (never floats, never null), RFC 3339
//! timestamps in UTC with a `Z` suffix (chrono's own format: `AutoSi` precision), and
//! `null` for absent optional values.
//!
//! **Unknown fields survive.** Each level (item, state, behaviour hints) keeps the fields
//! it does not know in `extra` (serde `flatten`), and writes them back after its own.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use url::Url;

use crate::watched::WatchedField;

/// stremio-core's `PosterShape`: unknown values read as `poster`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PosterShape {
    Square,
    Landscape,
    #[default]
    Poster,
}

impl<'de> Deserialize<'de> for PosterShape {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Value::deserialize(deserializer)? {
            Value::String(s) if s == "square" => PosterShape::Square,
            Value::String(s) if s == "landscape" => PosterShape::Landscape,
            _ => PosterShape::Poster,
        })
    }
}

/// One title in the user's library (`libraryItem` in Stremio's datastore).
///
/// `removed`/`temp` encode where it shows up (stremio-core semantics):
///
/// | removed | temp  | meaning |
/// | ------- | ----- | ------- |
/// | false   | false | in the library |
/// | true    | true  | played but never added: only Continue Watching / history |
/// | true    | false | removed by the user |
/// | false   | true  | in the library and played before it was added |
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    #[serde(rename = "_id", deserialize_with = "lenient::string")]
    pub id: String,
    #[serde(default, deserialize_with = "lenient::string")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient::string")]
    pub r#type: String,
    #[serde(default, deserialize_with = "lenient::opt_url")]
    pub poster: Option<Url>,
    #[serde(default)]
    pub poster_shape: PosterShape,
    #[serde(default, deserialize_with = "lenient::boolean")]
    pub removed: bool,
    #[serde(default, deserialize_with = "lenient::boolean")]
    pub temp: bool,
    /// Creation time.
    #[serde(
        rename = "_ctime",
        default,
        deserialize_with = "lenient::opt_time",
        serialize_with = "out::opt_time"
    )]
    pub ctime: Option<DateTime<Utc>>,
    /// Modification time: last-writer-wins key for sync (set by [`crate::touch`]).
    #[serde(
        rename = "_mtime",
        default = "epoch",
        deserialize_with = "lenient::time",
        serialize_with = "out::time"
    )]
    pub mtime: DateTime<Utc>,
    #[serde(default, deserialize_with = "lenient::object")]
    pub state: LibraryItemState,
    #[serde(default, deserialize_with = "lenient::object")]
    pub behavior_hints: LibraryItemBehaviorHints,
    /// Fields this version does not know, written back unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl LibraryItem {
    /// Live TV / channels: never tracked. Zeiler's rule (the same as its UI):
    /// `behaviorHints.isLive`, or the `tv` type.
    pub fn is_live(&self) -> bool {
        self.behavior_hints.is_live || self.r#type == "tv"
    }

    /// Whether it has been watched at least once (stremio-core `watched()`).
    pub fn watched(&self) -> bool {
        self.state.times_watched > 0
    }

    /// Equal apart from `_mtime` (stremio-core `eq_no_mtime`): decides whether a change
    /// is a change at all.
    pub fn eq_no_mtime(&self, other: &LibraryItem) -> bool {
        self.id == other.id
            && self.removed == other.removed
            && self.temp == other.temp
            && self.ctime == other.ctime
            && self.state == other.state
            && self.name == other.name
            && self.r#type == other.r#type
            && self.poster == other.poster
            && self.poster_shape == other.poster_shape
            && self.behavior_hints == other.behavior_hints
            && self.extra == other.extra
    }
}

/// Playback state of a [`LibraryItem`]. Times are milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItemState {
    /// Last time this item was played or marked watched.
    #[serde(
        default,
        deserialize_with = "lenient::opt_time",
        serialize_with = "out::opt_time"
    )]
    pub last_watched: Option<DateTime<Utc>>,
    /// Time watched of the current video (`video_id`).
    #[serde(default, deserialize_with = "lenient::uint")]
    pub time_watched: u64,
    /// Resume position in the current video; 0 = nothing to resume.
    #[serde(default, deserialize_with = "lenient::uint")]
    pub time_offset: u64,
    /// Time watched across all videos.
    #[serde(default, deserialize_with = "lenient::uint")]
    pub overall_time_watched: u64,
    /// Incremented once per video that crossed the watched threshold, and by "mark as
    /// watched".
    #[serde(default, deserialize_with = "lenient::uint32")]
    pub times_watched: u32,
    /// 1 once the current video crossed the watched threshold.
    #[serde(default, deserialize_with = "lenient::uint32")]
    pub flagged_watched: u32,
    /// Duration of the current video.
    #[serde(default, deserialize_with = "lenient::uint")]
    pub duration: u64,
    /// The video last played (an episode id, or the meta id for a movie). Spelled
    /// `video_id` on the wire, unlike its camelCase siblings — as in stremio-core.
    #[serde(rename = "video_id", default, deserialize_with = "lenient::opt_string")]
    pub video_id: Option<String>,
    /// Watched episodes, as a [`WatchedField`] string. Kept verbatim (so an unchanged
    /// item round-trips byte for byte); a string that does not parse reads as `None`.
    #[serde(default, deserialize_with = "lenient::opt_watched")]
    pub watched: Option<String>,
    /// Opt out of new-episode notifications.
    #[serde(default, deserialize_with = "lenient::boolean")]
    pub no_notif: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// stremio-core's `MetaItemBehaviorHints` as stored on a library item (its field order:
/// `isLive` first).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItemBehaviorHints {
    /// Written only when true (as stremio-core does).
    #[serde(
        default,
        deserialize_with = "lenient::boolean",
        skip_serializing_if = "is_false"
    )]
    pub is_live: bool,
    #[serde(default, deserialize_with = "lenient::opt_string")]
    pub default_video_id: Option<String>,
    #[serde(default, deserialize_with = "lenient::opt_string")]
    pub featured_video_id: Option<String>,
    #[serde(default, deserialize_with = "lenient::boolean")]
    pub has_scheduled_videos: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `[id, mtimeMillis]`, one entry of Stremio's `datastoreMeta` answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LibraryItemModified(pub String, pub i64);

impl<'de> Deserialize<'de> for LibraryItemModified {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (id, mtime) = <(String, Value)>::deserialize(deserializer)?;
        let millis = match &mtime {
            Value::String(s) => parse_time(s).map(|t| t.timestamp_millis()),
            other => number_to_u64(other).and_then(|n| i64::try_from(n).ok()),
        };
        Ok(LibraryItemModified(id, millis.unwrap_or(0)))
    }
}

/// URLs (posters) longer than this read as none: no real image URL is, and a hostile
/// one must not bloat every write of the library.
pub const MAX_URL_LEN: usize = 4096;

fn is_false(b: &bool) -> bool {
    !*b
}

fn epoch() -> DateTime<Utc> {
    DateTime::UNIX_EPOCH
}

/// chrono's own RFC 3339 form (what stremio-core writes): UTC, `Z`, fractional seconds
/// only as precise as needed.
pub fn format_time(time: &DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// RFC 3339 (any offset), as UTC.
pub fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// The current time at millisecond precision (like JavaScript's `Date`), so stored
/// timestamps match what Stremio's own apps write. Callers pass it into the pure
/// functions of this crate.
pub fn now_from_system(now: std::time::SystemTime) -> DateTime<Utc> {
    let millis = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    DateTime::from_timestamp_millis(millis).unwrap_or(DateTime::UNIX_EPOCH)
}

/// A non-negative integer from a JSON number (floats truncate), numeric string or bool.
fn number_to_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite())
                // `as` saturates: negative → 0, huge → u64::MAX
                .map(|f| f.trunc() as u64)
        }),
        Value::String(s) => {
            let s = s.trim();
            s.parse::<u64>()
                .ok()
                .or_else(|| number_to_u64(&serde_json::from_str::<Value>(s).ok()?))
        }
        Value::Bool(b) => Some(u64::from(*b)),
        _ => None,
    }
}

pub(crate) mod lenient {
    use super::*;

    pub fn string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::String(s) => s,
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
    }

    pub fn opt_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::String(s) if !s.is_empty() => Some(s),
            _ => None,
        })
    }

    pub fn opt_url<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Url>, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::String(s) if s.len() <= MAX_URL_LEN => Url::parse(&s)
                .ok()
                .filter(|url| url.as_str().len() <= MAX_URL_LEN),
            _ => None,
        })
    }

    pub fn opt_watched<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::String(s) if s.parse::<WatchedField>().is_ok() => Some(s),
            _ => None,
        })
    }

    pub fn boolean<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
        Ok(match Value::deserialize(d)? {
            Value::Bool(b) => b,
            Value::String(s) => s == "true" || s == "1",
            other => number_to_u64(&other).is_some_and(|n| n != 0),
        })
    }

    pub fn uint<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        Ok(number_to_u64(&Value::deserialize(d)?).unwrap_or(0))
    }

    pub fn uint32<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
        Ok(number_to_u64(&Value::deserialize(d)?)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX)))
    }

    fn any_time(value: &Value) -> Option<DateTime<Utc>> {
        match value {
            Value::String(s) => parse_time(s),
            // epoch milliseconds (as datastoreMeta reports them)
            Value::Number(_) => number_to_u64(value)
                .and_then(|n| i64::try_from(n).ok())
                .and_then(DateTime::from_timestamp_millis),
            _ => None,
        }
    }

    pub fn opt_time<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        Ok(any_time(&Value::deserialize(d)?))
    }

    pub fn time<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
        Ok(any_time(&Value::deserialize(d)?).unwrap_or_else(epoch))
    }

    /// A nested object read leniently; anything that is not an object (or fails anyway)
    /// becomes the default.
    pub fn object<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: serde::de::DeserializeOwned + Default,
    {
        Ok(match Value::deserialize(d)? {
            value @ Value::Object(_) => serde_json::from_value(value).unwrap_or_default(),
            _ => T::default(),
        })
    }
}

mod out {
    use super::*;

    pub fn time<S: Serializer>(time: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format_time(time))
    }

    pub fn opt_time<S: Serializer>(time: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match time {
            Some(time) => s.serialize_str(&format_time(time)),
            None => s.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An item as stremio-core writes it (field order, integers, `video_id`, `Z` times).
    const GOLDEN: &str = include_str!("../tests/fixtures/library_item.json");
    /// The same item as old/buggy clients wrote it: floats, numeric strings, nulls,
    /// missing fields, an invalid poster URL and watched string, plus unknown fields.
    const MESSY: &str = include_str!("../tests/fixtures/library_item_messy.json");

    #[test]
    fn golden_item_round_trips_byte_for_byte() {
        let golden = GOLDEN.trim_end();
        let item: LibraryItem = serde_json::from_str(golden).unwrap();
        assert_eq!(item.id, "tt0944947");
        assert_eq!(item.state.video_id.as_deref(), Some("tt0944947:1:3"));
        assert_eq!(item.state.time_offset, 1_900_000);
        assert_eq!(
            item.mtime,
            parse_time("2023-05-14T12:34:56.789Z").unwrap(),
            "fractional seconds are kept"
        );
        assert_eq!(serde_json::to_string(&item).unwrap(), golden);
    }

    #[test]
    fn messy_input_reads_leniently_and_writes_strictly() {
        let item: LibraryItem = serde_json::from_str(MESSY).unwrap();
        let expected = include_str!("../tests/fixtures/library_item_messy.out.json");
        assert_eq!(serde_json::to_string(&item).unwrap(), expected.trim_end());
    }

    #[test]
    fn integers_are_never_written_as_floats_or_nulls() {
        let item: LibraryItem = serde_json::from_value(json!({
            "_id": "tt1", "name": "A", "type": "movie", "_mtime": "2024-01-01T00:00:00Z",
            "state": {"timeOffset": 12.9, "timeWatched": null, "duration": "5400000.0",
                      "timesWatched": -3, "flaggedWatched": true}
        }))
        .unwrap();
        let state = serde_json::to_value(&item).unwrap()["state"].clone();
        assert_eq!(state["timeOffset"], json!(12));
        assert!(state["timeOffset"].is_u64());
        assert_eq!(state["timeWatched"], json!(0));
        assert_eq!(state["duration"], json!(5_400_000));
        assert_eq!(state["timesWatched"], json!(0));
        assert_eq!(state["flaggedWatched"], json!(1));
        assert_eq!(
            state["overallTimeWatched"],
            json!(0),
            "missing → 0, not null"
        );
    }

    #[test]
    fn unknown_fields_survive_at_every_level() {
        let item: LibraryItem = serde_json::from_value(json!({
            "_id": "tt1", "name": "A", "type": "movie", "_mtime": "2024-01-01T00:00:00Z",
            "aFutureField": [1, 2],
            "state": {"zSomething": {"deep": true}},
            "behaviorHints": {"futureHint": "x"}
        }))
        .unwrap();
        let back = serde_json::to_value(&item).unwrap();
        assert_eq!(back["aFutureField"], json!([1, 2]));
        assert_eq!(back["state"]["zSomething"], json!({"deep": true}));
        assert_eq!(back["behaviorHints"]["futureHint"], json!("x"));
        assert!(
            back["behaviorHints"].get("isLive").is_none(),
            "false isLive is omitted"
        );
    }

    #[test]
    fn library_item_modified_reads_millis_or_dates() {
        let entries: Vec<LibraryItemModified> = serde_json::from_value(json!([
            ["tt1", 1_700_000_000_000_i64],
            ["tt2", 1_700_000_000_000.0],
            ["tt3", "2023-11-14T22:13:20Z"],
            ["tt4", null]
        ]))
        .unwrap();
        assert_eq!(
            entries[0],
            LibraryItemModified("tt1".into(), 1_700_000_000_000)
        );
        assert_eq!(entries[1].1, 1_700_000_000_000);
        assert_eq!(entries[2].1, 1_700_000_000_000);
        assert_eq!(entries[3].1, 0);
        assert_eq!(
            serde_json::to_string(&entries[0]).unwrap(),
            r#"["tt1",1700000000000]"#
        );
    }

    #[test]
    fn times_use_chronos_rfc3339_form() {
        let whole = parse_time("2024-02-03T04:05:06+02:00").unwrap();
        assert_eq!(format_time(&whole), "2024-02-03T02:05:06Z");
        let millis = DateTime::from_timestamp_millis(1_700_000_000_123).unwrap();
        assert_eq!(format_time(&millis), "2023-11-14T22:13:20.123Z");
        let now = now_from_system(
            std::time::UNIX_EPOCH + std::time::Duration::from_nanos(1_700_000_000_123_456_789),
        );
        assert_eq!(now, millis, "now is truncated to milliseconds");
    }
}
