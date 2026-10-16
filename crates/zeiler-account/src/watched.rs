//! The watched-episodes bitfield of a library item (`state.watched`).
//!
//! Ported from stremio-watched-bitfield (part of stremio-core,
//! <https://github.com/Stremio/stremio-core>), MIT License, Copyright (c) 2019 SmartCode
//! OOD — see THIRD-PARTY-NOTICES.md for the license text.
//!
//! Wire format: `{anchorVideoId}:{anchorLength}:{base64(zlib(bitfield8))}`.
//!
//! - Bit *i* (little-endian within each byte) is video *i* of the meta's videos ordered
//!   by (season, episode, released) — see [`crate::ordered_video_ids`].
//! - The anchor is the last watched video and `anchorLength` its index + 1. Video lists
//!   grow (new episodes) and shift (specials inserted), so the bits are re-aligned on
//!   read by finding the anchor in the current list ("resize on anchor").
//! - Video ids contain `:` (`tt0944947:1:5`), so the string is parsed from the right.
//!
//! Differences from the original, none of them visible on the wire: decoding is capped
//! (a hostile string cannot inflate into gigabytes), base64 padding is optional on
//! input, and offsets use checked signed arithmetic instead of `i32` casts.

use std::fmt;
use std::io::{Read, Write};
use std::str::FromStr;

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

/// Standard alphabet; padding written, optional when reading (JavaScript clients and
/// older base64 crates disagree about it).
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Upper bound for a decoded bitfield: 1 MiB is 8 million videos.
const MAX_DECODED_BYTES: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WatchedError {
    #[error("the watched field is malformed: {0}")]
    Format(&'static str),
    #[error("the watched bitfield is not valid base64")]
    Base64,
    #[error("the watched bitfield is not valid zlib data")]
    Zlib,
    #[error("the watched bitfield is too large")]
    TooLarge,
}

/// A growable bitfield, `length` bits stored in `ceil(length / 8)` bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitField8 {
    pub length: usize,
    values: Vec<u8>,
}

impl BitField8 {
    pub fn new(length: usize) -> Self {
        Self {
            length,
            values: vec![0; length.div_ceil(8)],
        }
    }

    /// From raw bytes; `length` defaults to all their bits, and the bytes are padded
    /// with zeros when `length` needs more.
    pub fn new_with_values(mut values: Vec<u8>, length: Option<usize>) -> Self {
        let length = length.unwrap_or(values.len() * 8);
        let bytes = length.div_ceil(8);
        if bytes > values.len() {
            values.resize(bytes, 0);
        }
        Self { length, values }
    }

    pub fn get(&self, i: usize) -> bool {
        self.values
            .get(i / 8)
            .is_some_and(|byte| (byte >> (i % 8)) & 1 != 0)
    }

    /// Set bit `i`, growing the field if needed (the length then becomes a whole
    /// number of bytes, as in the original).
    pub fn set(&mut self, i: usize, value: bool) {
        let index = i / 8;
        let mask = 1 << (i % 8);
        if index >= self.values.len() {
            self.values.resize(index + 1, 0);
            self.length = self.values.len() * 8;
        }
        if value {
            self.values[index] |= mask;
        } else {
            self.values[index] &= !mask;
        }
    }

    /// The last index (below `length`) whose bit equals `value`.
    pub fn last_index_of(&self, value: bool) -> Option<usize> {
        (0..self.length).rev().find(|&i| self.get(i) == value)
    }

    /// `base64(zlib(bytes))`, zlib level 6 like the original.
    pub fn encode(&self) -> String {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(6));
        // writing into a Vec cannot fail
        let compressed = encoder
            .write_all(&self.values)
            .and_then(|()| encoder.finish())
            .unwrap_or_default();
        BASE64.encode(compressed)
    }

    pub fn decode(encoded: &str, length: Option<usize>) -> Result<Self, WatchedError> {
        let compressed = BASE64
            .decode(encoded.trim())
            .map_err(|_| WatchedError::Base64)?;
        let mut values = Vec::new();
        ZlibDecoder::new(&compressed[..])
            .take(MAX_DECODED_BYTES + 1)
            .read_to_end(&mut values)
            .map_err(|_| WatchedError::Zlib)?;
        if values.len() as u64 > MAX_DECODED_BYTES {
            return Err(WatchedError::TooLarge);
        }
        Ok(Self::new_with_values(values, length))
    }
}

/// The serialized form, parsed without knowing the meta's videos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchedField {
    /// The last watched video id (`undefined` when nothing is watched).
    pub anchor_video: String,
    /// Index of the anchor + 1.
    pub anchor_length: usize,
    pub bitfield: BitField8,
}

impl fmt::Display for WatchedField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.anchor_video,
            self.anchor_length,
            self.bitfield.encode()
        )
    }
}

impl FromStr for WatchedField {
    type Err = WatchedError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // {id}:{len}:{buf}, where {id} may itself contain ':' — pop from the right
        let mut parts = s.rsplitn(3, ':');
        let buf = parts.next().ok_or(WatchedError::Format("no bitfield"))?;
        let length = parts
            .next()
            .ok_or(WatchedError::Format("no anchor length"))?;
        let anchor = parts
            .next()
            .ok_or(WatchedError::Format("no anchor video"))?;
        let anchor_length = length
            .parse::<usize>()
            .map_err(|_| WatchedError::Format("the anchor length is not a number"))?;
        Ok(Self {
            anchor_video: anchor.to_string(),
            anchor_length,
            bitfield: BitField8::decode(buf, None)?,
        })
    }
}

impl From<WatchedBitField> for WatchedField {
    fn from(watched: WatchedBitField) -> Self {
        let last = watched.bitfield.last_index_of(true).unwrap_or(0);
        Self {
            anchor_video: watched
                .video_ids
                .get(last)
                .cloned()
                .unwrap_or_else(|| "undefined".to_string()),
            anchor_length: last + 1,
            bitfield: watched.bitfield,
        }
    }
}

/// A bitfield aligned to a concrete, ordered list of video ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchedBitField {
    bitfield: BitField8,
    video_ids: Vec<String>,
}

impl WatchedBitField {
    /// All unwatched (or `values[i]` for the first entries).
    pub fn construct_from_array(values: &[bool], video_ids: Vec<String>) -> Self {
        let mut bitfield = BitField8::new(video_ids.len());
        for (i, &value) in values.iter().enumerate() {
            bitfield.set(i, value);
        }
        Self {
            bitfield,
            video_ids,
        }
    }

    /// Align a parsed field to `video_ids` by its anchor. When the anchor is not in the
    /// list the result is blank (as in the original).
    pub fn construct_with_videos(watched: WatchedField, video_ids: Vec<String>) -> Self {
        let Some(anchor_index) = video_ids.iter().position(|id| *id == watched.anchor_video) else {
            return Self::construct_from_array(&[], video_ids);
        };
        // an anchor length too large to shift by is as good as an unknown anchor
        let Some(offset) = i64::try_from(watched.anchor_length)
            .ok()
            .and_then(|length| length.checked_sub(anchor_index as i64 + 1))
        else {
            return Self::construct_from_array(&[], video_ids);
        };
        let bitfield = BitField8::new_with_values(watched.bitfield.values, Some(video_ids.len()));
        if offset == 0 {
            return Self {
                bitfield,
                video_ids,
            };
        }
        let mut resized = Self::construct_from_array(&[], video_ids);
        for i in 0..resized.video_ids.len() {
            let Some(previous) = (i as i64).checked_add(offset) else {
                continue;
            };
            if let Ok(previous) = usize::try_from(previous)
                && previous < bitfield.length
            {
                resized.set(i, bitfield.get(previous));
            }
        }
        resized
    }

    /// Parse `serialized` and align it to `video_ids`.
    pub fn construct_and_resize(
        serialized: &str,
        video_ids: Vec<String>,
    ) -> Result<Self, WatchedError> {
        Ok(Self::construct_with_videos(serialized.parse()?, video_ids))
    }

    pub fn video_ids(&self) -> &[String] {
        &self.video_ids
    }

    pub fn get(&self, index: usize) -> bool {
        self.bitfield.get(index)
    }

    pub fn get_video(&self, video_id: &str) -> bool {
        self.video_ids
            .iter()
            .position(|id| id == video_id)
            .is_some_and(|i| self.bitfield.get(i))
    }

    pub fn set(&mut self, index: usize, value: bool) {
        self.bitfield.set(index, value);
    }

    /// Set `video_id`'s bit; returns false (and changes nothing) when it is not in the
    /// list.
    pub fn set_video(&mut self, video_id: &str, value: bool) -> bool {
        match self.video_ids.iter().position(|id| id == video_id) {
            Some(i) => {
                self.bitfield.set(i, value);
                true
            }
            None => false,
        }
    }

    /// Whether any video is marked watched.
    pub fn any(&self) -> bool {
        self.bitfield.last_index_of(true).is_some()
    }
}

impl fmt::Display for WatchedBitField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let last = self.bitfield.last_index_of(true).unwrap_or(0);
        let anchor = self.video_ids.get(last).map_or("undefined", String::as_str);
        write!(f, "{anchor}:{}:{}", last + 1, self.bitfield.encode())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(prefix: &str, range: std::ops::RangeInclusive<u32>) -> Vec<String> {
        range.map(|i| format!("{prefix}{i}")).collect()
    }

    // --- the original crate's tests -----------------------------------------------------

    #[test]
    fn bitfield8_decodes_and_encodes_the_reference_string() {
        let bf = BitField8::decode("eJyTZwAAAEAAIA==", None).unwrap();
        assert_eq!(bf.length, 16);
        assert_eq!(bf.values, vec![31, 0]);
        assert_eq!(bf.encode(), "eJyTZwAAAEAAIA==");
        // a custom length keeps the bytes, so the encoding is the same
        let short = BitField8::decode("eJyTZwAAAEAAIA==", Some(9)).unwrap();
        assert_eq!(short.length, 9);
        assert_eq!(short.encode(), "eJyTZwAAAEAAIA==");
    }

    #[test]
    fn bitfield8_new_uses_the_bit_length() {
        let mut bf = BitField8::new(20);
        assert_eq!(bf.length, 20);
        bf.set(15, true);
        assert_eq!(bf.last_index_of(true), Some(15));
    }

    #[test]
    fn parse_and_modify() {
        let videos = ids("tt2934286:1:", 1..=9);
        let watched = "tt2934286:1:5:5:eJyTZwAAAEAAIA==";
        let mut wb = WatchedBitField::construct_and_resize(watched, videos).unwrap();

        assert!(wb.get_video("tt2934286:1:5"));
        assert!(!wb.get_video("tt2934286:1:6"));
        assert_eq!(wb.to_string(), watched);

        assert!(wb.set_video("tt2934286:1:6", true));
        assert!(wb.get_video("tt2934286:1:6"));
        assert!(!wb.set_video("tt2934286:9:9", true), "not in the list");
    }

    #[test]
    fn construct_from_array_round_trips() {
        let video_ids = ids("tt2934286:1:", 1..=499);
        let mut wb = WatchedBitField::construct_from_array(&[false; 500], video_ids.clone());
        assert!(video_ids.iter().all(|id| !wb.get_video(id)));
        for i in 0..video_ids.len() {
            wb.set(i, i % 2 == 0);
        }
        let back =
            WatchedBitField::construct_and_resize(&wb.to_string(), video_ids.clone()).unwrap();
        for (i, id) in video_ids.iter().enumerate() {
            assert_eq!(back.get(i), i % 2 == 0);
            assert_eq!(back.get_video(id), i % 2 == 0);
        }
    }

    #[test]
    fn empty_serializes_to_the_known_string() {
        let empty = WatchedBitField::construct_from_array(&[], vec![]);
        assert_eq!(empty.to_string(), "undefined:1:eJwDAAAAAAE=");
        let back = WatchedBitField::construct_and_resize("undefined:1:eJwDAAAAAAE=", vec![]);
        assert_eq!(back, Ok(WatchedBitField::construct_from_array(&[], vec![])));
    }

    #[test]
    fn watched_field_parses_series_anchors_from_the_right() {
        let field: WatchedField = "tt7767422:3:8:24:eJz7//8/AAX9Av4=".parse().unwrap();
        assert_eq!(field.anchor_video, "tt7767422:3:8");
        assert_eq!(field.anchor_length, 24);
        assert_eq!(field.bitfield.encode(), "eJz7//8/AAX9Av4=");
        assert_eq!(field.to_string(), "tt7767422:3:8:24:eJz7//8/AAX9Av4=");
    }

    #[test]
    fn anchor_points_to_the_last_watched_video() {
        let video_ids = ids("tt0:1:", 1..=20);
        let mut watched = WatchedBitField::construct_from_array(&[], video_ids);
        watched.set_video("tt0:1:16", true);
        let field: WatchedField = watched.into();
        assert!(field.to_string().starts_with("tt0:1:16:16:"), "{field}");
    }

    // --- resize on anchor -----------------------------------------------------------

    #[test]
    fn bits_follow_the_anchor_when_videos_are_inserted_before_it() {
        // watched 1..=5 of season 1
        let before = ids("tt1:1:", 1..=9);
        let mut wb = WatchedBitField::construct_from_array(&[], before);
        for i in 1..=5 {
            wb.set_video(&format!("tt1:1:{i}"), true);
        }
        let stored = wb.to_string();
        assert!(stored.starts_with("tt1:1:5:5:"));

        // two specials (season 0) now sort first
        let mut after = vec!["tt1:0:1".to_string(), "tt1:0:2".to_string()];
        after.extend(ids("tt1:1:", 1..=10));
        let realigned = WatchedBitField::construct_and_resize(&stored, after).unwrap();
        assert!(!realigned.get_video("tt1:0:1"));
        assert!(!realigned.get_video("tt1:0:2"));
        for i in 1..=5 {
            assert!(realigned.get_video(&format!("tt1:1:{i}")), "episode {i}");
        }
        assert!(!realigned.get_video("tt1:1:6"));
        assert!(!realigned.get_video("tt1:1:10"));
    }

    #[test]
    fn an_unknown_anchor_gives_a_blank_field() {
        let wb = WatchedBitField::construct_and_resize(
            "tt9:9:9:3:eJyTZwAAAEAAIA==",
            ids("tt1:1:", 1..=4),
        )
        .unwrap();
        assert!(!wb.any());
    }

    #[test]
    fn an_absurd_anchor_length_gives_a_blank_field_not_an_overflow() {
        for length in [
            "9223372036854775808",
            "18446744073709551615",
            "9223372036854775807",
        ] {
            let wb = WatchedBitField::construct_and_resize(
                &format!("tt1:1:{length}:eJyTZwAAAEAAIA=="),
                ids("tt1:", 1..=3),
            )
            .unwrap();
            assert!(!wb.any(), "{length}");
        }
    }

    #[test]
    fn malformed_strings_are_errors_not_panics() {
        for bad in [
            "",
            "abc",
            "a:b",
            "tt1:x:eJyTZwAAAEAAIA==",
            "tt1:1:not base64!",
            "tt1:1:aGVsbG8=", // base64, but not zlib
        ] {
            assert!(bad.parse::<WatchedField>().is_err(), "{bad:?}");
        }
        // unpadded base64 is accepted
        let unpadded: WatchedField = "tt1:1:5:5:eJyTZwAAAEAAIA".parse().unwrap();
        assert_eq!(unpadded.bitfield.values, vec![31, 0]);
    }

    #[test]
    fn decoding_is_capped() {
        // 2 MiB of zeros compress to a few KiB
        let huge = BitField8::new((MAX_DECODED_BYTES as usize + (1 << 20)) * 8).encode();
        assert!(huge.len() < 16 * 1024);
        assert_eq!(BitField8::decode(&huge, None), Err(WatchedError::TooLarge));
    }
}
