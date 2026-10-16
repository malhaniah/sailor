//! Readable text for libmpv's client-API error codes (`enum mpv_error` in
//! mpv's client.h).
//!
//! libmpv2's `Error` displays as its `Debug` form (`Raw(-13)`), which means
//! nothing to the person watching. The codes are part of mpv's stable client
//! ABI, so this is plain data: always compiled and tested, independent of the
//! `embedded` feature that actually links libmpv.

/// Describe an `mpv_error` code as a lowercase clause that reads on its own
/// or after a colon. Worded for the viewer rather than for mpv developers
/// (compare `mpv_error_string`). Arms follow client.h order, from
/// `MPV_ERROR_SUCCESS` (0) down to `MPV_ERROR_GENERIC` (-20).
pub fn describe(code: i32) -> &'static str {
    match code {
        0 => "success",
        -1 => "the player's event queue is full",
        -2 => "out of memory",
        -3 => "the player is not initialized",
        -4 => "invalid parameter",
        -5 => "unknown option",
        -6 => "option value not accepted (wrong type or out of range)",
        -7 => "invalid option value",
        -8 => "unknown property",
        -9 => "property value not accepted (wrong type, out of range or unparseable)",
        -10 => "property not available right now",
        -11 => "the property could not be read or set",
        -12 => "the player command failed",
        -13 => "the stream could not be opened",
        -14 => "no audio output could be opened",
        -15 => "the video output could not be initialized",
        -16 => "the stream has no playable audio or video",
        -17 => "unrecognized or damaged media format",
        -18 => "not supported on this system",
        -19 => "not implemented by this libmpv",
        -20 => "unspecified player error",
        _ => "unknown player error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_documented_code_has_its_own_text() {
        let texts: Vec<&str> = (-20..=0).map(describe).collect();
        assert!(texts.iter().all(|t| *t != describe(i32::MIN)));
        let mut unique = texts.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), texts.len(), "two codes share a text");
    }

    #[test]
    fn loading_failed_is_readable() {
        assert_eq!(describe(-13), "the stream could not be opened");
    }

    #[test]
    fn unknown_codes_fall_back() {
        assert_eq!(describe(-21), "unknown player error");
        assert_eq!(describe(7), "unknown player error");
    }
}
