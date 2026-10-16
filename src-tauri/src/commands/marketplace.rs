//! The opt-in addon marketplace: an index of third-party addon directories
//! (stremio-addons.net and Stremio's community/official addon catalogs), fetched in
//! Rust (the webview's CSP allows no remote fetches), cached for an hour in memory and
//! in the cache dir, and filtered here.
//!
//! Nothing is fetched until `marketplaceEnabled` is on. Listings are display data:
//! installing one goes through `addon_preview` (which fetches the live manifest and
//! compares it with the listing) and the confirmation dialog, like a pasted URL. Errors
//! name the directory, never a URL.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::{Mutex, RwLock};
use url::Url;
use zeiler_addon::directory::{
    self, Category, DirectoryEntry, DirectoryQuery, DirectorySource, DirectoryVisibility,
    NSFW_CATEGORY, STREMIO_ADDONS_NET_API,
};
use zeiler_addon::{AddonClient, AddonError, Descriptor};

use crate::error::IpcError;
use crate::persist::{read_json, write_json};
use crate::state::{AppState, Settings};

/// How long an index is used before it is rebuilt.
const INDEX_TTL: Duration = Duration::from_secs(60 * 60);
/// Where Stremio's addon catalogs come from (Cinemeta serves `addon_catalog`).
const STREMIO_CATALOG_TRANSPORT: &str = "https://v3-cinemeta.strem.io/manifest.json";
/// Bumped when the cached index's shape changes, so an old cache is rebuilt.
const INDEX_VERSION: u32 = 2;
/// While no directory answers, the last index is served and a rebuild is tried again
/// only this often (not on every keystroke).
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// A directory that could not be read for the current index.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFailure {
    pub source: DirectorySource,
    /// A sentence naming the directory and what went wrong (never a URL).
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MarketIndex {
    version: u32,
    entries: Vec<DirectoryEntry>,
    categories: Vec<Category>,
    /// Unix milliseconds.
    fetched_at: u64,
    sources_ok: Vec<DirectorySource>,
    sources_failed: Vec<SourceFailure>,
    /// Served stale because no directory answered; tried again after this (Unix ms).
    retry_at: Option<u64>,
    /// The `marketplaceSources` it was built from.
    with_addons_net: bool,
    with_catalog: bool,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl MarketIndex {
    fn same_sources(&self, settings: &Settings) -> bool {
        self.with_addons_net == settings.marketplace_sources.stremio_addons_net
            && self.with_catalog == settings.marketplace_sources.stremio_catalog
    }

    fn fresh_for(&self, settings: &Settings) -> bool {
        let now = now_ms();
        let young = match self.retry_at {
            Some(retry_at) => now < retry_at,
            None => now.saturating_sub(self.fetched_at) < INDEX_TTL.as_millis() as u64,
        };
        self.version == INDEX_VERSION && young && self.same_sources(settings)
    }
}

/// The index, managed state (not part of `AppState`).
pub struct MarketState {
    index: RwLock<Option<MarketIndex>>,
    /// One rebuild at a time; callers that wait reuse its result.
    rebuild: Mutex<()>,
    cache_file: PathBuf,
    api: Url,
    catalog_transport: Url,
}

impl MarketState {
    pub fn new(cache_dir: &std::path::Path) -> Self {
        Self {
            index: RwLock::new(None),
            rebuild: Mutex::new(()),
            cache_file: cache_dir.join("marketplace-index.json"),
            api: Url::parse(STREMIO_ADDONS_NET_API).expect("valid directory URL"),
            catalog_transport: Url::parse(STREMIO_CATALOG_TRANSPORT).expect("valid URL"),
        }
    }

    #[cfg(test)]
    fn for_tests(cache_dir: &std::path::Path, api: Url, catalog_transport: Url) -> Self {
        Self {
            api,
            catalog_transport,
            ..Self::new(cache_dir)
        }
    }

    /// The listing with this key from the current index, if one was built.
    pub async fn listing(&self, key: &str) -> Option<DirectoryEntry> {
        self.index
            .read()
            .await
            .as_ref()?
            .entries
            .iter()
            .find(|e| e.key == key)
            .cloned()
    }

    /// The index for these settings: memory, else the cache file, else a rebuild. A
    /// rebuild that reaches no directory keeps a stale index (with the failures) if
    /// there is one.
    async fn get(
        &self,
        client: &AddonClient,
        settings: &Settings,
        force: bool,
    ) -> Result<MarketIndex, IpcError> {
        if !force
            && let Some(index) = self.index.read().await.as_ref()
            && index.fresh_for(settings)
        {
            return Ok(index.clone());
        }
        let _one = self.rebuild.lock().await;
        // someone else may have rebuilt while we waited
        if !force
            && let Some(index) = self.index.read().await.as_ref()
            && index.fresh_for(settings)
        {
            return Ok(index.clone());
        }
        if !force
            && let Ok(cached) = read_json::<MarketIndex>(&self.cache_file).await
            && cached.fresh_for(settings)
        {
            *self.index.write().await = Some(cached.clone());
            return Ok(cached);
        }
        let built = build_index(client, &self.api, &self.catalog_transport, settings).await;
        // nothing answered (as opposed to nothing asked, with every source off): keep
        // the last index built from the same sources, marked stale, and try again later
        if built.sources_ok.is_empty() && !built.sources_failed.is_empty() {
            let stale = match self.index.read().await.clone() {
                Some(index) => Some(index),
                None => read_json::<MarketIndex>(&self.cache_file).await.ok(),
            }
            .filter(|i| i.version == INDEX_VERSION && !i.entries.is_empty())
            .filter(|i| i.same_sources(settings));
            let mut served = stale.unwrap_or(built);
            served.sources_failed = served_failures(&served, settings);
            served.sources_ok.clear();
            served.retry_at = Some(now_ms() + RETRY_AFTER.as_millis() as u64);
            *self.index.write().await = Some(served.clone());
            return Ok(served);
        }
        if let Err(e) = write_json(&self.cache_file, &built).await {
            tracing::warn!("could not cache the marketplace index: {e}");
        }
        *self.index.write().await = Some(built.clone());
        Ok(built)
    }
}

/// The failures to show with a stale index: one per source it was built from.
fn served_failures(index: &MarketIndex, settings: &Settings) -> Vec<SourceFailure> {
    let mut failures = index.sources_failed.clone();
    let wanted = [
        (
            settings.marketplace_sources.stremio_addons_net,
            DirectorySource::StremioAddonsNet,
        ),
        (
            settings.marketplace_sources.stremio_catalog,
            DirectorySource::StremioCommunity,
        ),
        (
            settings.marketplace_sources.stremio_catalog,
            DirectorySource::StremioOfficial,
        ),
    ];
    for (on, source) in wanted {
        if on && !failures.iter().any(|f| f.source == source) {
            failures.push(SourceFailure {
                source,
                message: format!("{} could not be reached.", source.label()),
            });
        }
    }
    failures
}

fn failure(source: DirectorySource, error: &AddonError) -> SourceFailure {
    let what = match error {
        e if e.is_unreachable() => "could not be reached".to_string(),
        AddonError::BadStatus { status, .. } => format!("answered with HTTP {status}"),
        AddonError::Json(_) => "sent a listing Zeiler could not read".to_string(),
        AddonError::TooLarge { .. } => "sent a listing that is too large".to_string(),
        _ => "could not be read".to_string(),
    };
    SourceFailure {
        source,
        message: format!("{} {what}.", source.label()),
    }
}

async fn build_index(
    client: &AddonClient,
    api: &Url,
    catalog_transport: &Url,
    settings: &Settings,
) -> MarketIndex {
    let sources = &settings.marketplace_sources;
    let addons_net = async {
        if sources.stremio_addons_net {
            Some(directory::fetch_stremio_addons_net_index(client, api).await)
        } else {
            None
        }
    };
    let categories = async {
        if sources.stremio_addons_net {
            directory::fetch_stremio_addons_net_categories(client, api)
                .await
                .ok()
        } else {
            None
        }
    };
    let catalog = |id: &'static str, source: DirectorySource| async move {
        if sources.stremio_catalog {
            Some(directory::fetch_addon_catalog(client, catalog_transport, id, source).await)
        } else {
            None
        }
    };
    let (addons_net, categories, community, official) = tokio::join!(
        addons_net,
        categories,
        catalog("community", DirectorySource::StremioCommunity),
        catalog("official", DirectorySource::StremioOfficial),
    );

    let mut index = MarketIndex {
        version: INDEX_VERSION,
        fetched_at: now_ms(),
        with_addons_net: sources.stremio_addons_net,
        with_catalog: sources.stremio_catalog,
        categories: categories
            .filter(|c| !c.is_empty())
            .unwrap_or_else(directory::fallback_categories),
        ..Default::default()
    };
    let mut lists = Vec::new();
    for (source, result) in [
        (DirectorySource::StremioAddonsNet, addons_net),
        (DirectorySource::StremioOfficial, official),
        (DirectorySource::StremioCommunity, community),
    ] {
        match result {
            Some(Ok(entries)) => {
                index.sources_ok.push(source);
                lists.push(entries);
            }
            Some(Err(e)) => index.sources_failed.push(failure(source, &e)),
            None => {}
        }
    }
    index.entries = directory::merge_entries(lists);
    index
}

/// How a listing relates to what is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallState {
    BuiltIn,
    Installed,
    Available,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketEntryView {
    #[serde(flatten)]
    pub entry: DirectoryEntry,
    pub install_state: InstallState,
    /// The listing's page on stremio-addons.net.
    pub site_url: Option<String>,
}

fn install_state(entry: &DirectoryEntry, effective: &[Descriptor]) -> InstallState {
    match effective
        .iter()
        .find(|d| d.manifest.id == entry.key || d.transport_url == entry.transport_url)
    {
        Some(d) if d.flags.protected => InstallState::BuiltIn,
        Some(_) => InstallState::Installed,
        None => InstallState::Available,
    }
}

fn view(entry: &DirectoryEntry, effective: &[Descriptor]) -> MarketEntryView {
    MarketEntryView {
        install_state: install_state(entry, effective),
        site_url: entry
            .slug
            .as_deref()
            .and_then(directory::stremio_addons_net_page)
            .map(String::from),
        entry: entry.clone(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketPage {
    pub entries: Vec<MarketEntryView>,
    /// Matches for the query, over all pages.
    pub total: usize,
    pub has_more: bool,
    pub sources_ok: Vec<DirectorySource>,
    pub sources_failed: Vec<SourceFailure>,
    /// Unix milliseconds; 0 when nothing could be fetched.
    pub fetched_at: u64,
}

async fn enabled_settings(state: &AppState) -> Result<Settings, IpcError> {
    let settings = state.settings.read().await.clone();
    if !settings.marketplace_enabled {
        return Err(IpcError::InvalidInput(
            "The addon directory is off. Turn it on to browse listings.".into(),
        ));
    }
    Ok(settings)
}

fn visibility(settings: &Settings) -> DirectoryVisibility {
    DirectoryVisibility {
        show_adult: settings.show_adult,
        hide_p2p: settings.hide_p2p_addons,
    }
}

/// One page of the directory for `query`. Adult listings are left out unless
/// `showAdult`, P2P ones when `hideP2pAddons`.
#[tauri::command]
pub async fn marketplace_list(
    state: State<'_, AppState>,
    market: State<'_, MarketState>,
    query: DirectoryQuery,
    page: usize,
    limit: usize,
) -> Result<MarketPage, IpcError> {
    let settings = enabled_settings(&state).await?;
    let index = market.get(&state.client, &settings, false).await?;
    let effective = state.effective_addons().await;
    Ok(page_of(&index, &query, &settings, &effective, page, limit))
}

fn page_of(
    index: &MarketIndex,
    query: &DirectoryQuery,
    settings: &Settings,
    effective: &[Descriptor],
    page: usize,
    limit: usize,
) -> MarketPage {
    let limit = limit.clamp(1, 200);
    let selected = directory::filter_entries(&index.entries, query, visibility(settings));
    let start = page.saturating_mul(limit).min(selected.len());
    let end = start.saturating_add(limit).min(selected.len());
    MarketPage {
        entries: selected[start..end]
            .iter()
            .map(|e| view(e, effective))
            .collect(),
        total: selected.len(),
        has_more: end < selected.len(),
        sources_ok: index.sources_ok.clone(),
        sources_failed: index.sources_failed.clone(),
        fetched_at: index.fetched_at,
    }
}

/// The directory's categories (`nsfw` only while adult content is shown).
#[tauri::command]
pub async fn marketplace_categories(
    state: State<'_, AppState>,
    market: State<'_, MarketState>,
) -> Result<Vec<Category>, IpcError> {
    let settings = enabled_settings(&state).await?;
    let index = market.get(&state.client, &settings, false).await?;
    Ok(visible_categories(index.categories, settings.show_adult))
}

fn visible_categories(categories: Vec<Category>, show_adult: bool) -> Vec<Category> {
    categories
        .into_iter()
        .filter(|c| show_adult || c.slug != NSFW_CATEGORY)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDetail {
    pub entry: MarketEntryView,
    /// The listing's documentation as stremio-addons.net has it (markdown, possibly
    /// with HTML): shown as plain text only.
    pub documentation: Option<String>,
}

/// Most characters of documentation passed on.
const MAX_DOCUMENTATION_CHARS: usize = 20_000;

/// One listing, with its documentation when stremio-addons.net has some.
#[tauri::command]
pub async fn marketplace_detail(
    state: State<'_, AppState>,
    market: State<'_, MarketState>,
    key: String,
) -> Result<MarketDetail, IpcError> {
    let settings = enabled_settings(&state).await?;
    let index = market.get(&state.client, &settings, false).await?;
    let entry = index
        .entries
        .iter()
        .filter(|e| settings.show_adult || !e.adult)
        .find(|e| e.key == key)
        .ok_or_else(|| IpcError::NotFound("This addon is not in the directory.".into()))?;
    let documentation = match (&entry.slug, settings.marketplace_sources.stremio_addons_net) {
        (Some(slug), true) => {
            directory::fetch_stremio_addons_net_detail(&state.client, &market.api, slug)
                .await
                .ok()
                .and_then(|detail| detail.documentation)
                .map(|d| {
                    d.trim()
                        .chars()
                        .take(MAX_DOCUMENTATION_CHARS)
                        .collect::<String>()
                })
                .filter(|d| !d.is_empty())
        }
        _ => None,
    };
    let effective = state.effective_addons().await;
    Ok(MarketDetail {
        entry: view(entry, &effective),
        documentation,
    })
}

/// Rebuild the index now.
#[tauri::command]
pub async fn marketplace_refresh(
    state: State<'_, AppState>,
    market: State<'_, MarketState>,
) -> Result<(), IpcError> {
    let settings = enabled_settings(&state).await?;
    market.get(&state.client, &settings, true).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Answers the directory paths with fixed bodies and counts the requests.
    async fn serve(routes: Vec<(&'static str, String)>) -> (Url, Arc<AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let routes = routes.clone();
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => head.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&head);
                    let target = head.split_whitespace().nth(1).unwrap_or("").to_owned();
                    let path = target.split('?').next().unwrap_or("");
                    let (status, body) = routes
                        .iter()
                        .find(|(p, _)| *p == path)
                        .map_or(("404 Not Found", String::new()), |(_, b)| {
                            ("200 OK", b.clone())
                        });
                    let response = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (base, hits)
    }

    fn sa_page() -> String {
        include_str!("../../../crates/zeiler-addon/tests/fixtures/stremio-addons-net-list.json")
            .replace("\"hasNextPage\": true", "\"hasNextPage\": false")
    }

    fn community() -> String {
        include_str!("../../../crates/zeiler-addon/tests/fixtures/addon-catalog-community.json")
            .to_string()
    }

    fn enabled() -> Settings {
        Settings {
            marketplace_enabled: true,
            ..Settings::default()
        }
    }

    fn client() -> AddonClient {
        // loopback requests must not go to a proxy from the environment
        AddonClient::for_loopback_tests()
    }

    #[tokio::test]
    async fn builds_merges_caches_and_reports_failed_sources() {
        let (base, hits) = serve(vec![
            ("/api/addons", sa_page()),
            (
                "/api/categories",
                r#"{"categories":[{"name":"subtitles","slug":"subtitles"},{"name":"nsfw","slug":"nsfw"}]}"#
                    .into(),
            ),
            ("/addon_catalog/all/community.json", community()),
            // official is missing: 404
        ])
        .await;
        let dir = crate::persist::tests::temp_dir("market-index");
        let market = MarketState::for_tests(
            &dir,
            base.join("api/").unwrap(),
            base.join("manifest.json").unwrap(),
        );
        let settings = enabled();
        let index = market.get(&client(), &settings, false).await.unwrap();

        assert_eq!(
            index.sources_ok,
            [
                DirectorySource::StremioAddonsNet,
                DirectorySource::StremioCommunity
            ]
        );
        assert_eq!(index.sources_failed.len(), 1);
        assert_eq!(
            index.sources_failed[0].message,
            "Stremio official catalog answered with HTTP 404."
        );
        assert!(!index.sources_failed[0].message.contains("127.0.0.1"));
        assert!(index.entries.iter().any(|e| e.key == "org.example.radio"));
        assert_eq!(
            index
                .entries
                .iter()
                .filter(|e| e.key == "org.example.subs")
                .count(),
            1
        );
        assert!(dir.join("marketplace-index.json").exists());

        // reused from memory, then from the cache file, without asking again
        let asked = hits.load(Ordering::SeqCst);
        market.get(&client(), &settings, false).await.unwrap();
        let reloaded = MarketState::for_tests(
            &dir,
            base.join("api/").unwrap(),
            base.join("manifest.json").unwrap(),
        );
        let cached = reloaded.get(&client(), &settings, false).await.unwrap();
        assert_eq!(cached.entries.len(), index.entries.len());
        assert_eq!(hits.load(Ordering::SeqCst), asked);

        // showing adult content needs no rebuild (adult listings are always indexed)
        let adult = Settings {
            show_adult: true,
            ..settings.clone()
        };
        market.get(&client(), &adult, false).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), asked);
        assert!(index.entries.iter().any(|e| e.adult));

        // switching a source off rebuilds without it
        let no_catalog = Settings {
            marketplace_sources: crate::settings_groups::MarketplaceSources {
                stremio_catalog: false,
                ..Default::default()
            },
            ..settings.clone()
        };
        let rebuilt = market.get(&client(), &no_catalog, false).await.unwrap();
        assert_eq!(rebuilt.sources_ok, [DirectorySource::StremioAddonsNet]);
        assert!(!rebuilt.entries.iter().any(|e| e.key == "org.example.radio"));

        // the categories hide nsfw unless adult content is shown
        assert!(index.categories.iter().any(|c| c.slug == NSFW_CATEGORY));
        let hidden = visible_categories(index.categories.clone(), false);
        assert!(hidden.iter().all(|c| c.slug != NSFW_CATEGORY));
        assert!(hidden.iter().any(|c| c.slug == "subtitles"));
        assert_eq!(visible_categories(index.categories.clone(), true).len(), 2);
    }

    #[tokio::test]
    async fn a_rebuild_that_reaches_nothing_keeps_the_stale_index() {
        let (base, _) = serve(vec![
            ("/api/addons", sa_page()),
            ("/addon_catalog/all/community.json", community()),
            ("/addon_catalog/all/official.json", community()),
        ])
        .await;
        let dir = crate::persist::tests::temp_dir("market-stale");
        let market = MarketState::for_tests(
            &dir,
            base.join("api/").unwrap(),
            base.join("manifest.json").unwrap(),
        );
        let settings = enabled();
        let good = market.get(&client(), &settings, false).await.unwrap();
        assert!(good.sources_failed.is_empty());

        // the same cache dir, but every directory now unreachable
        let offline = MarketState::for_tests(
            &dir,
            Url::parse("http://127.0.0.1:9/api/").unwrap(),
            Url::parse("http://127.0.0.1:9/manifest.json").unwrap(),
        );
        let stale = offline.get(&client(), &settings, true).await.unwrap();
        assert_eq!(stale.entries.len(), good.entries.len());
        assert!(stale.sources_ok.is_empty());
        assert_eq!(stale.sources_failed.len(), 3);
        // kept in memory (so a listing can still be compared) and not rebuilt on the
        // next call
        assert!(offline.listing("org.example.subs").await.is_some());
        assert!(stale.fresh_for(&settings));

        // with every source off nothing is asked and nothing stale is shown
        let none = Settings {
            marketplace_sources: crate::settings_groups::MarketplaceSources {
                stremio_addons_net: false,
                stremio_catalog: false,
            },
            ..settings.clone()
        };
        let empty = offline.get(&client(), &none, false).await.unwrap();
        assert!(empty.entries.is_empty() && empty.sources_failed.is_empty());
    }

    #[test]
    fn pages_filter_and_mark_install_state() {
        let list: directory::SaList = serde_json::from_str(&sa_page()).unwrap();
        let entries = directory::merge_entries([list
            .addons
            .into_iter()
            .filter_map(directory::SaAddon::into_entry)
            .collect()]);
        let index = MarketIndex {
            entries,
            ..Default::default()
        };
        let installed: Descriptor = serde_json::from_value(serde_json::json!({
            "manifest": { "id": "org.example.subs", "name": "Mine" },
            "transportUrl": "https://elsewhere.example/manifest.json"
        }))
        .unwrap();
        let builtin: Descriptor = serde_json::from_value(serde_json::json!({
            "manifest": { "id": "org.example.meta", "name": "Meta" },
            "transportUrl": "https://meta.example.org/manifest.json",
            "flags": { "protected": true, "official": true }
        }))
        .unwrap();
        let effective = [builtin, installed];
        let settings = enabled();
        let query = DirectoryQuery::default();

        let first = page_of(&index, &query, &settings, &effective, 0, 2);
        assert_eq!(first.total, 4, "adult listing hidden");
        assert!(first.has_more);
        assert_eq!(first.entries.len(), 2);
        let all = page_of(&index, &query, &settings, &effective, 0, 50);
        let state = |key: &str| {
            all.entries
                .iter()
                .find(|e| e.entry.key == key)
                .unwrap()
                .install_state
        };
        assert_eq!(state("org.example.subs"), InstallState::Installed);
        assert_eq!(state("org.example.meta"), InstallState::BuiltIn);
        assert_eq!(state("org.example.streams"), InstallState::Available);
        let subs = all
            .entries
            .iter()
            .find(|e| e.entry.key == "org.example.subs")
            .unwrap();
        assert_eq!(
            subs.site_url.as_deref(),
            Some("https://stremio-addons.net/addons/example-subs")
        );
        // past the end: empty, not an error
        let past = page_of(&index, &query, &settings, &effective, 9, 50);
        assert!(past.entries.is_empty() && !past.has_more);

        let hidden_p2p = Settings {
            hide_p2p_addons: true,
            ..settings
        };
        let page = page_of(&index, &query, &hidden_p2p, &effective, 0, 50);
        assert!(page.entries.iter().all(|e| !e.entry.p2p));
    }
}
