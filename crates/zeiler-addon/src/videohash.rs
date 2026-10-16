//! The OpenSubtitles hash of a video, for the `videoHash` subtitle extra when a stream
//! does not give one (Stremio's own server computes it the same way).
//!
//! The hash is the file size plus the wrapping sum of the little-endian 64-bit words of
//! the first and the last 64 KiB, as 16 hex digits. It is read with two HTTP range
//! requests against the stream itself (a direct link with its own headers, or the
//! engine's loopback URL for a torrent), so nothing but those 128 KiB is downloaded; a
//! server that ignores ranges is given up on after the first chunk.

use url::Url;

/// How much of each end the hash reads.
pub const HASH_CHUNK: u64 = 64 * 1024;

/// A video's OpenSubtitles hash and size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoHash {
    /// 16 lowercase hex digits.
    pub hash: String,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum VideoHashError {
    #[error("only http and https videos can be hashed")]
    UnsupportedScheme,
    /// (without the URL, which may carry a token: the error is logged)
    #[error("the request failed: {0}")]
    Http(reqwest::Error),
    #[error("it took too long")]
    Timeout,
    #[error("the server answered {0}")]
    Status(u16),
    #[error("the server does not serve parts of the file")]
    NoRanges,
    #[error("the file is too small to hash")]
    TooSmall,
}

impl From<reqwest::Error> for VideoHashError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e.without_url())
    }
}

/// Where the video is and how to ask for it.
#[derive(Debug, Clone, Default)]
pub struct HashTarget {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
}

/// Both range requests together may take this long (a torrent's last pieces come from
/// peers).
const HASH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Hashes videos (one HTTP client for every call).
#[derive(Debug, Clone)]
pub struct VideoHasher {
    http: reqwest::Client,
}

impl VideoHasher {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            http: reqwest::Client::builder()
                .connect_timeout(HASH_TIMEOUT)
                .build()?,
        })
    }

    /// The OpenSubtitles hash of the video at `target`.
    pub async fn hash(&self, target: &HashTarget) -> Result<VideoHash, VideoHashError> {
        tokio::time::timeout(HASH_TIMEOUT, opensubtitles_hash(&self.http, target))
            .await
            .map_err(|_| VideoHashError::Timeout)?
    }
}

/// The OpenSubtitles hash of the video at `target`.
async fn opensubtitles_hash(
    http: &reqwest::Client,
    target: &HashTarget,
) -> Result<VideoHash, VideoHashError> {
    let mut url = Url::parse(&target.url).map_err(|_| VideoHashError::UnsupportedScheme)?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(VideoHashError::UnsupportedScheme);
    }
    // credentials in the URL (the engine's loopback stream) go as basic auth
    let user = (!url.username().is_empty()).then(|| {
        let user = percent_decode(url.username());
        let password = url.password().map(percent_decode);
        (user, password)
    });
    let _ = url.set_username("");
    let _ = url.set_password(None);

    let (head, size) = fetch_range(http, &url, target, user.as_ref(), 0, HASH_CHUNK - 1).await?;
    let size = size.ok_or(VideoHashError::NoRanges)?;
    if size < 2 * HASH_CHUNK {
        return Err(VideoHashError::TooSmall);
    }
    let (tail, _) = fetch_range(
        http,
        &url,
        target,
        user.as_ref(),
        size - HASH_CHUNK,
        size - 1,
    )
    .await?;
    Ok(VideoHash {
        hash: format!("{:016x}", hash_of(size, &head, &tail)),
        size,
    })
}

/// The hash from the size and the two 64 KiB ends.
pub fn hash_of(size: u64, head: &[u8], tail: &[u8]) -> u64 {
    let words = |bytes: &[u8]| -> u64 {
        let (words, _) = bytes.as_chunks::<8>();
        words.iter().fold(0u64, |sum, word| {
            sum.wrapping_add(u64::from_le_bytes(*word))
        })
    };
    size.wrapping_add(words(head)).wrapping_add(words(tail))
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// GET bytes `start..=end`; the body must be exactly that range, and the total size
/// comes from `Content-Range` (`None` when the server sent the whole file instead).
async fn fetch_range(
    http: &reqwest::Client,
    url: &Url,
    target: &HashTarget,
    user: Option<&(String, Option<String>)>,
    start: u64,
    end: u64,
) -> Result<(Vec<u8>, Option<u64>), VideoHashError> {
    let mut request = http
        .get(url.clone())
        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"));
    for (name, value) in &target.headers {
        request = request.header(name.as_str(), value.as_str());
    }
    if let Some(agent) = &target.user_agent {
        request = request.header(reqwest::header::USER_AGENT, agent.as_str());
    }
    if let Some(referrer) = &target.referrer {
        request = request.header(reqwest::header::REFERER, referrer.as_str());
    }
    if let Some((user, password)) = user {
        request = request.basic_auth(user, password.as_deref());
    }
    let mut response = request.send().await?;
    let status = response.status();
    if status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(if status.is_success() {
            VideoHashError::NoRanges
        } else {
            VideoHashError::Status(status.as_u16())
        });
    }
    let total = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let want = (end - start + 1) as usize;
    let mut body = Vec::with_capacity(want);
    while let Some(chunk) = response.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() > want {
            return Err(VideoHashError::NoRanges);
        }
    }
    if body.len() != want {
        return Err(VideoHashError::NoRanges);
    }
    Ok((body, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference values from the OpenSubtitles wiki's test files are for files we
    /// cannot ship; this checks the definition on a synthetic file instead.
    #[test]
    fn the_hash_is_the_size_plus_the_words_of_both_ends() {
        let size = 3 * HASH_CHUNK;
        let file: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let head = &file[..HASH_CHUNK as usize];
        let tail = &file[(size - HASH_CHUNK) as usize..];
        let mut expected = size;
        for bytes in head.chunks(8).chain(tail.chunks(8)) {
            expected = expected.wrapping_add(u64::from_le_bytes(bytes.try_into().unwrap()));
        }
        assert_eq!(hash_of(size, head, tail), expected);
        // wrapping, not overflowing
        let ones = vec![0xffu8; HASH_CHUNK as usize];
        assert_eq!(
            hash_of(u64::MAX, &ones, &ones),
            u64::MAX.wrapping_sub(2 * 8192)
        );
    }

    /// A loopback server that serves `file` with range support (or without), and logs
    /// the Range and Authorization headers it got.
    async fn serve(
        file: Vec<u8>,
        ranges: bool,
    ) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
        let seen = log.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let file = file.clone();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => head.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&head).to_lowercase();
                    for line in head.lines() {
                        if line.starts_with("range:") || line.starts_with("authorization:") {
                            seen.lock().unwrap().push(line.trim().to_string());
                        }
                    }
                    let range = head
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|r| r.trim().split_once('-'))
                        .map(|(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                    let (status, body, extra) = match range {
                        Some((a, b)) if ranges => (
                            "206 Partial Content",
                            file[a..=b].to_vec(),
                            format!("content-range: bytes {a}-{b}/{}\r\n", file.len()),
                        ),
                        _ => ("200 OK", file.clone(), String::new()),
                    };
                    let response = format!(
                        "HTTP/1.1 {status}\r\n{extra}content-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });
        (base, log)
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    #[tokio::test]
    async fn two_range_requests_hash_a_served_file() {
        let size = 200_000usize;
        let file: Vec<u8> = (0..size).map(|i| (i * 7 % 256) as u8).collect();
        let expected = hash_of(
            size as u64,
            &file[..HASH_CHUNK as usize],
            &file[size - HASH_CHUNK as usize..],
        );
        let (base, log) = serve(file, true).await;
        let target = HashTarget {
            url: format!(
                "http://zeiler:s%3Acret@{}/video.mkv",
                base.trim_start_matches("http://")
            ),
            ..HashTarget::default()
        };
        let got = opensubtitles_hash(&client(), &target).await.unwrap();
        assert_eq!(
            got,
            VideoHash {
                hash: format!("{expected:016x}"),
                size: size as u64
            }
        );
        let log = log.lock().unwrap().clone();
        assert!(log.contains(&"range: bytes=0-65535".to_string()), "{log:?}");
        assert!(
            log.contains(&format!("range: bytes={}-{}", size - 65536, size - 1)),
            "{log:?}"
        );
        // zeiler:s:cret as basic auth
        assert!(
            log.contains(&"authorization: basic emvpbgvyonm6y3jlda==".to_string()),
            "{log:?}"
        );
    }

    #[tokio::test]
    async fn a_server_without_ranges_or_a_tiny_file_is_given_up_on() {
        let (base, _) = serve(vec![0u8; 300_000], false).await;
        let target = HashTarget {
            url: format!("{base}/v.mkv"),
            ..HashTarget::default()
        };
        assert!(matches!(
            opensubtitles_hash(&client(), &target).await,
            Err(VideoHashError::NoRanges)
        ));
        let (base, _) = serve(vec![0u8; 100_000], true).await;
        let target = HashTarget {
            url: format!("{base}/v.mkv"),
            ..HashTarget::default()
        };
        assert!(matches!(
            opensubtitles_hash(&client(), &target).await,
            Err(VideoHashError::TooSmall)
        ));
        let target = HashTarget {
            url: "file:///etc/passwd".into(),
            ..HashTarget::default()
        };
        assert!(matches!(
            opensubtitles_hash(&client(), &target).await,
            Err(VideoHashError::UnsupportedScheme)
        ));
    }
}
