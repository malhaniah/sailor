//! Embedded BitTorrent streaming engine for Sailor, built on librqbit.
//!
//! The engine maps Stremio torrent streams (`infoHash` + `fileIdx` + `sources`) onto a
//! librqbit session and hands the player a loopback HTTP URL with Range support.
//! Sailor ships no content and no trackers of its own: every torrent comes from a stream
//! object served by an addon the user installed.

mod engine;
mod magnet;

pub use engine::{Engine, EngineConfig, ResolvedStream, TorrentStreamRequest};
pub use magnet::{build_magnet, normalize_info_hash};
