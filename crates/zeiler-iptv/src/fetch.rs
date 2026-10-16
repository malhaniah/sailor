//! Fetching playlists, guides and Xtream answers: bounded in size and time, gzip read
//! when the bytes say so, and errors that never quote a URL (they carry accounts). The
//! limits and the per-status messages follow Harbor's `src/lib/iptv/store.ts`
//! (github.com/harborstremio/harbor), Copyright (c) 2026 Harbor, MIT License — see
//! THIRD-PARTY-NOTICES.md.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::time::Duration;

use flate2::read::MultiGzDecoder;
use reqwest::StatusCode;
use serde_json::Value;
use url::Url;

use crate::xmltv::{self, Program};
use crate::{IptvError, Playlist, m3u};

/// Largest playlist read (after unpacking).
pub const PLAYLIST_MAX_BYTES: usize = 80 * 1024 * 1024;
/// Largest guide downloaded (held in memory while it is read)…
pub const EPG_MAX_BYTES: usize = 100 * 1024 * 1024;
/// …and read once unpacked (unpacking streams into the parser).
const EPG_UNPACKED_MAX_BYTES: u64 = 4 * EPG_MAX_BYTES as u64;
/// Largest Xtream API answer (a channel list of tens of thousands).
const JSON_MAX_BYTES: usize = 40 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(180);

pub struct IptvClient {
    http: reqwest::Client,
}

impl IptvClient {
    pub fn new() -> Result<Self, IptvError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Zeiler/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(TOTAL_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(5))
            // a redirect would otherwise send the previous URL (with its account in the
            // query) to the next host as the Referer
            .referer(false)
            .build()
            .map_err(|e| {
                IptvError::Network(format!(
                    "Could not start the IPTV client: {}",
                    e.without_url()
                ))
            })?;
        Ok(Self { http })
    }

    /// A playlist, from an http(s) URL.
    pub async fn playlist(
        &self,
        url: &Url,
        user_agent: Option<&str>,
    ) -> Result<Playlist, IptvError> {
        let bytes = self.get(url, user_agent, PLAYLIST_MAX_BYTES).await?;
        blocking(move || {
            let bytes = unpack(bytes, PLAYLIST_MAX_BYTES)?;
            let text = String::from_utf8_lossy(&bytes);
            let playlist = m3u::parse(&text);
            // a bare list of stream URLs is a playlist too; anything else is not
            if playlist.channels.is_empty()
                && !text
                    .trim_start_matches('\u{feff}')
                    .trim_start()
                    .starts_with("#EXT")
            {
                return Err(IptvError::Source(
                    "That address did not answer with an M3U playlist.".into(),
                ));
            }
            Ok(playlist)
        })
        .await
    }

    /// A guide's programmes overlapping `from_ms..to_ms`, for the `wanted` guide ids.
    pub async fn guide(
        &self,
        url: &Url,
        user_agent: Option<&str>,
        from_ms: i64,
        to_ms: i64,
        wanted: HashSet<String>,
    ) -> Result<HashMap<String, Vec<Program>>, IptvError> {
        let bytes = self.get(url, user_agent, EPG_MAX_BYTES).await?;
        blocking(move || Ok(parse_guide(&bytes, from_ms, to_ms, Some(&wanted)))).await
    }

    /// A JSON answer (Xtream's API).
    pub async fn json(&self, url: &Url, user_agent: Option<&str>) -> Result<Value, IptvError> {
        let bytes = self.get(url, user_agent, JSON_MAX_BYTES).await?;
        blocking(move || {
            serde_json::from_slice(&bytes).map_err(|_| {
                IptvError::Source("The server did not answer like an Xtream Codes server.".into())
            })
        })
        .await
    }

    async fn get(
        &self,
        url: &Url,
        user_agent: Option<&str>,
        cap: usize,
    ) -> Result<Vec<u8>, IptvError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(IptvError::Invalid(
                "Only http and https addresses can be used.".into(),
            ));
        }
        let mut request = self.http.get(url.clone());
        if let Some(agent) = user_agent.filter(|a| !a.trim().is_empty()) {
            request = request.header(reqwest::header::USER_AGENT, agent.trim());
        }
        let mut response = request.send().await.map_err(network)?;
        let status = response.status();
        if !status.is_success() {
            return Err(IptvError::Source(status_message(status)));
        }
        if response.content_length().is_some_and(|n| n > cap as u64) {
            return Err(too_large(cap));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network)? {
            if body.len() + chunk.len() > cap {
                return Err(too_large(cap));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// Parsing big files is CPU work: off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, IptvError> + Send + 'static,
) -> Result<T, IptvError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| IptvError::Source("Reading the answer failed.".into()))?
}

/// A downloaded guide, gzipped or not (by its bytes), read as a stream.
pub(crate) fn parse_guide(
    bytes: &[u8],
    from_ms: i64,
    to_ms: i64,
    wanted: Option<&HashSet<String>>,
) -> HashMap<String, Vec<Program>> {
    if is_gzip(bytes) {
        let unpacked = MultiGzDecoder::new(bytes).take(EPG_UNPACKED_MAX_BYTES);
        xmltv::parse_window(unpacked, from_ms, to_ms, wanted)
    } else {
        xmltv::parse_window(bytes, from_ms, to_ms, wanted)
    }
}

fn is_gzip(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1f, 0x8b])
}

/// Gunzip when the bytes are gzip (some servers send `.gz` without saying so).
fn unpack(bytes: Vec<u8>, cap: usize) -> Result<Vec<u8>, IptvError> {
    if !is_gzip(&bytes) {
        return Ok(bytes);
    }
    let mut out = Vec::new();
    MultiGzDecoder::new(&bytes[..])
        .take(cap as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| IptvError::Source("The file is damaged (it could not be unpacked).".into()))?;
    if out.len() > cap {
        return Err(too_large(cap));
    }
    Ok(out)
}

fn too_large(cap: usize) -> IptvError {
    IptvError::Source(format!(
        "The file is larger than {} MB.",
        cap / (1024 * 1024)
    ))
}

fn network(error: reqwest::Error) -> IptvError {
    let what = if error.is_timeout() {
        "The server took too long to answer."
    } else if error.is_connect() {
        "Could not connect to the server."
    } else {
        "The download failed."
    };
    IptvError::Network(what.into())
}

fn status_message(status: StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => {
            "The server refused access: check the account, or whether it has expired.".into()
        }
        404 => "Nothing was found at that address.".into(),
        429 => "The server is limiting requests; try again in a while.".into(),
        500..=599 => format!("The server had an error ({}).", status.as_u16()),
        code => format!("The server answered with an error ({code})."),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};

    use super::*;

    #[test]
    fn gzip_is_detected_by_its_bytes_and_capped() {
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(b"#EXTM3U\n").unwrap();
        let packed = gz.finish().unwrap();
        assert_eq!(unpack(packed.clone(), 100).unwrap(), b"#EXTM3U\n");
        assert!(unpack(packed, 4).is_err(), "over the cap after unpacking");
        assert_eq!(unpack(b"plain".to_vec(), 100).unwrap(), b"plain");
    }

    #[test]
    fn a_gzipped_guide_is_read_as_a_stream() {
        let guide = br#"<tv><programme start="20261016180000" stop="20261016190000" channel="a"><title>News</title></programme></tv>"#;
        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(guide).unwrap();
        let packed = gz.finish().unwrap();
        let from = crate::xmltv::parse_time("20261016170000").unwrap();
        let parsed = parse_guide(&packed, from, from + 86_400_000, None);
        assert_eq!(parsed["a"][0].title, "News");
        assert_eq!(parse_guide(guide, from, from + 86_400_000, None), parsed);
    }

    #[test]
    fn status_messages_never_quote_the_address() {
        for code in [401, 403, 404, 429, 500, 503, 418] {
            let message = status_message(StatusCode::from_u16(code).unwrap());
            assert!(
                message.ends_with('.') || message.ends_with(")."),
                "{message}"
            );
            assert!(!message.contains("http"));
        }
    }
}
