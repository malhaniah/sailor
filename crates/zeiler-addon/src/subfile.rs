//! Subtitle files: download one and turn it into UTF-8 text mpv can load.
//!
//! What subtitle addons serve is messy: `.gz` files, legacy single-byte encodings
//! (windows-1256 Arabic, windows-1251 Cyrillic, …), UTF-16 with a byte-order mark, and
//! now and then an archive or an HTML error page with a 200 status. libmpv builds do not
//! all detect encodings themselves, so every file is normalized here before it reaches
//! the player:
//! - gzip (by its magic bytes) is unpacked, with the same size cap as the download;
//! - ZIP, RAR and 7z archives and web pages are refused with a readable reason;
//! - only text that starts as a subtitle is accepted (see `format_of`: a saved file is
//!   local to mpv, which would follow the paths an mpv EDL or a CUE sheet names);
//! - a byte-order mark decides the encoding, else a caller's override, else valid UTF-8
//!   stays UTF-8, else `chardetng` guesses (the override is the escape hatch for a
//!   wrong guess on a short file);
//! - the format comes from how the text starts: `WEBVTT`, an `[Script Info]` header
//!   (ASS/SSA), a MicroDVD frame line, or a SubRip cue (number and timing).

use encoding_rs::{Encoding, UTF_8};
use serde::Serialize;
use url::Url;

use crate::client::AddonClient;
use crate::error::AddonError;

/// Cap on a subtitle download, and on what a gzip file may unpack to. Real subtitle
/// files are a few hundred KiB at most.
pub const MAX_SUBTITLE_BYTES: usize = 10 * 1024 * 1024;

/// A subtitle file's format, as its file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    /// ASS and SSA (SSA v4 is read by the same parser).
    Ass,
    /// MicroDVD (`{start}{end}text`).
    Sub,
}

impl SubtitleFormat {
    /// The file extension mpv's demuxer probe expects for this format.
    pub fn extension(self) -> &'static str {
        match self {
            SubtitleFormat::Srt => "srt",
            SubtitleFormat::Vtt => "vtt",
            SubtitleFormat::Ass => "ass",
            SubtitleFormat::Sub => "sub",
        }
    }
}

/// A subtitle file, normalized to UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleFile {
    pub text: String,
    pub format: SubtitleFormat,
    /// The encoding the file was read as (an encoding_rs name: `UTF-8`,
    /// `windows-1256`, …).
    pub encoding: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum SubtitleError {
    #[error("Only http and https subtitle links can be loaded.")]
    UnsupportedScheme,
    #[error("The subtitle could not be downloaded: {0}")]
    Fetch(#[from] AddonError),
    #[error("The subtitle file is larger than {} MiB.", MAX_SUBTITLE_BYTES / (1024 * 1024))]
    TooLarge,
    #[error("The subtitle is packed in a {0} archive, which Zeiler cannot open.")]
    Archive(&'static str),
    #[error("The subtitle file is damaged (it could not be unpacked).")]
    BadGzip,
    #[error("The subtitle file is empty.")]
    Empty,
    #[error("This is not a subtitle file ({0}).")]
    NotSubtitles(&'static str),
}

/// Download `url` (http/https only, at most [`MAX_SUBTITLE_BYTES`]) and normalize it
/// with [`decode_subtitle`].
pub async fn fetch_subtitle(
    client: &AddonClient,
    url: &Url,
    encoding_override: Option<&'static Encoding>,
) -> Result<SubtitleFile, SubtitleError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(SubtitleError::UnsupportedScheme);
    }
    let bytes = match client.get_bytes(url.clone(), MAX_SUBTITLE_BYTES).await {
        Err(AddonError::TooLarge { .. }) => return Err(SubtitleError::TooLarge),
        other => other?,
    };
    decode_subtitle(&bytes, encoding_override)
}

/// Normalize the bytes of a subtitle file (see the module docs). `encoding_override`
/// replaces detection, but never a byte-order mark: a BOM is unambiguous.
pub fn decode_subtitle(
    bytes: &[u8],
    encoding_override: Option<&'static Encoding>,
) -> Result<SubtitleFile, SubtitleError> {
    let unpacked;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        unpacked = gunzip(bytes)?;
        unpacked.as_slice()
    } else {
        bytes
    };
    if let Some(kind) = archive_kind(bytes) {
        return Err(SubtitleError::Archive(kind));
    }

    let (encoding, body) = match Encoding::for_bom(bytes) {
        Some((encoding, bom)) => (encoding, &bytes[bom..]),
        None => (encoding_override.unwrap_or_else(|| detect(bytes)), bytes),
    };
    let (text, _had_errors) = encoding.decode_without_bom_handling(body);
    let text = text.trim_end_matches('\0');
    if text.trim().is_empty() {
        return Err(SubtitleError::Empty);
    }
    if text.contains('\0') {
        return Err(SubtitleError::NotSubtitles("it holds binary data"));
    }
    // written as it starts: a signature only counts at the very beginning
    let start = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let head: String = start
        .chars()
        .take(64)
        .collect::<String>()
        .to_ascii_lowercase();
    if head.starts_with("<!doctype html") || head.starts_with("<html") {
        return Err(SubtitleError::NotSubtitles("the link returned a web page"));
    }
    let format = format_of(start).ok_or(SubtitleError::NotSubtitles(
        "it does not start like an SRT, WebVTT, ASS or MicroDVD subtitle",
    ))?;
    Ok(SubtitleFile {
        format,
        text: start.to_owned(),
        encoding: encoding.name(),
    })
}

/// The encoding of bytes without a byte-order mark: UTF-8 when they are (nearly all)
/// valid UTF-8, else `chardetng`'s guess among the legacy encodings.
fn detect(bytes: &[u8]) -> &'static Encoding {
    // A UTF-8 file with a stray invalid byte or two (a bad edit, a cut-off download)
    // must not be read as windows-1252: count what decodes. (In a legacy file nearly
    // every non-ASCII byte is an invalid sequence, and valid ones are rare accidents.)
    let mut valid_non_ascii = 0usize;
    let mut invalid = 0usize;
    for chunk in bytes.utf8_chunks() {
        valid_non_ascii += chunk.valid().chars().filter(|c| !c.is_ascii()).count();
        invalid += usize::from(!chunk.invalid().is_empty());
    }
    if invalid == 0 || valid_non_ascii >= 2 * invalid {
        return UTF_8;
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    detector.guess(None, chardetng::Utf8Detection::Deny)
}

fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, SubtitleError> {
    use std::io::Read;
    let mut out = Vec::new();
    // MultiGz: a concatenation of gzip members is still one file
    flate2::read::MultiGzDecoder::new(bytes)
        .take(MAX_SUBTITLE_BYTES as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| SubtitleError::BadGzip)?;
    if out.len() > MAX_SUBTITLE_BYTES {
        return Err(SubtitleError::TooLarge);
    }
    Ok(out)
}

fn archive_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        Some("ZIP")
    } else if bytes.starts_with(b"Rar!\x1a\x07") {
        Some("RAR")
    } else if bytes.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Some("7z")
    } else {
        None
    }
}

/// The format `text` starts as, when it is one Zeiler passes on to mpv; none otherwise.
///
/// Every signature is anchored at the start of the text. mpv identifies a file by its
/// content, not its name, and a file Zeiler saved is a local file to mpv: an mpv EDL, a
/// CUE sheet or a playlist that an addon serves as a "subtitle" would make mpv open the
/// local files (or network shares) it names, which mpv refuses for content it fetched
/// itself. So only text that begins as a subtitle format gets through.
fn format_of(start: &str) -> Option<SubtitleFormat> {
    if let Some(rest) = start.strip_prefix("WEBVTT")
        && (rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n']))
    {
        return Some(SubtitleFormat::Vtt);
    }
    let mut lines = start.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next()?;
    // ASS/SSA: the first line that is not a `;` comment is the [Script Info] header
    let header = std::iter::once(first)
        .chain(lines.clone())
        .find(|l| !l.starts_with(';'));
    if header.is_some_and(|l| l.eq_ignore_ascii_case("[script info]")) {
        return Some(SubtitleFormat::Ass);
    }
    if is_microdvd_line(first) {
        return Some(SubtitleFormat::Sub);
    }
    // SubRip: a cue number and then its timing, or the timing right away
    let numbered = (1..=9).contains(&first.len()) && first.bytes().all(|b| b.is_ascii_digit());
    if is_srt_timing(first) || (numbered && lines.next().is_some_and(is_srt_timing)) {
        return Some(SubtitleFormat::Srt);
    }
    None
}

/// `00:00:01,000 --> 00:00:02,500` (`.` for `,` and positions after it allowed).
fn is_srt_timing(line: &str) -> bool {
    let Some((from, to)) = line.split_once("-->") else {
        return false;
    };
    is_timestamp(from.trim()) && to.split_whitespace().next().is_some_and(is_timestamp)
}

/// `h:mm:ss,mmm` with one to three hour digits and one to three millisecond digits.
fn is_timestamp(stamp: &str) -> bool {
    let digits = |s: &str, len: std::ops::RangeInclusive<usize>| {
        len.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
    };
    let mut parts = stamp.split(':');
    let (Some(hours), Some(minutes), Some(rest), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let Some((seconds, millis)) = rest.split_once([',', '.']) else {
        return false;
    };
    digits(hours, 1..=3)
        && digits(minutes, 2..=2)
        && digits(seconds, 2..=2)
        && digits(millis, 1..=3)
}

/// `{123}{456}text` (the end frame may be empty).
fn is_microdvd_line(line: &str) -> bool {
    let frame = |s: &str| -> Option<usize> {
        let rest = s.strip_prefix('{')?;
        let end = rest.find('}')?;
        rest[..end]
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then_some(end + 2)
    };
    let Some(first) = frame(line) else {
        return false;
    };
    line[..first].len() > 2 && frame(&line[first..]).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\r\n00:00:01,000 --> 00:00:02,500\r\nHello there.\r\n\r\n";

    fn decoded(bytes: &[u8]) -> SubtitleFile {
        decode_subtitle(bytes, None).expect("decodes")
    }

    #[test]
    fn utf8_stays_utf8_and_a_bom_is_dropped() {
        let file = decoded(SRT.as_bytes());
        assert_eq!(file.text, SRT);
        assert_eq!(file.encoding, "UTF-8");
        assert_eq!(file.format, SubtitleFormat::Srt);

        let with_bom = [
            b"\xEF\xBB\xBF".as_slice(),
            "1\n00:00:01,000 --> 00:00:02,000\nÇa va ?\n".as_bytes(),
        ]
        .concat();
        let file = decoded(&with_bom);
        assert!(file.text.starts_with("1\n"), "{:?}", file.text);
        assert!(file.text.contains("Ça va ?"));
    }

    #[test]
    fn utf16_is_read_by_its_bom() {
        let text = "1\n00:00:01,000 --> 00:00:02,000\nПривет\n";
        let mut le = vec![0xFF, 0xFE];
        le.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        let file = decoded(&le);
        assert_eq!(file.text, text);
        assert_eq!(file.encoding, "UTF-16LE");

        let mut be = vec![0xFE, 0xFF];
        be.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decoded(&be).text, text);
    }

    /// A few lines of real-looking dialogue in a legacy encoding, the way subtitle sites
    /// serve them.
    fn legacy(encoding: &'static Encoding, lines: &[&str]) -> Vec<u8> {
        let mut text = String::new();
        for (i, line) in lines.iter().enumerate() {
            text.push_str(&format!(
                "{}\r\n00:00:{:02},000 --> 00:00:{:02},500\r\n{line}\r\n\r\n",
                i + 1,
                i * 3 + 1,
                i * 3 + 2
            ));
        }
        let (bytes, _, unmappable) = encoding.encode(&text);
        assert!(!unmappable, "fixture fits {}", encoding.name());
        bytes.into_owned()
    }

    #[test]
    fn arabic_windows_1256_is_detected() {
        let bytes = legacy(
            encoding_rs::WINDOWS_1256,
            &[
                "مرحبا، كيف حالك اليوم؟",
                "أنا بخير، شكرا لك. وأنت؟",
                "لقد كان يوما طويلا في العمل.",
                "هل تريد أن نذهب إلى السينما الليلة؟",
            ],
        );
        let file = decoded(&bytes);
        assert_eq!(file.encoding, "windows-1256");
        assert!(file.text.contains("كيف حالك"), "{}", file.text);
    }

    #[test]
    fn cyrillic_windows_1251_is_detected() {
        let bytes = legacy(
            encoding_rs::WINDOWS_1251,
            &[
                "Привет, как у тебя дела сегодня?",
                "Спасибо, всё хорошо. А у тебя?",
                "Это был очень длинный день на работе.",
                "Пойдём сегодня вечером в кино?",
            ],
        );
        let file = decoded(&bytes);
        assert_eq!(file.encoding, "windows-1251");
        assert!(file.text.contains("как у тебя дела"));
    }

    #[test]
    fn an_override_replaces_detection_but_not_a_bom() {
        let bytes = legacy(encoding_rs::WINDOWS_1256, &["مرحبا"]);
        let forced = decode_subtitle(&bytes, Some(encoding_rs::WINDOWS_1256)).unwrap();
        assert!(forced.text.contains("مرحبا"));
        assert_eq!(forced.encoding, "windows-1256");
        // a wrong override reads wrongly, as asked
        let wrong = decode_subtitle(&bytes, Some(encoding_rs::WINDOWS_1252)).unwrap();
        assert_eq!(wrong.encoding, "windows-1252");
        assert!(!wrong.text.contains("مرحبا"));
        // a BOM is unambiguous
        let bom = [b"\xEF\xBB\xBF".as_slice(), SRT.as_bytes()].concat();
        let file = decode_subtitle(&bom, Some(encoding_rs::WINDOWS_1256)).unwrap();
        assert_eq!(file.encoding, "UTF-8");
    }

    #[test]
    fn a_stray_invalid_byte_does_not_turn_utf8_into_latin1() {
        let mut bytes = "1\n00:00:01,000 --> 00:00:02,000\nNo sé qué decir, señor. ¿Mañana?\n"
            .as_bytes()
            .to_vec();
        bytes.push(0xFF);
        let file = decoded(&bytes);
        assert_eq!(file.encoding, "UTF-8");
        assert!(file.text.contains("señor"));
    }

    #[test]
    fn gzip_is_unpacked() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(SRT.as_bytes()).unwrap();
        let file = decoded(&gz.finish().unwrap());
        assert_eq!(file.text, SRT);
        assert!(matches!(
            decode_subtitle(&[0x1f, 0x8b, 0, 1, 2, 3], None),
            Err(SubtitleError::BadGzip)
        ));
    }

    #[test]
    fn a_gzip_bomb_is_capped() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        let zeros = vec![b'a'; 1024 * 1024];
        for _ in 0..=MAX_SUBTITLE_BYTES / zeros.len() {
            gz.write_all(&zeros).unwrap();
        }
        assert!(matches!(
            decode_subtitle(&gz.finish().unwrap(), None),
            Err(SubtitleError::TooLarge)
        ));
    }

    #[test]
    fn archives_web_pages_and_binary_data_are_refused() {
        for (bytes, kind) in [
            (b"PK\x03\x04rest".as_slice(), "ZIP"),
            (b"Rar!\x1a\x07\x00rest", "RAR"),
            (b"7z\xbc\xaf\x27\x1crest", "7z"),
        ] {
            match decode_subtitle(bytes, None) {
                Err(SubtitleError::Archive(k)) => assert_eq!(k, kind),
                other => panic!("{kind}: {other:?}"),
            }
        }
        for page in [
            "<!DOCTYPE html><html><body>Not found</body></html>",
            "\n  <html><head><title>Error</title></head></html>",
        ] {
            assert!(matches!(
                decode_subtitle(page.as_bytes(), None),
                Err(SubtitleError::NotSubtitles(_))
            ));
        }
        assert!(matches!(
            decode_subtitle(b"\x00\x01\x02\x03binary\x00junk", None),
            Err(SubtitleError::NotSubtitles(_))
        ));
        assert!(matches!(
            decode_subtitle(b"", None),
            Err(SubtitleError::Empty)
        ));
        assert!(matches!(
            decode_subtitle(b"  \r\n\0\0", None),
            Err(SubtitleError::Empty)
        ));
        // trailing NUL padding alone is not binary
        let padded = [SRT.as_bytes(), b"\0\0\0"].concat();
        assert_eq!(decoded(&padded).text, SRT);
    }

    #[test]
    fn formats_are_sniffed_from_the_text() {
        let vtt = "WEBVTT\n\n00:01.000 --> 00:02.000\nHi\n";
        assert_eq!(decoded(vtt.as_bytes()).format, SubtitleFormat::Vtt);
        let ass = "\u{feff}[Script Info]\nScriptType: v4.00+\n\n[Events]\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hi\n";
        assert_eq!(decoded(ass.as_bytes()).format, SubtitleFormat::Ass);
        let ssa = "; comment\n[SCRIPT INFO]\nScriptType: v4.00\n";
        assert_eq!(decoded(ssa.as_bytes()).format, SubtitleFormat::Ass);
        let microdvd = "{1}{100}Hello|world\n{120}{}Bye\n";
        assert_eq!(decoded(microdvd.as_bytes()).format, SubtitleFormat::Sub);
        assert_eq!(decoded(SRT.as_bytes()).format, SubtitleFormat::Srt);
        let unnumbered = "\n\n00:00:01.000 --> 00:00:02.000 X1:10 X2:20\nHi\n";
        let file = decoded(unnumbered.as_bytes());
        assert_eq!(file.format, SubtitleFormat::Srt);
        assert!(file.text.starts_with("00:00:01"), "written as it starts");
        assert_eq!(SubtitleFormat::Ass.extension(), "ass");
        assert_eq!(serde_json::to_value(SubtitleFormat::Vtt).unwrap(), "vtt");
    }

    /// A saved file is local to mpv, and mpv reads it by its content: an mpv EDL, a CUE
    /// sheet or a playlist would make it open the local files or network shares they
    /// name. Only text that starts as a subtitle gets through.
    #[test]
    fn files_that_only_pretend_to_be_subtitles_are_refused() {
        for text in [
            "# mpv EDL v0\n/etc/passwd,0,10\n",
            "# mpv EDL v0\n[Script Info]\n1\n00:00:01,000 --> 00:00:02,000\nHi\n",
            "\u{feff}  # mpv EDL v0\n%10%C:/Users/me/secret.txt\n",
            "FILE \"C:/Users/me/secret.txt\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n",
            "#EXTM3U\n#EXTINF:-1,x\nfile:///etc/passwd\n",
            "[playlist]\nFile1=//server/share/x.srt\n",
            "edl:///etc/passwd,0,10\n",
            "{not microdvd}\n",
            "Hello, these are just words.\n",
            "1\nnot a timing line\n",
            "WEBVTTX\n",
        ] {
            assert!(
                matches!(
                    decode_subtitle(text.as_bytes(), None),
                    Err(SubtitleError::NotSubtitles(_))
                ),
                "{text:?} must be refused"
            );
        }
    }

    #[tokio::test]
    async fn only_http_links_are_fetched() {
        let client = AddonClient::new().unwrap();
        for url in [
            "file:///etc/passwd",
            "ftp://host/a.srt",
            "data:text/plain,1",
        ] {
            let url = Url::parse(url).unwrap();
            assert!(matches!(
                fetch_subtitle(&client, &url, None).await,
                Err(SubtitleError::UnsupportedScheme)
            ));
        }
    }

    #[tokio::test]
    async fn a_download_is_fetched_decoded_and_capped() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cyrillic = legacy(
            encoding_rs::WINDOWS_1251,
            &[
                "Привет, как у тебя дела сегодня?",
                "Спасибо, всё хорошо. А у тебя?",
                "Это был очень длинный день.",
            ],
        );
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let cyrillic = cyrillic.clone();
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => head.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&head).to_string();
                    let body: Vec<u8> = if head.starts_with("GET /big") {
                        // announced as too large
                        let _ = socket
                            .write_all(format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", MAX_SUBTITLE_BYTES + 1).as_bytes())
                            .await;
                        return;
                    } else {
                        cyrillic
                    };
                    let _ = socket
                        .write_all(format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).as_bytes())
                        .await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });
        let client = AddonClient::for_loopback_tests();
        let url = Url::parse(&format!("http://{addr}/sub.srt")).unwrap();
        let file = fetch_subtitle(&client, &url, None).await.unwrap();
        assert_eq!(file.encoding, "windows-1251");
        assert!(file.text.contains("как у тебя дела"));
        let big = Url::parse(&format!("http://{addr}/big.srt")).unwrap();
        assert!(matches!(
            fetch_subtitle(&client, &big, None).await,
            Err(SubtitleError::TooLarge)
        ));
    }
}
