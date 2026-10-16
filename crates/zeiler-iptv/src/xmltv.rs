//! XMLTV programme guides, read as a stream: an EPG can be hundreds of megabytes, and
//! only a window of it (around now) is kept. The time format and the now/next idea follow
//! Harbor's `src/lib/iptv/xmltv.ts` (github.com/harborstremio/harbor), Copyright (c) 2026
//! Harbor, MIT License — see THIRD-PARTY-NOTICES.md; the parser itself is quick-xml's.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read};

use encoding_rs::{Decoder, Encoding, UTF_8};
use quick_xml::XmlVersion;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use serde::Serialize;

/// Most programmes kept from one guide, whatever the window…
pub const MAX_PROGRAMMES: usize = 200_000;
/// …and per channel (a day is rarely more than 50).
pub const MAX_PER_CHANNEL: usize = 96;
const MAX_TITLE: usize = 300;
const MAX_DESC: usize = 2000;
const MAX_CATEGORY: usize = 100;
/// A programme without a `stop` is kept when it starts this long before the window, in
/// case it is still on; its end is the next programme's start.
const OPEN_ENDED_LOOKBACK_MS: i64 = 12 * 3600 * 1000;
/// The end of an open-ended programme that has no next one.
const OPEN_ENDED_DEFAULT_MS: i64 = 3600 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    pub start_ms: i64,
    pub end_ms: i64,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// Programmes per channel id (`tvg-id`), sorted by start, keeping those that overlap
/// `from_ms..to_ms` — only for the channels in `wanted` when it is given (a guide often
/// covers far more channels than the source has). A malformed programme is skipped; a
/// broken document ends the read with what was read so far. The input is decoded from
/// its BOM or declared encoding (Latin-1 guides exist), invalid bytes replaced.
pub fn parse_window<R: Read>(
    input: R,
    from_ms: i64,
    to_ms: i64,
    wanted: Option<&HashSet<String>>,
) -> HashMap<String, Vec<Program>> {
    let mut reader =
        Reader::from_reader(BufReader::with_capacity(64 * 1024, Utf8Reader::new(input)));
    let mut buf = Vec::new();
    let mut out: HashMap<String, Vec<Program>> = HashMap::new();
    let mut kept = 0usize;
    // channel, start, stop (None: open-ended)
    let mut current: Option<(String, i64, Option<i64>)> = None;
    let mut field: Option<Field> = None;
    let mut fields = Fields::default();

    // a read error ends the read with what was read so far
    while let Ok(event) = reader.read_event_into(&mut buf) {
        match event {
            Event::Start(e) => match e.local_name().as_ref() {
                "programme" => {
                    current = programme_head(&e).filter(|(channel, start, stop)| {
                        let overlaps = match stop {
                            Some(stop) => *start < to_ms && *stop > from_ms,
                            None => *start < to_ms && *start > from_ms - OPEN_ENDED_LOOKBACK_MS,
                        };
                        overlaps
                            && wanted.is_none_or(|w| w.contains(channel))
                            && out.get(channel).is_none_or(|p| p.len() < MAX_PER_CHANNEL)
                    });
                    fields = Fields::default();
                }
                "title" if current.is_some() => field = fields.open(Field::Title),
                "desc" if current.is_some() => field = fields.open(Field::Desc),
                "category" if current.is_some() => field = fields.open(Field::Category),
                _ => field = None,
            },
            Event::Text(t) => {
                if let Some(f) = field {
                    fields.push(f, &t.xml10_content());
                }
            }
            Event::CData(t) => {
                if let Some(f) = field {
                    fields.push(f, &t.xml10_content());
                }
            }
            Event::GeneralRef(r) => {
                if let Some(f) = field {
                    let text = match r.resolve_char_ref() {
                        Ok(Some(c)) => c.to_string(),
                        _ => resolve_predefined_entity(&r)
                            .unwrap_or_default()
                            .to_string(),
                    };
                    fields.push(f, &text);
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                "programme" => {
                    if let Some((channel, start_ms, stop)) = current.take()
                        && let Some(title) = fields.title.take().filter(|t| !t.trim().is_empty())
                    {
                        out.entry(channel).or_default().push(Program {
                            start_ms,
                            // filled in below
                            end_ms: stop.unwrap_or(i64::MIN),
                            title: title.trim().to_string(),
                            desc: fields
                                .desc
                                .take()
                                .map(|d| d.trim().to_string())
                                .filter(|d| !d.is_empty()),
                            category: fields.category.take().map(|c| c.trim().to_string()),
                        });
                        kept += 1;
                        if kept >= MAX_PROGRAMMES {
                            break;
                        }
                    }
                }
                _ => {
                    if let Some(f) = field.take() {
                        fields.closed[f as usize] = true;
                    }
                }
            },
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    for programmes in out.values_mut() {
        programmes.sort_by_key(|p| p.start_ms);
        // an open-ended programme lasts until the next one starts
        for i in 0..programmes.len() {
            if programmes[i].end_ms == i64::MIN {
                let next = programmes[i + 1..]
                    .iter()
                    .map(|p| p.start_ms)
                    .find(|s| *s > programmes[i].start_ms);
                programmes[i].end_ms =
                    next.unwrap_or(programmes[i].start_ms + OPEN_ENDED_DEFAULT_MS);
            }
        }
        programmes.retain(|p| p.start_ms < to_ms && p.end_ms > from_ms);
    }
    out.retain(|_, p| !p.is_empty());
    out
}

/// UTF-8 out of whatever the guide is in: the encoding comes from a BOM, else the XML
/// declaration, else UTF-8; invalid bytes become U+FFFD instead of ending the read.
struct Utf8Reader<R> {
    inner: R,
    decoder: Option<Decoder>,
    out: Vec<u8>,
    pos: usize,
    done: bool,
}

impl<R: Read> Utf8Reader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            decoder: None,
            out: Vec::new(),
            pos: 0,
            done: false,
        }
    }
}

/// The encoding an XML declaration at the start of `head` names.
fn declared_encoding(head: &[u8]) -> Option<&'static Encoding> {
    let head = &head[..head.len().min(512)];
    let text = String::from_utf8_lossy(head);
    let decl = text.split("?>").next()?;
    let after = &decl[decl.find("encoding")? + "encoding".len()..];
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let label = after[1..].split(quote).next()?;
    Encoding::for_label(label.trim().as_bytes())
}

impl<R: Read> Read for Utf8Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.pos < self.out.len() {
                let n = buf.len().min(self.out.len() - self.pos);
                buf[..n].copy_from_slice(&self.out[self.pos..self.pos + n]);
                self.pos += n;
                return Ok(n);
            }
            if self.done {
                return Ok(0);
            }
            let mut chunk = vec![0u8; 64 * 1024];
            let n = self.inner.read(&mut chunk)?;
            let last = n == 0;
            let decoder = self.decoder.get_or_insert_with(|| {
                // a BOM still wins: new_decoder sniffs it. UTF-16 can't be declared in
                // an ASCII-compatible declaration, so a file that says so without a BOM
                // is UTF-8 (output_encoding)
                declared_encoding(&chunk[..n])
                    .map(Encoding::output_encoding)
                    .unwrap_or(UTF_8)
                    .new_decoder()
            });
            let mut text =
                String::with_capacity(decoder.max_utf8_buffer_length(n).unwrap_or(n * 3 + 16));
            let _ = decoder.decode_to_string(&chunk[..n], &mut text, last);
            self.out = text.into_bytes();
            self.pos = 0;
            self.done = last;
        }
    }
}

#[derive(Clone, Copy)]
enum Field {
    Title,
    Desc,
    Category,
}

#[derive(Default)]
struct Fields {
    title: Option<String>,
    desc: Option<String>,
    category: Option<String>,
    /// Fields whose first element has ended.
    closed: [bool; 3],
}

impl Fields {
    /// The field to collect text into, unless its first element was already read.
    fn open(&self, field: Field) -> Option<Field> {
        (!self.closed[field as usize]).then_some(field)
    }

    /// Text arrives in pieces (around entities). Only the first element of each kind
    /// counts: a second `<title lang="…">` is a translation.
    fn push(&mut self, field: Field, text: &str) {
        let (slot, max) = match field {
            Field::Title => (&mut self.title, MAX_TITLE),
            Field::Desc => (&mut self.desc, MAX_DESC),
            Field::Category => (&mut self.category, MAX_CATEGORY),
        };
        let slot = slot.get_or_insert_with(String::new);
        let room = max.saturating_sub(slot.chars().count());
        slot.extend(text.chars().take(room));
    }
}

fn programme_head(e: &BytesStart) -> Option<(String, i64, Option<i64>)> {
    let mut start = None;
    let mut stop = None;
    let mut channel = None;
    for attr in e.attributes().flatten() {
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .ok()?
            .into_owned();
        match attr.key.local_name().as_ref() {
            "start" => start = parse_time(&value),
            "stop" => stop = parse_time(&value),
            "channel" => channel = Some(value),
            _ => {}
        }
    }
    let start = start?;
    // `stop` is optional in XMLTV, but one that isn't after the start is malformed
    if stop.is_some_and(|stop| stop <= start) {
        return None;
    }
    Some((channel.filter(|c| !c.is_empty())?, start, stop))
}

/// `20261016203000 +0200` (the offset is optional and means UTC when missing) to Unix
/// milliseconds.
pub fn parse_time(text: &str) -> Option<i64> {
    let text = text.trim();
    let (stamp, offset) = match text.split_once(' ') {
        Some((stamp, offset)) => (stamp, Some(offset.trim())),
        None => (text, None),
    };
    if stamp.len() < 12 || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let num = |range: std::ops::Range<usize>| stamp.get(range).and_then(|s| s.parse::<i64>().ok());
    let (year, month, day) = (num(0..4)?, num(4..6)?, num(6..8)?);
    let (hour, minute) = (num(8..10)?, num(10..12)?);
    let second = if stamp.len() >= 14 { num(12..14)? } else { 0 };
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let offset_minutes = match offset {
        None | Some("") => 0,
        Some(o) => {
            let sign = match o.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits = &o[1..];
            if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            sign * (digits[..2].parse::<i64>().ok()? * 60 + digits[2..].parse::<i64>().ok()?)
        }
    };
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    Some(seconds * 1000)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_with_and_without_offsets() {
        assert_eq!(parse_time("19700101000000 +0000"), Some(0));
        assert_eq!(parse_time("19700101000000"), Some(0));
        assert_eq!(parse_time("20261016203000 +0200"), Some(1_792_175_400_000));
        assert_eq!(parse_time("20261016183000 +0000"), Some(1_792_175_400_000));
        assert_eq!(parse_time("20261016133000 -0500"), Some(1_792_175_400_000));
        assert_eq!(parse_time("202610161830"), Some(1_792_175_400_000));
        for bad in [
            "",
            "2026",
            "20261316183000",
            "20261016183000 0200",
            "x0261016183000",
        ] {
            assert_eq!(parse_time(bad), None, "{bad}");
        }
    }

    const GUIDE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tv>
  <channel id="news.uk"><display-name>News</display-name></channel>
  <programme start="20261016180000 +0000" stop="20261016190000 +0000" channel="news.uk">
    <title lang="en">Evening &amp; Night News</title>
    <title lang="fr">Infos</title>
    <desc>Headlines &#8212; then weather.</desc>
    <category>News</category>
  </programme>
  <programme start="20261016170000 +0000" stop="20261016180000 +0000" channel="news.uk">
    <title><![CDATA[Before <Evening>]]></title>
  </programme>
  <programme start="20261010170000 +0000" stop="20261010180000 +0000" channel="news.uk">
    <title>Last week</title>
  </programme>
  <programme start="20261016180000 +0000" stop="20261016170000 +0000" channel="news.uk">
    <title>Ends before it starts</title>
  </programme>
  <programme start="20261016180000 +0000" stop="20261016190000 +0000" channel="sport.uk">
    <title></title>
  </programme>
</tv>"#;

    #[test]
    fn programmes_in_the_window_sorted_with_entities_resolved() {
        let from = parse_time("20261016160000").unwrap();
        let to = parse_time("20261017160000").unwrap();
        let guide = parse_window(GUIDE.as_bytes(), from, to, None);
        assert_eq!(guide.len(), 1, "an untitled programme is skipped");
        let news = &guide["news.uk"];
        let titles: Vec<_> = news.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Before <Evening>", "Evening & Night News"]);
        assert_eq!(news[1].desc.as_deref(), Some("Headlines — then weather."));
        assert_eq!(news[1].category.as_deref(), Some("News"));
        assert_eq!(news[1].end_ms - news[1].start_ms, 3_600_000);
    }

    #[test]
    fn a_broken_document_keeps_what_was_read() {
        let cut = &GUIDE[..GUIDE.find("<programme start=\"20261010").unwrap() + 20];
        let from = parse_time("20261016160000").unwrap();
        let guide = parse_window(cut.as_bytes(), from, from + 86_400_000, None);
        assert_eq!(guide["news.uk"].len(), 2);
    }

    #[test]
    fn only_wanted_channels_and_open_ended_programmes() {
        let guide = r#"<tv>
  <programme start="20261016170000 +0000" channel="a"><title>First</title></programme>
  <programme start="20261016180000 +0000" channel="a"><title>Second</title></programme>
  <programme start="20261016170000 +0000" stop="20261016180000 +0000" channel="b"><title>Other</title></programme>
</tv>"#;
        let from = parse_time("20261016173000").unwrap();
        let to = from + 3_600_000;
        let wanted: HashSet<String> = ["a".to_string()].into();
        let parsed = parse_window(guide.as_bytes(), from, to, Some(&wanted));
        assert_eq!(parsed.len(), 1, "channel b is not the source's");
        let a = &parsed["a"];
        assert_eq!(a[0].title, "First");
        assert_eq!(
            a[0].end_ms,
            parse_time("20261016180000").unwrap(),
            "until the next one"
        );
        assert_eq!(a[1].end_ms - a[1].start_ms, OPEN_ENDED_DEFAULT_MS);
    }

    #[test]
    fn latin1_guides_and_bad_bytes_are_read_through() {
        let mut latin1 = br#"<?xml version="1.0" encoding="ISO-8859-1"?><tv><programme start="20261016180000" stop="20261016190000" channel="a"><title>Caf"#.to_vec();
        latin1.push(0xe9); // é in Latin-1, invalid as UTF-8
        latin1.extend_from_slice(br#"</title></programme><programme start="20261016190000" stop="20261016200000" channel="a"><title>After</title></programme></tv>"#);
        let from = parse_time("20261016170000").unwrap();
        let parsed = parse_window(&latin1[..], from, from + 86_400_000, None);
        let titles: Vec<_> = parsed["a"].iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Café", "After"]);

        let mut broken = br#"<tv><programme start="20261016180000" stop="20261016190000" channel="a"><title>Bad "#.to_vec();
        broken.push(0xff);
        broken.extend_from_slice(br#"</title></programme><programme start="20261016190000" stop="20261016200000" channel="a"><title>After</title></programme></tv>"#);
        let parsed = parse_window(&broken[..], from, from + 86_400_000, None);
        assert_eq!(parsed["a"].len(), 2, "one bad byte doesn't end the guide");
        assert!(parsed["a"][0].title.contains('\u{fffd}'));
    }

    #[test]
    fn long_fields_are_cut_and_channels_capped() {
        let mut guide = String::from("<tv>");
        for i in 0..(MAX_PER_CHANNEL + 10) {
            let start = 1_000_000 + i as i64;
            guide.push_str(&format!(
                r#"<programme start="2026101618{:02}00" stop="2026101618{:02}30" channel="a"><title>{}</title></programme>"#,
                i % 60,
                i % 60,
                "x".repeat(400 + start as usize % 2)
            ));
        }
        guide.push_str("</tv>");
        let from = parse_time("20261016170000").unwrap();
        let parsed = parse_window(guide.as_bytes(), from, from + 86_400_000, None);
        assert!(parsed["a"].len() <= MAX_PER_CHANNEL);
        assert!(
            parsed["a"]
                .iter()
                .all(|p| p.title.chars().count() <= MAX_TITLE)
        );
    }
}
