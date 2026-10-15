use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

/// Query-component encode set for `tr=` values inside a magnet URI: encode everything
/// that could terminate or confuse the query component (space, `&`, `=`, `#`, `+`, `%`).
const QUERY_VALUE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'=')
    .add(b'+')
    .add(b'?')
    .add(b'\'');

#[derive(Debug, thiserror::Error)]
#[error("invalid infoHash: expected 40 hex characters")]
pub struct InvalidInfoHash;

/// Stremio infoHashes must be 40-char hex; accept any case, return lowercase.
pub fn normalize_info_hash(info_hash: &str) -> Result<String, InvalidInfoHash> {
    let trimmed = info_hash.trim();
    if trimmed.len() == 40 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(trimmed.to_ascii_lowercase())
    } else {
        Err(InvalidInfoHash)
    }
}

/// Build a magnet URI from an infoHash and Stremio `sources` entries.
///
/// `sources` entries look like `tracker:udp://host:port/announce` or `dht:<hash>`;
/// bare tracker URLs (no `tracker:` prefix) also occur in the wild. Trackers must be
/// folded into the magnet because librqbit ignores `AddTorrentOptions.trackers` for
/// magnet adds; `dht:` entries are dropped (DHT is a session-level switch).
pub fn build_magnet(info_hash: &str, sources: &[String]) -> Result<String, InvalidInfoHash> {
    let hash = normalize_info_hash(info_hash)?;
    let mut magnet = format!("magnet:?xt=urn:btih:{hash}");
    for tracker in sources.iter().filter_map(|s| tracker_url(s)) {
        magnet.push_str("&tr=");
        magnet.push_str(&utf8_percent_encode(tracker, QUERY_VALUE).to_string());
    }
    Ok(magnet)
}

fn tracker_url(source: &str) -> Option<&str> {
    let source = source.trim();
    if let Some(rest) = source.strip_prefix("tracker:") {
        return Some(rest);
    }
    if source.starts_with("dht:") {
        return None;
    }
    // Bare tracker URL without the prefix.
    if source.contains("://") {
        Some(source)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_hash_case() {
        assert_eq!(
            normalize_info_hash("AAAAAAAAAAAAAAAAAAAABBBBBBBBBBBBBBBBBBBB").unwrap(),
            "aaaaaaaaaaaaaaaaaaaabbbbbbbbbbbbbbbbbbbb"
        );
        assert!(normalize_info_hash("short").is_err());
        assert!(normalize_info_hash("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn builds_magnet_with_trackers() {
        let m = build_magnet(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            &[
                "tracker:udp://tracker.opentrackr.org:1337/announce".to_string(),
                "dht:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                "http://bare.example:6969/announce?passkey=a&b=c".to_string(),
                "not a url".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(
            m,
            "magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\
             &tr=udp://tracker.opentrackr.org:1337/announce\
             &tr=http://bare.example:6969/announce%3Fpasskey%3Da%26b%3Dc"
        );
    }
}
