//! Xtream Codes servers (`player_api.php`). URL shapes follow Harbor's
//! `src/lib/iptv/xtream.ts` (github.com/harborstremio/harbor), Copyright (c) 2026 Harbor,
//! MIT License — see THIRD-PARTY-NOTICES.md.
//!
//! The username and password travel in every URL here, so these URLs are secrets: never
//! log them, never hand them to the webview except as the stream to play.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::{Channel, IptvError, stable_id};

/// A server and its account.
#[derive(Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XtreamAccount {
    /// `http(s)://host[:port]`, without a path.
    pub server: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for XtreamAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XtreamAccount")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

impl XtreamAccount {
    /// Checks and normalizes the server: http(s), a host, nothing after it (a pasted
    /// `…/player_api.php` or `…/get.php?…` is cut back to the server).
    pub fn new(server: &str, username: &str, password: &str) -> Result<Self, IptvError> {
        let url = Url::parse(server.trim())
            .map_err(|_| IptvError::Invalid("The server is not a valid address.".into()))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(IptvError::Invalid(
                "The server must be an http(s) address.".into(),
            ));
        }
        let (username, password) = (username.trim(), password.trim());
        if username.is_empty() || password.is_empty() {
            return Err(IptvError::Invalid(
                "Enter the username and the password.".into(),
            ));
        }
        let mut server = format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default());
        if let Some(port) = url.port() {
            server.push_str(&format!(":{port}"));
        }
        Ok(Self {
            server,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    /// The account in a pasted `get.php` / `player_api.php` link, if it is one.
    pub fn from_link(link: &str) -> Option<Self> {
        let url = Url::parse(link.trim()).ok()?;
        let path = url.path().to_ascii_lowercase();
        if !(path.ends_with("/get.php") || path.ends_with("/player_api.php")) {
            return None;
        }
        let query: HashMap<_, _> = url.query_pairs().collect();
        let base = format!("{}://{}", url.scheme(), url.host_str()?);
        let base = match url.port() {
            Some(port) => format!("{base}:{port}"),
            None => base,
        };
        Self::new(&base, query.get("username")?, query.get("password")?).ok()
    }

    pub fn host(&self) -> String {
        Url::parse(&self.server)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .unwrap_or_default()
    }

    pub fn api_url(&self, action: &str) -> Url {
        let mut url =
            Url::parse(&format!("{}/player_api.php", self.server)).expect("checked server");
        url.query_pairs_mut()
            .append_pair("username", &self.username)
            .append_pair("password", &self.password)
            .append_pair("action", action);
        url
    }

    pub fn epg_url(&self) -> Url {
        let mut url = Url::parse(&format!("{}/xmltv.php", self.server)).expect("checked server");
        url.query_pairs_mut()
            .append_pair("username", &self.username)
            .append_pair("password", &self.password);
        url
    }

    pub fn stream_url(&self, stream_id: &str) -> String {
        let segment = |s: &str| {
            percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
        };
        format!(
            "{}/live/{}/{}/{}.m3u8",
            self.server,
            segment(&self.username),
            segment(&self.password),
            segment(stream_id)
        )
    }
}

/// Channels from `get_live_categories` and `get_live_streams` answers. Lenient: ids may
/// be numbers or strings, and rows without an id are skipped.
pub fn channels(account: &XtreamAccount, categories: &Value, streams: &Value) -> Vec<Channel> {
    let text = |v: Option<&Value>| -> Option<String> {
        match v? {
            Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    let names: HashMap<String, String> = categories
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| Some((text(c.get("category_id"))?, text(c.get("category_name"))?)))
        .collect();
    streams
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let stream_id = text(s.get("stream_id"))?;
            if !stream_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return None;
            }
            let name = text(s.get("name")).unwrap_or_else(|| format!("Stream {stream_id}"));
            Some(Channel {
                id: stable_id(&["xtream", &stream_id]),
                tvg_id: text(s.get("epg_channel_id")),
                name,
                logo: text(s.get("stream_icon"))
                    .filter(|u| u.starts_with("http://") || u.starts_with("https://")),
                group: text(s.get("category_id")).and_then(|id| names.get(&id).cloned()),
                url: account.stream_url(&stream_id),
                user_agent: None,
                referrer: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::hash::BuildHasher;

    use serde_json::json;

    use super::*;

    /// A password made at run time: tests carry no hard-coded secrets.
    fn password() -> String {
        let random = std::collections::hash_map::RandomState::new();
        format!("{:016x}", random.hash_one(0u8))
    }

    #[test]
    fn servers_are_normalized_and_links_recognized() {
        let pass = password();
        let a =
            XtreamAccount::new(" http://tv.example:8080/player_api.php?x=1 ", "me", &pass).unwrap();
        assert_eq!(a.server, "http://tv.example:8080");
        assert_eq!(a.host(), "tv.example");
        assert!(XtreamAccount::new("ftp://tv.example", "a", &pass).is_err());
        assert!(XtreamAccount::new("http://tv.example", "", &pass).is_err());
        // an empty password, made at run time too
        let none: String = pass.chars().filter(|c| c.is_whitespace()).collect();
        assert!(XtreamAccount::new("http://tv.example", "a", &none).is_err());

        let link = format!("https://tv.example/get.php?username=me&password={pass}&type=m3u_plus");
        let b = XtreamAccount::from_link(&link).unwrap();
        assert_eq!(
            (b.server.as_str(), b.username.as_str(), b.password.as_str()),
            ("https://tv.example", "me", pass.as_str())
        );
        assert!(XtreamAccount::from_link("https://tv.example/list.m3u").is_none());
        assert!(!format!("{b:?}").contains(&pass), "Debug hides the account");
    }

    #[test]
    fn urls_carry_the_account_escaped() {
        // the same escaping serves username and password
        let pass = password();
        let a = XtreamAccount::new("http://tv.example", "m&e/x", &pass).unwrap();
        assert_eq!(
            a.api_url("get_live_streams").as_str(),
            format!(
                "http://tv.example/player_api.php?username=m%26e%2Fx&password={pass}&action=get_live_streams"
            )
        );
        assert_eq!(
            a.epg_url().as_str(),
            format!("http://tv.example/xmltv.php?username=m%26e%2Fx&password={pass}")
        );
        assert_eq!(
            a.stream_url("42"),
            format!("http://tv.example/live/m%26e%2Fx/{pass}/42.m3u8")
        );
    }

    #[test]
    fn channels_from_api_answers() {
        let pass = password();
        let a = XtreamAccount::new("http://tv.example", "me", &pass).unwrap();
        let categories = json!([{"category_id": "1", "category_name": "News"}, {"category_id": 2, "category_name": "Sports"}]);
        let streams = json!([
            {"stream_id": 42, "name": "News One", "stream_icon": "https://i.example/1.png", "epg_channel_id": "news.uk", "category_id": "1"},
            {"stream_id": "7", "name": "Sport", "stream_icon": "", "category_id": 2},
            {"name": "no id"},
            {"stream_id": "../x", "name": "bad id"}
        ]);
        let list = channels(&a, &categories, &streams);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].group.as_deref(), Some("News"));
        assert_eq!(list[0].tvg_id.as_deref(), Some("news.uk"));
        assert_eq!(
            list[0].url,
            format!("http://tv.example/live/me/{pass}/42.m3u8")
        );
        assert_eq!(list[1].group.as_deref(), Some("Sports"));
        assert_eq!(list[1].logo, None);
        assert!(!list[0].id.contains(&pass));
        assert!(channels(&a, &json!({"user_info": {}}), &json!(null)).is_empty());
    }
}
