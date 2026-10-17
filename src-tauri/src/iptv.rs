//! The user's IPTV sources (M3U playlists, Xtream Codes accounts) for the Live room.
//!
//! What a source is lives in `iptv.json` under the profile dir: its id, name, kind, host,
//! user agent and favourite channels. What gets you in — the playlist and guide URLs
//! (which often carry a login) and the Xtream account — lives in the OS keychain
//! ([`Secrets`]), never in a file and never sent to the webview. The webview gets
//! channel ids, names, logos and groups; a stream URL only when it plays one, through the
//! one-shot player handoff (never the location hash).
//!
//! Channel lists are kept in memory for 6 hours and guides for 1 hour (Harbor's
//! `src/lib/iptv/store.ts` cache times; MIT, see THIRD-PARTY-NOTICES.md).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use url::Url;
use zeiler_iptv::{Access, Channel, IptvClient, IptvError, Program, XtreamAccount, xtream};

use crate::error::IpcError;
use crate::persist::JsonStore;
use crate::secrets::{SecretStorage, Secrets};

const CHANNELS_TTL: Duration = Duration::from_secs(6 * 3600);
const FILE_NAME: &str = "iptv.json";
const GUIDE_TTL: Duration = Duration::from_secs(3600);
const GUIDE_FAILURE_TTL: Duration = Duration::from_secs(5 * 60);
/// The guide window kept per source: a little of the past, a day ahead.
const GUIDE_BEHIND_MS: i64 = 2 * 3600 * 1000;
const GUIDE_AHEAD_MS: i64 = 24 * 3600 * 1000;
pub const MAX_SOURCES: usize = 50;
const MAX_FAVORITES: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    M3u,
    Xtream,
}

/// A source as stored in `iptv.json` (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IptvSource {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    /// Shown to tell sources apart; the rest of the address stays in the keychain.
    pub host: String,
    pub has_epg: bool,
    #[serde(default)]
    pub user_agent: Option<String>,
    /// The user accepted that this source's addresses use plain http, which sends the
    /// account unencrypted; without it only https is fetched.
    #[serde(default)]
    pub allow_http: bool,
    #[serde(default)]
    pub favorites: Vec<String>,
}

impl IptvSource {
    pub fn access(&self) -> Access<'_> {
        Access {
            user_agent: self.user_agent.as_deref(),
            allow_http: self.allow_http,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IptvFile {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub sources: Vec<IptvSource>,
}

fn version() -> u32 {
    1
}

impl Default for IptvFile {
    fn default() -> Self {
        Self {
            version: version(),
            sources: Vec::new(),
        }
    }
}

/// What the keychain holds for a source.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSecret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epg_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xtream: Option<XtreamAccount>,
}

impl SourceSecret {
    /// The guide: the one the user gave, else the playlist's own, else Xtream's.
    fn guide_url(&self, playlist_epg: Option<&str>) -> Option<Url> {
        self.epg_url
            .as_deref()
            .or(playlist_epg)
            .and_then(|u| Url::parse(u).ok())
            .or_else(|| self.xtream.as_ref().map(XtreamAccount::epg_url))
    }
}

struct Loaded {
    at: Instant,
    channels: Arc<Vec<Channel>>,
    playlist_epg: Option<String>,
    has_epg: bool,
}

struct Guide {
    at: Instant,
    /// A failure is remembered too (for less long), so a guide that can't be read isn't
    /// downloaded again on every change of the channels on screen.
    result: Result<Arc<HashMap<String, Vec<Program>>>, String>,
}

impl Guide {
    fn fresh(&self) -> bool {
        let ttl = if self.result.is_ok() {
            GUIDE_TTL
        } else {
            GUIDE_FAILURE_TTL
        };
        self.at.elapsed() < ttl
    }
}

/// A source's channels and whether it has a guide.
pub struct Channels {
    pub list: Arc<Vec<Channel>>,
    pub has_epg: bool,
}

pub struct IptvState {
    pub client: IptvClient,
    pub store: JsonStore<IptvFile>,
    pub secrets: Arc<Secrets>,
    channels: Mutex<HashMap<String, Loaded>>,
    guides: Mutex<HashMap<String, Guide>>,
    /// One download at a time per source and kind (channels, guide): later callers wait
    /// for it and take its result from the cache.
    fetching: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// Where each source's secret was found or put this run (None: not found). Sources
    /// not looked up yet are assumed to be in the keychain, so listing them doesn't read
    /// (and on some systems prompt for) every secret.
    storage: Mutex<HashMap<String, Option<SecretStorage>>>,
}

pub fn secret_key(id: &str) -> String {
    format!("iptv:{id}")
}

fn iptv_error(e: IptvError) -> IpcError {
    match e {
        IptvError::Invalid(m) => IpcError::InvalidInput(m),
        IptvError::Source(m) => IpcError::Addon(m),
        IptvError::Network(m) => IpcError::Network(m),
    }
}

impl IptvState {
    pub async fn load(profile_dir: &Path, secrets: Arc<Secrets>) -> anyhow::Result<Self> {
        Ok(Self {
            client: IptvClient::new().map_err(|e| anyhow::anyhow!("{e}"))?,
            store: JsonStore::load(profile_dir.join(FILE_NAME)).await?,
            secrets,
            channels: Mutex::new(HashMap::new()),
            guides: Mutex::new(HashMap::new()),
            fetching: Mutex::new(HashMap::new()),
            storage: Mutex::new(HashMap::new()),
        })
    }

    pub async fn source(&self, id: &str) -> Result<IptvSource, IpcError> {
        self.store
            .read(|f| f.sources.iter().find(|s| s.id == id).cloned())
            .await
            .ok_or_else(|| IpcError::NotFound("That playlist is not in Zeiler any more.".into()))
    }

    async fn exists(&self, id: &str) -> bool {
        self.store
            .read(|f| f.sources.iter().any(|s| s.id == id))
            .await
    }

    pub async fn secret(&self, id: &str) -> Result<SourceSecret, IpcError> {
        let found = self.secrets.locate(&secret_key(id)).await;
        self.storage
            .lock()
            .await
            .insert(id.to_string(), found.as_ref().map(|(_, storage)| *storage));
        let (raw, _) = found.ok_or_else(|| {
            IpcError::Locked(
                "Zeiler can't find this playlist's address in the system keychain (it may have been kept for one session only). Remove it and add it again.".into(),
            )
        })?;
        serde_json::from_str(&raw).map_err(|_| {
            IpcError::Storage("This playlist's saved address is damaged; add it again.".into())
        })
    }

    /// Keep a new source's secret; says where it went.
    pub async fn put_secret(&self, id: &str, secret: &SourceSecret) -> SecretStorage {
        let raw = serde_json::to_string(secret).expect("a secret serializes");
        let storage = self.secrets.set(&secret_key(id), &raw).await;
        self.storage
            .lock()
            .await
            .insert(id.to_string(), Some(storage));
        storage
    }

    pub async fn storage_of(&self, id: &str) -> Option<SecretStorage> {
        self.storage
            .lock()
            .await
            .get(id)
            .copied()
            .unwrap_or(Some(SecretStorage::Os))
    }

    async fn fetch_lock(&self, key: String) -> Arc<Mutex<()>> {
        self.fetching.lock().await.entry(key).or_default().clone()
    }

    /// Fetch a source's channels (and its playlist's guide URL).
    pub async fn fetch(
        &self,
        kind: SourceKind,
        secret: &SourceSecret,
        access: Access<'_>,
    ) -> Result<(Vec<Channel>, Option<String>), IpcError> {
        match kind {
            SourceKind::M3u => {
                let url = secret
                    .url
                    .as_deref()
                    .and_then(|u| Url::parse(u).ok())
                    .ok_or_else(|| {
                        IpcError::InvalidInput("The playlist address is not valid.".into())
                    })?;
                let playlist = self
                    .client
                    .playlist(&url, access)
                    .await
                    .map_err(iptv_error)?;
                Ok((playlist.channels, playlist.epg_url))
            }
            SourceKind::Xtream => {
                let account = secret.xtream.clone().ok_or_else(|| {
                    IpcError::InvalidInput("The Xtream account is missing.".into())
                })?;
                let (categories_url, streams_url) = (
                    account.api_url("get_live_categories"),
                    account.api_url("get_live_streams"),
                );
                let (categories, streams) = tokio::try_join!(
                    self.client.json(&categories_url, access),
                    self.client.json(&streams_url, access),
                )
                .map_err(iptv_error)?;
                let channels = tokio::task::spawn_blocking(move || {
                    xtream::channels(&account, &categories, &streams)
                })
                .await
                .map_err(|_| IpcError::Addon("Reading the channel list failed.".into()))?;
                Ok((channels, None))
            }
        }
    }

    /// A source's channels, from memory while fresh.
    pub async fn channels(&self, id: &str, force: bool) -> Result<Channels, IpcError> {
        let cached = |loaded: &Loaded| Channels {
            list: loaded.channels.clone(),
            has_epg: loaded.has_epg,
        };
        if !force
            && let Some(loaded) = self.channels.lock().await.get(id)
            && loaded.at.elapsed() < CHANNELS_TTL
        {
            return Ok(cached(loaded));
        }
        let lock = self.fetch_lock(format!("channels:{id}")).await;
        let started = Instant::now();
        let _one_at_a_time = lock.lock().await;
        // someone else loaded it while this call waited
        if let Some(loaded) = self.channels.lock().await.get(id)
            && (loaded.at >= started || (!force && loaded.at.elapsed() < CHANNELS_TTL))
        {
            return Ok(cached(loaded));
        }
        let source = self.source(id).await?;
        let secret = self.secret(id).await?;
        let (channels, playlist_epg) = self.fetch(source.kind, &secret, source.access()).await?;
        let has_epg = secret.guide_url(playlist_epg.as_deref()).is_some();
        let list = Arc::new(channels);
        // the guide kept only the old list's channels (or a failure): read it again
        self.guides.lock().await.remove(id);
        self.remember(id, list.clone(), playlist_epg, has_epg).await;
        Ok(Channels { list, has_epg })
    }

    /// Cache a source's channels, unless it was removed meanwhile.
    pub async fn remember(
        &self,
        id: &str,
        channels: Arc<Vec<Channel>>,
        playlist_epg: Option<String>,
        has_epg: bool,
    ) {
        // checked under the cache lock, which `forget` takes after the source is gone
        let mut cache = self.channels.lock().await;
        if !self.exists(id).await {
            return;
        }
        cache.insert(
            id.to_string(),
            Loaded {
                at: Instant::now(),
                channels,
                playlist_epg,
                has_epg,
            },
        );
    }

    /// Point at another profile's `iptv.json` (a profile switch). The caches belong to
    /// the profile left, so they go.
    pub async fn switch_to(&self, profile_dir: &Path) -> anyhow::Result<()> {
        self.store.switch_to(profile_dir.join(FILE_NAME)).await?;
        self.channels.lock().await.clear();
        self.guides.lock().await.clear();
        self.storage.lock().await.clear();
        Ok(())
    }

    /// Remove the keychain entries of a deleted profile's sources (they would outlive
    /// it). Best effort: an unreadable file leaves them.
    pub async fn delete_secrets_of(&self, profile_dir: &Path) {
        let file: IptvFile = match crate::persist::read_json(&profile_dir.join(FILE_NAME)).await {
            Ok(file) => file,
            Err(e) => {
                tracing::warn!("could not read a deleted profile's IPTV sources: {e:#}");
                return;
            }
        };
        for source in file.sources {
            self.secrets.delete(&secret_key(&source.id)).await;
        }
    }

    pub async fn forget(&self, id: &str) {
        self.channels.lock().await.remove(id);
        self.guides.lock().await.remove(id);
        self.storage.lock().await.remove(id);
    }

    /// The source's guide around `now_ms`, for its own channels, from memory while
    /// fresh. None when the source has no guide.
    pub async fn guide(
        &self,
        id: &str,
        now_ms: i64,
    ) -> Result<Option<Arc<HashMap<String, Vec<Program>>>>, IpcError> {
        let cached = |guide: &Guide| guide.result.clone().map(Some).map_err(IpcError::Addon);
        if let Some(guide) = self.guides.lock().await.get(id)
            && guide.fresh()
        {
            return cached(guide);
        }
        let lock = self.fetch_lock(format!("guide:{id}")).await;
        let _one_at_a_time = lock.lock().await;
        if let Some(guide) = self.guides.lock().await.get(id)
            && guide.fresh()
        {
            return cached(guide);
        }
        let channels = match self.channels(id, false).await {
            Ok(channels) => channels,
            Err(e) => {
                // remembered like a failed guide, so the next request doesn't retry the
                // playlist right away
                self.remember_guide(id, Err(e.to_string())).await;
                return Err(e);
            }
        };
        let source = self.source(id).await?;
        let secret = self.secret(id).await?;
        let playlist_epg = self
            .channels
            .lock()
            .await
            .get(id)
            .and_then(|l| l.playlist_epg.clone());
        let Some(url) = secret.guide_url(playlist_epg.as_deref()) else {
            return Ok(None);
        };
        let wanted: HashSet<String> = channels
            .list
            .iter()
            .filter_map(|c| c.tvg_id.clone())
            .collect();
        let result = self
            .client
            .guide(
                &url,
                source.access(),
                now_ms - GUIDE_BEHIND_MS,
                now_ms + GUIDE_AHEAD_MS,
                wanted,
            )
            .await
            .map(Arc::new)
            .map_err(|e| e.to_string());
        self.remember_guide(id, result.clone()).await;
        result.map(Some).map_err(IpcError::Addon)
    }

    async fn remember_guide(
        &self,
        id: &str,
        result: Result<Arc<HashMap<String, Vec<Program>>>, String>,
    ) {
        let mut cache = self.guides.lock().await;
        if self.exists(id).await {
            cache.insert(
                id.to_string(),
                Guide {
                    at: Instant::now(),
                    result,
                },
            );
        }
    }

    pub fn max_favorites() -> usize {
        MAX_FAVORITES
    }
}
