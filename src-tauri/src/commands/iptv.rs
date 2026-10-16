//! IPTV commands for the Live room's Playlists tab. See `iptv.rs` for what is stored
//! where; nothing here returns a stream URL or an account except `iptv_resolve`, which
//! the UI hands straight to the player through the one-shot launch.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use url::Url;
use zeiler_iptv::{Program, XtreamAccount};
use zeiler_player::PlaybackRequest;

use crate::error::IpcError;
use crate::iptv::{IptvSource, IptvState, MAX_SOURCES, SourceKind, SourceSecret, secret_key};
use crate::secrets::SecretStorage;

/// A source as the UI sees it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub host: String,
    pub has_epg: bool,
    pub favorites: Vec<String>,
    /// Where its address is kept; null when it can't be found (removed from the
    /// keychain, or kept in memory by an earlier run).
    pub storage: Option<SecretStorage>,
}

async fn view(state: &IptvState, source: IptvSource) -> SourceView {
    let storage = state.storage_of(&source.id).await;
    SourceView {
        storage,
        id: source.id,
        name: source.name,
        kind: source.kind,
        host: source.host,
        has_epg: source.has_epg,
        favorites: source.favorites,
    }
}

#[tauri::command]
pub async fn iptv_sources(state: State<'_, IptvState>) -> Result<Vec<SourceView>, IpcError> {
    let sources = state.store.read(|f| f.sources.clone()).await;
    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        out.push(view(&state, source).await);
    }
    Ok(out)
}

// no Debug: these hold the account
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XtreamInput {
    pub server: String,
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInput {
    pub name: String,
    pub kind: SourceKind,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub epg_url: Option<String>,
    #[serde(default)]
    pub xtream: Option<XtreamInput>,
    #[serde(default)]
    pub user_agent: Option<String>,
}

fn http_url(text: &str, what: &str) -> Result<Url, IpcError> {
    Url::parse(text.trim())
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
        .ok_or_else(|| IpcError::InvalidInput(format!("The {what} must be an http(s) address.")))
}

/// The checked secret, the host to show and the user agent.
fn parse_input(input: &SourceInput) -> Result<(SourceSecret, String, Option<String>), IpcError> {
    let epg_url = match input
        .epg_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        Some(u) => Some(http_url(u, "guide (EPG) address")?.to_string()),
        None => None,
    };
    let user_agent = input
        .user_agent
        .as_deref()
        .map(str::trim)
        .filter(|ua| !ua.is_empty())
        .map(|ua| {
            if ua.len() > 256 || ua.chars().any(char::is_control) {
                Err(IpcError::InvalidInput(
                    "The user agent is too long or has control characters.".into(),
                ))
            } else {
                Ok(ua.to_string())
            }
        })
        .transpose()?;
    match input.kind {
        SourceKind::M3u => {
            let url = http_url(input.url.as_deref().unwrap_or_default(), "playlist address")?;
            let host = url.host_str().unwrap_or_default().to_string();
            Ok((
                SourceSecret {
                    url: Some(url.to_string()),
                    epg_url,
                    xtream: None,
                },
                host,
                user_agent,
            ))
        }
        SourceKind::Xtream => {
            let x = input.xtream.as_ref().ok_or_else(|| {
                IpcError::InvalidInput("Enter the server, the username and the password.".into())
            })?;
            let account = XtreamAccount::new(&x.server, &x.username, &x.password)
                .map_err(|e| IpcError::InvalidInput(e.to_string()))?;
            let host = account.host();
            Ok((
                SourceSecret {
                    url: None,
                    epg_url,
                    xtream: Some(account),
                },
                host,
                user_agent,
            ))
        }
    }
}

/// Add a source: checked by loading its channels once, then its address goes to the
/// keychain and the rest to `iptv.json`.
#[tauri::command]
pub async fn iptv_source_add(
    state: State<'_, IptvState>,
    source: SourceInput,
) -> Result<SourceView, IpcError> {
    let name = source.name.trim();
    if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
        return Err(IpcError::InvalidInput(
            "Give the playlist a name of up to 60 characters.".into(),
        ));
    }
    if state.store.read(|f| f.sources.len()).await >= MAX_SOURCES {
        return Err(IpcError::InvalidInput(format!(
            "Zeiler keeps at most {MAX_SOURCES} playlists."
        )));
    }
    let (secret, host, user_agent) = parse_input(&source)?;
    let (channels, playlist_epg) = state
        .fetch(source.kind, &secret, user_agent.as_deref())
        .await?;
    if channels.is_empty() {
        return Err(IpcError::Addon(
            "That source has no channels Zeiler can play.".into(),
        ));
    }

    let id = uuid::Uuid::new_v4().to_string();
    let has_epg = secret.epg_url.is_some() || playlist_epg.is_some() || secret.xtream.is_some();
    state.put_secret(&id, &secret).await;
    let entry = IptvSource {
        id: id.clone(),
        name: name.to_string(),
        kind: source.kind,
        host,
        has_epg,
        user_agent,
        favorites: Vec::new(),
    };
    let stored = entry.clone();
    if let Err(e) = state.store.update(move |f| f.sources.push(stored)).await {
        state.secrets.delete(&secret_key(&id)).await;
        return Err(e);
    }
    state
        .remember(&id, Arc::new(channels), playlist_epg, has_epg)
        .await;
    Ok(view(&state, entry).await)
}

#[tauri::command]
pub async fn iptv_source_remove(state: State<'_, IptvState>, id: String) -> Result<(), IpcError> {
    state.source(&id).await?;
    let gone = id.clone();
    state
        .store
        .update(move |f| f.sources.retain(|s| s.id != gone))
        .await?;
    state.forget(&id).await;
    // the source is gone even if the keychain refuses: say so
    state.secrets.delete(&secret_key(&id)).await;
    Ok(())
}

/// A channel as the UI sees it: no URL.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    pub id: String,
    pub name: String,
    pub logo: Option<String>,
    pub group: Option<String>,
    pub tvg_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelList {
    pub channels: Vec<ChannelView>,
    /// Group names in first-seen order.
    pub groups: Vec<String>,
    /// Whether the source has a guide (now: a playlist may have gained one).
    pub has_epg: bool,
}

#[tauri::command]
pub async fn iptv_channels(
    state: State<'_, IptvState>,
    id: String,
    force: Option<bool>,
) -> Result<ChannelList, IpcError> {
    let loaded = state.channels(&id, force.unwrap_or(false)).await?;
    let channels = loaded.list;
    let mut seen = BTreeSet::new();
    let mut groups = Vec::new();
    for group in channels.iter().filter_map(|c| c.group.as_ref()) {
        if seen.insert(group.clone()) {
            groups.push(group.clone());
        }
    }
    Ok(ChannelList {
        channels: channels
            .iter()
            .map(|c| ChannelView {
                id: c.id.clone(),
                name: c.name.clone(),
                logo: c.logo.clone(),
                group: c.group.clone(),
                tvg_id: c.tvg_id.clone(),
            })
            .collect(),
        groups,
        has_epg: loaded.has_epg,
    })
}

/// What is on, per guide id, overlapping `fromMs..toMs` (within the day ahead the guide
/// keeps), for the given guide ids only.
#[tauri::command]
pub async fn iptv_epg(
    state: State<'_, IptvState>,
    id: String,
    tvg_ids: Vec<String>,
    from_ms: i64,
    to_ms: i64,
) -> Result<HashMap<String, Vec<Program>>, IpcError> {
    if tvg_ids.len() > 5000 || to_ms <= from_ms {
        return Err(IpcError::InvalidInput(
            "That guide request is too large.".into(),
        ));
    }
    let now = chrono_now_ms();
    let Some(guide) = state.guide(&id, now).await? else {
        return Ok(HashMap::new());
    };
    Ok(tvg_ids
        .into_iter()
        .filter_map(|tvg| {
            let programs: Vec<Program> = guide
                .get(&tvg)?
                .iter()
                .filter(|p| p.start_ms < to_ms && p.end_ms > from_ms)
                .take(8)
                .cloned()
                .collect();
            (!programs.is_empty()).then_some((tvg, programs))
        })
        .collect())
}

fn chrono_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[tauri::command]
pub async fn iptv_favorite_set(
    state: State<'_, IptvState>,
    id: String,
    channel_id: String,
    on: bool,
) -> Result<Vec<String>, IpcError> {
    if channel_id.is_empty() || channel_id.len() > 64 {
        return Err(IpcError::InvalidInput("That is not a channel.".into()));
    }
    state.source(&id).await?;
    let max = IptvState::max_favorites();
    let file = state
        .store
        .try_update(|f| {
            let source = f.sources.iter_mut().find(|s| s.id == id).ok_or_else(|| {
                IpcError::NotFound("That playlist is not in Zeiler any more.".into())
            })?;
            let has = source.favorites.contains(&channel_id);
            if has == on {
                return Ok(None);
            }
            if on {
                if source.favorites.len() >= max {
                    return Err(IpcError::InvalidInput(format!(
                        "At most {max} favourites per playlist."
                    )));
                }
                source.favorites.push(channel_id.clone());
            } else {
                source.favorites.retain(|c| c != &channel_id);
            }
            Ok(Some(source.favorites.clone()))
        })
        .await?;
    match file {
        Some(favorites) => Ok(favorites),
        None => Ok(state.source(&id).await?.favorites),
    }
}

/// What the player needs to play a channel: its stream, user agent and referrer. Only
/// http(s). The UI hands it to the Player screen through the one-shot launch (marked
/// live), so the URL never reaches the location hash or history.
#[tauri::command]
pub async fn iptv_resolve(
    state: State<'_, IptvState>,
    source_id: String,
    channel_id: String,
) -> Result<PlaybackRequest, IpcError> {
    let source = state.source(&source_id).await?;
    let channels = state.channels(&source_id, false).await?;
    let channel = channels
        .list
        .iter()
        .find(|c| c.id == channel_id)
        .ok_or_else(|| IpcError::NotFound("That channel is no longer in the playlist.".into()))?;
    http_url(&channel.url, "channel's stream")?;
    Ok(PlaybackRequest {
        url: channel.url.clone(),
        title: Some(channel.name.clone()),
        headers: Vec::new(),
        user_agent: channel.user_agent.clone().or(source.user_agent),
        referrer: channel.referrer.clone(),
        start_at_sec: None,
        options: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: SourceKind) -> SourceInput {
        SourceInput {
            name: "Mine".into(),
            kind,
            url: None,
            epg_url: None,
            xtream: None,
            user_agent: None,
        }
    }

    #[test]
    fn m3u_input_needs_an_http_address_and_keeps_it_secret() {
        let mut m3u = input(SourceKind::M3u);
        m3u.url = Some(" https://tv.example/get.php?username=me&password=pw ".into());
        m3u.epg_url = Some("https://tv.example/epg.xml.gz".into());
        m3u.user_agent = Some("VLC/3.0".into());
        let (secret, host, ua) = parse_input(&m3u).unwrap();
        assert_eq!(host, "tv.example", "only the host is shown");
        assert_eq!(ua.as_deref(), Some("VLC/3.0"));
        assert!(secret.url.unwrap().contains("password=pw"));

        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "",
            "tv.example/list.m3u",
        ] {
            m3u.url = Some(bad.into());
            assert!(parse_input(&m3u).is_err(), "{bad}");
        }
        m3u.url = Some("http://tv.example/a.m3u".into());
        m3u.epg_url = Some("ftp://x".into());
        assert!(parse_input(&m3u).is_err());
        m3u.epg_url = None;
        m3u.user_agent = Some("a\nb".into());
        assert!(parse_input(&m3u).is_err());
    }

    #[test]
    fn xtream_input_is_normalized() {
        let mut x = input(SourceKind::Xtream);
        assert!(parse_input(&x).is_err());
        x.xtream = Some(XtreamInput {
            server: "http://tv.example:8080/".into(),
            username: "me".into(),
            password: "pw".into(),
        });
        let (secret, host, _) = parse_input(&x).unwrap();
        assert_eq!(host, "tv.example");
        let account = secret.xtream.unwrap();
        assert_eq!(account.server, "http://tv.example:8080");
        assert!(secret.url.is_none());
    }

    #[test]
    fn a_secret_round_trips_and_its_debug_hides_the_account() {
        let secret = SourceSecret {
            url: None,
            epg_url: None,
            xtream: Some(XtreamAccount::new("http://tv.example", "me", "pw").unwrap()),
        };
        let raw = serde_json::to_string(&secret).unwrap();
        let back: SourceSecret = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.xtream, secret.xtream);
        assert!(!format!("{:?}", secret.xtream).contains("pw"));
    }
}
