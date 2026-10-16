//! M3U / M3U8 playlists (`#EXTM3U` + `#EXTINF`).
//!
//! The attribute parsing and the decorative-row filter are adapted from Harbor's
//! `src/lib/iptv/m3u.ts` and `divider-filter.ts` (github.com/harborstremio/harbor),
//! Copyright (c) 2026 Harbor, MIT License — see THIRD-PARTY-NOTICES.md. Zeiler also
//! reads the playlist's EPG URL and the `#EXTVLCOPT` user agent and referrer, which mpv
//! needs to open some streams.

use std::collections::HashMap;

use crate::{Channel, Playlist, stable_id};

/// Parse a playlist. Lenient: anything that is not a channel is skipped.
pub fn parse(text: &str) -> Playlist {
    let mut playlist = Playlist::default();
    let mut pending: Option<Pending> = None;
    let mut seen: HashMap<String, usize> = HashMap::new();

    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTM3U") {
            let attrs = parse_attrs(rest);
            let epg = ["url-tvg", "x-tvg-url", "tvg-url"]
                .iter()
                .find_map(|k| attrs.get(*k))
                // several EPGs may be listed, comma-separated: the first one
                .and_then(|v| v.split(',').map(str::trim).find(|u| !u.is_empty()))
                .filter(|u| is_http(u))
                .map(str::to_owned);
            // merged playlists repeat the header: a later one without a guide keeps it
            if epg.is_some() {
                playlist.epg_url = epg;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            pending = Some(parse_extinf(rest));
            continue;
        }
        if let Some(group) = line.strip_prefix("#EXTGRP:") {
            if let Some(p) = pending.as_mut() {
                p.attrs
                    .entry("group-title".into())
                    .or_insert_with(|| group.trim().to_string());
            }
            continue;
        }
        if let Some(option) = line.strip_prefix("#EXTVLCOPT:") {
            if let Some(p) = pending.as_mut()
                && let Some((key, value)) = option.split_once('=')
            {
                let value = value.trim().to_string();
                match key.trim().to_ascii_lowercase().as_str() {
                    "http-user-agent" => p.user_agent = Some(value),
                    "http-referrer" | "http-referer" => p.referrer = Some(value),
                    _ => {}
                }
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        // a URL line: the channel the pending #EXTINF describes
        let p = pending.take().unwrap_or_default();
        if !is_http(line) {
            continue;
        }
        let name = p
            .attrs
            .get("tvg-name")
            .filter(|n| !n.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| p.title.clone());
        let mut name = name.trim().to_string();
        if name.is_empty() {
            // a bare list of URLs: the file name
            name = line
                .split(['?', '#'])
                .next()
                .and_then(|path| path.trim_end_matches('/').rsplit('/').next())
                .unwrap_or_default()
                .to_string();
        }
        if is_decorative(&name) {
            continue;
        }
        // not from the URL: some carry a token that changes on every refresh, and the
        // favourites keyed by these ids would be lost
        let attr = |k: &str| p.attrs.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
        let base = stable_id(&[
            attr("tvg-id").unwrap_or_default(),
            &name,
            attr("group-title").unwrap_or_default(),
        ]);
        let n = seen.entry(base.clone()).or_insert(0);
        *n += 1;
        let id = if *n == 1 { base } else { format!("{base}-{n}") };
        playlist.channels.push(Channel {
            id,
            tvg_id: attr("tvg-id").map(str::to_owned),
            name,
            logo: attr("tvg-logo")
                .or(attr("logo"))
                .filter(|u| is_http(u))
                .map(str::to_owned),
            group: attr("group-title").map(str::to_owned),
            url: line.to_string(),
            user_agent: p
                .user_agent
                .or_else(|| attr("user-agent").map(str::to_owned)),
            referrer: p.referrer,
        });
    }
    playlist
}

fn is_http(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

#[derive(Default)]
struct Pending {
    title: String,
    attrs: HashMap<String, String>,
    user_agent: Option<String>,
    referrer: Option<String>,
}

/// `-1 tvg-id="x" group-title="News, World",Channel name`
fn parse_extinf(rest: &str) -> Pending {
    // an unbalanced quote would hide every comma: then the last one, as players do
    let comma = if rest.matches('"').count().is_multiple_of(2) {
        first_unquoted_comma(rest)
    } else {
        rest.rfind(',')
    };
    let (attrs, title) = match comma {
        Some(i) => (&rest[..i], rest[i + 1..].trim()),
        None => (rest, ""),
    };
    Pending {
        title: title.to_string(),
        attrs: parse_attrs(attrs),
        ..Pending::default()
    }
}

/// `key="value"` and `key=value` pairs, keys lowercased.
fn parse_attrs(text: &str) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    let mut chars = text.char_indices().peekable();
    while let Some(&(start, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        // the key runs to '=' or whitespace
        let mut end = start;
        while let Some(&(i, c)) = chars.peek() {
            if c == '=' || c.is_whitespace() {
                break;
            }
            end = i + c.len_utf8();
            chars.next();
        }
        let key = text[start..end].to_ascii_lowercase();
        if chars.peek().map(|&(_, c)| c) != Some('=') {
            continue; // a bare token (the duration)
        }
        chars.next();
        let value = if chars.peek().map(|&(_, c)| c) == Some('"') {
            chars.next();
            let mut value = String::new();
            for (_, c) in chars.by_ref() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
            value
        } else {
            let mut value = String::new();
            while let Some(&(_, c)) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                value.push(c);
                chars.next();
            }
            value
        };
        if !key.is_empty() {
            attrs.insert(key, value);
        }
    }
    attrs
}

/// The comma that ends the attributes: the first one outside quotes (the title after it
/// may have commas of its own, "CNN, International").
fn first_unquoted_comma(s: &str) -> Option<usize> {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

/// Separator rows some playlists use as headings (`##### NEWS #####`, `───────`).
pub fn is_decorative(name: &str) -> bool {
    let name = name.trim();
    // "M6", "E!", "1+1" are channels; "★★★" and "-----" are not
    if !name.chars().any(char::is_alphanumeric) {
        return true;
    }
    let symbols = name
        .chars()
        .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
        .count();
    let char_count = name.chars().count();
    let fenced = |s: &str| {
        let lead = s
            .chars()
            .take_while(|c| "#=-~*_+<>▶◀►◄─━═.".contains(*c))
            .count();
        let trail = s
            .chars()
            .rev()
            .take_while(|c| "#=-~*_+<>▶◀►◄─━═.".contains(*c))
            .count();
        lead >= 3 && trail >= 3
    };
    fenced(name) || (char_count >= 5 && symbols * 100 > char_count * 55)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}#EXTM3U url-tvg=\"https://epg.example/guide.xml.gz, https://other.example/e.xml\"
#EXTINF:-1 tvg-id=\"news.uk\" tvg-name=\"News One\" tvg-logo=\"https://img.example/n1.png\" group-title=\"News, World\",News One HD
#EXTVLCOPT:http-user-agent=Mozilla/5.0 Test
#EXTVLCOPT:http-referrer=https://site.example/
https://cdn.example/live/n1.m3u8
#EXTINF:-1,##### SPORTS #####
https://cdn.example/divider.ts
#EXTINF:-1 tvg-logo=\"javascript:alert(1)\",Sport Two
#EXTGRP:Sports
http://cdn.example/s2.ts
#EXTINF:-1,Local File
file:///etc/passwd
#EXTINF:-1,Sport Two
http://cdn.example/s2.ts
";

    #[test]
    fn channels_attributes_options_and_epg() {
        let playlist = parse(SAMPLE);
        assert_eq!(
            playlist.epg_url.as_deref(),
            Some("https://epg.example/guide.xml.gz")
        );
        let names: Vec<_> = playlist.channels.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["News One", "Sport Two", "Sport Two"]);

        let news = &playlist.channels[0];
        assert_eq!(news.tvg_id.as_deref(), Some("news.uk"));
        assert_eq!(news.group.as_deref(), Some("News, World"));
        assert_eq!(news.logo.as_deref(), Some("https://img.example/n1.png"));
        assert_eq!(news.user_agent.as_deref(), Some("Mozilla/5.0 Test"));
        assert_eq!(news.referrer.as_deref(), Some("https://site.example/"));

        let sport = &playlist.channels[1];
        assert_eq!(sport.group.as_deref(), Some("Sports"), "#EXTGRP");
        assert_eq!(sport.logo, None, "only http(s) logos");
        assert_eq!(sport.user_agent, None, "options belong to one entry");
        // the same URL and name twice: still two ids, both stable
        assert_ne!(playlist.channels[1].id, playlist.channels[2].id);
        assert_eq!(parse(SAMPLE).channels[2].id, playlist.channels[2].id);
        assert!(
            !playlist.channels[0].id.contains("cdn.example"),
            "ids don't carry the URL"
        );

        // a comma in the title; a token that changes the URL keeps the id
        let a = parse(
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"x\" group-title=\"News\",CNN, International\nhttps://cdn.example/cnn.m3u8?token=1\n",
        );
        let b = parse(
            "#EXTM3U\n#EXTINF:-1 tvg-id=\"x\" group-title=\"News\",CNN, International\nhttps://cdn.example/cnn.m3u8?token=2\n",
        );
        assert_eq!(a.channels[0].name, "CNN, International");
        assert_eq!(a.channels[0].group.as_deref(), Some("News"));
        assert_eq!(a.channels[0].id, b.channels[0].id);

        // a second header without a guide keeps the first one's
        let merged = parse(
            "#EXTM3U url-tvg=\"https://e.example/g.xml\"\n#EXTINF:-1,A\nhttps://a.example/1\n#EXTM3U\n#EXTINF:-1,B\nhttps://a.example/2\n",
        );
        assert_eq!(merged.epg_url.as_deref(), Some("https://e.example/g.xml"));

        // an unbalanced quote: the title after the last comma still counts
        let odd = parse(
            "#EXTINF:-1 tvg-logo=\"http://x/a.png group-title=\"News\",CNN\nhttps://a.example/cnn\n",
        );
        assert_eq!(odd.channels[0].name, "CNN");
    }

    #[test]
    fn decorative_rows_are_dropped() {
        for row in [
            "##### SPORTS #####",
            "───────────",
            "=== NEWS ===",
            "★★★",
            "",
            "--",
        ] {
            assert!(is_decorative(row), "{row:?}");
        }
        for row in [
            "BBC One",
            "CNN",
            "Das Erste HD",
            "beIN Sports 1 (720p)",
            "TV5Monde",
            "M6",
            "E!",
            "1+1",
        ] {
            assert!(!is_decorative(row), "{row:?}");
        }
    }

    #[test]
    fn a_bare_url_list_still_parses() {
        let playlist = parse("https://a.example/1.m3u8\nhttps://a.example/2.m3u8\n");
        let names: Vec<_> = playlist.channels.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["1.m3u8", "2.m3u8"], "no #EXTINF: the file name");
        let playlist = parse("#EXTM3U\n#EXTINF:5,Radio\nhttps://a.example/r.mp3\n");
        assert_eq!(playlist.channels[0].name, "Radio");
        assert_eq!(playlist.epg_url, None);
    }
}
