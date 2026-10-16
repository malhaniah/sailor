//! User-supplied IPTV for Zeiler's Live room: M3U playlists, Xtream Codes servers and
//! XMLTV guides. Zeiler ships no playlists and seeds none; every source is one the user
//! adds, and where it comes from is their responsibility.
//!
//! Stream URLs and Xtream accounts are secrets (they carry credentials), so nothing here
//! logs them, errors never quote them, and [`Channel`] is not serializable: the shell
//! decides what reaches the webview.

mod fetch;
pub mod m3u;
pub mod xmltv;
pub mod xtream;

pub use fetch::{EPG_MAX_BYTES, IptvClient, PLAYLIST_MAX_BYTES};
pub use xmltv::Program;
pub use xtream::XtreamAccount;

/// One channel of a source.
#[derive(Clone, PartialEq)]
pub struct Channel {
    /// Stable across refreshes (from the guide id, name and group, or the Xtream stream
    /// id; never the URL, which can carry a rotating token) and not reversible.
    pub id: String,
    /// The guide id (`tvg-id` / Xtream `epg_channel_id`).
    pub tvg_id: Option<String>,
    pub name: String,
    pub logo: Option<String>,
    pub group: Option<String>,
    /// The stream (http or https). A secret: it can carry the account.
    pub url: String,
    /// From `#EXTVLCOPT`: some providers need these to answer.
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[derive(Default, Clone, PartialEq)]
pub struct Playlist {
    /// The guide the playlist names (`url-tvg`), if any. Can carry credentials too.
    pub epg_url: Option<String>,
    pub channels: Vec<Channel>,
}

impl std::fmt::Debug for Playlist {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Playlist")
            .field("has_epg", &self.epg_url.is_some())
            .field("channels", &self.channels.len())
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IptvError {
    /// The user's input is not usable (a full sentence).
    #[error("{0}")]
    Invalid(String),
    /// The source answered, but not with what was asked for (a full sentence).
    #[error("{0}")]
    Source(String),
    /// The source could not be reached (a full sentence).
    #[error("{0}")]
    Network(String),
}

/// A short, stable, non-reversible id from some parts (FNV-1a 64).
pub(crate) fn stable_id(parts: &[&str]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.bytes().chain([0xff]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}
