use std::time::{Duration, Instant};

use futures::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, State};
use tokio::sync::Mutex;
use url::Url;
use zeiler_addon::{
    Descriptor, is_adult_manifest, manifest_changes, normalize_transport_input,
    without_adult_options,
};
use zeiler_sync::sync::AddonOp;

use crate::account;
use crate::commands;
use crate::commands::marketplace::MarketState;
use crate::error::IpcError;
use crate::state::AppState;

/// A transport URL as the UI shows it by default: scheme, host and port, and the
/// `/manifest.json` suffix, with any path or query in between (where configured
/// addons keep their keys) elided.
pub(crate) fn mask_transport_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or("");
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let middle = url.path() != "/manifest.json" || url.query().is_some();
    format!(
        "{}://{host}{port}{}/manifest.json",
        url.scheme(),
        if middle { "/…" } else { "" }
    )
}

/// How a previewed addon relates to one already installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::enum_variant_names, reason = "named as the UI shows them")]
pub enum MatchKind {
    /// The same transport URL: installing updates it in place.
    SameUrl,
    /// The same manifest id under another URL: most likely a reconfigured copy.
    SameId,
    /// Another addon from the same host and port: possibly a reconfigured copy.
    SameHost,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExistingMatch {
    pub kind: MatchKind,
    pub name: String,
    pub transport_url: String,
    pub masked_url: String,
}

/// What the install confirmation shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPreview {
    pub descriptor: Descriptor,
    pub masked_url: String,
    pub existing: Option<ExistingMatch>,
    /// The addon's setup page, when it is configurable.
    pub configure_url: Option<String>,
    /// Where the live manifest differs from the directory listing the user picked.
    pub listing_changes: Vec<String>,
    pub listing_requires_review: bool,
}

/// The addon's setup page: its transport URL with `/manifest.json` replaced by
/// `/configure` (the convention Stremio's apps and addon SDKs follow).
pub(crate) fn configure_url(descriptor: &Descriptor) -> Option<String> {
    let hints = &descriptor.manifest.behavior_hints;
    (hints.configurable || hints.configuration_required).then(|| {
        let mut url = descriptor.transport_url.clone();
        let path = url.path().trim_end_matches("/manifest.json").to_string();
        url.set_path(&format!("{path}/configure"));
        url.into()
    })
}

/// The installed user addon this one would update or could replace, if any: the same
/// URL first, then the same manifest id, then the same host.
pub(crate) fn find_match(
    installed: &[Descriptor],
    candidate: &Descriptor,
) -> Option<ExistingMatch> {
    let same_host = |d: &Descriptor| {
        d.transport_url.host_str().is_some()
            && d.transport_url.host_str() == candidate.transport_url.host_str()
            && d.transport_url.port_or_known_default()
                == candidate.transport_url.port_or_known_default()
    };
    let found = installed
        .iter()
        .find(|d| d.transport_url == candidate.transport_url)
        .map(|d| (MatchKind::SameUrl, d))
        .or_else(|| {
            installed
                .iter()
                .find(|d| d.manifest.id == candidate.manifest.id)
                .map(|d| (MatchKind::SameId, d))
        })
        .or_else(|| {
            installed
                .iter()
                .find(|d| same_host(d))
                .map(|d| (MatchKind::SameHost, d))
        })?;
    let (kind, d) = found;
    Some(ExistingMatch {
        kind,
        name: d.manifest.name.clone(),
        transport_url: d.transport_url.to_string(),
        masked_url: mask_transport_url(&d.transport_url),
    })
}

/// Fetch and parse a manifest without installing: feeds the install confirmation.
/// `listing_key` names the marketplace listing the install started from, so the live
/// manifest can be compared with what the listing showed.
#[tauri::command]
pub async fn addon_preview(
    state: State<'_, AppState>,
    market: State<'_, MarketState>,
    url: String,
    listing_key: Option<String>,
) -> Result<InstallPreview, IpcError> {
    let transport_url = normalize_transport_input(&url)?;
    let descriptor = state.client.descriptor(&transport_url).await?;
    let existing = find_match(&state.addons.read().await, &descriptor);
    let (listing_changes, listing_requires_review) = match listing_key {
        Some(key) => match market.listing(&key).await {
            Some(listing) => {
                let diff = manifest_changes(&listing.manifest, &descriptor.manifest);
                // what the addon serves, not its version or wording, is worth a warning
                let mut changes = diff.capability_changes().to_vec();
                let mut review = diff.requires_review;
                if listing.transport_url != descriptor.transport_url {
                    changes.insert(
                        0,
                        "It is installed from a different address than listed.".into(),
                    );
                    review = true;
                }
                (changes, review)
            }
            None => (Vec::new(), false),
        },
        None => (Vec::new(), false),
    };
    Ok(InstallPreview {
        masked_url: mask_transport_url(&descriptor.transport_url),
        configure_url: configure_url(&descriptor),
        existing,
        listing_changes,
        listing_requires_review,
        descriptor,
    })
}

/// Put `descriptor` into `addons`: in place of `replace` (keeping its position) when
/// given, else in place of the same URL, else at the end. A second copy of the new URL
/// elsewhere in the list is dropped.
pub(crate) fn install_into(
    addons: &mut Vec<Descriptor>,
    descriptor: Descriptor,
    replace: Option<&str>,
) -> Result<(), IpcError> {
    let target = match replace {
        Some(old) => Some(
            addons
                .iter()
                .position(|d| d.transport_url.as_str() == old)
                .ok_or_else(|| {
                    IpcError::NotFound("The addon to replace is no longer installed.".into())
                })?,
        ),
        None => addons
            .iter()
            .position(|d| d.transport_url == descriptor.transport_url),
    };
    match target {
        Some(at) => {
            let url = descriptor.transport_url.clone();
            addons[at] = descriptor;
            let mut index = 0;
            addons.retain(|d| {
                let keep = index == at || d.transport_url != url;
                index += 1;
                keep
            });
        }
        None => addons.push(descriptor),
    }
    Ok(())
}

/// Install (or update in place) an addon by transport URL. `replace` names an installed
/// addon's transport URL that the new one takes the place of (a reconfigured copy),
/// keeping its position in the priority order. The UI calls this only from the install
/// confirmation; the manifest is fetched again here, so what is stored is what the
/// addon serves now, never what the webview sent.
#[tauri::command]
pub async fn addon_add(
    app: AppHandle,
    state: State<'_, AppState>,
    updates: State<'_, UpdateCache>,
    url: String,
    replace: Option<String>,
) -> Result<Descriptor, IpcError> {
    let transport_url = normalize_transport_input(&url)?;
    let descriptor = state.client.descriptor(&transport_url).await?;

    if descriptor.manifest.behavior_hints.configuration_required {
        return Err(IpcError::InvalidInput(
            "This addon must be set up first. Open its setup page, then paste the link it gives you.".into(),
        ));
    }

    // a profile switch can't land between the change and its write
    let _profile = state.profile_op.lock().await;
    install_into(
        &mut *state.addons.write().await,
        descriptor.clone(),
        replace.as_deref(),
    )?;
    state.persist_addons().await?;
    updates.forget(&[descriptor.transport_url.as_str()]);
    if let Some(old) = &replace {
        updates.forget(&[old.as_str()]);
    }
    let mut ops: Vec<AddonOp> = account::to_raw(&descriptor)
        .map(AddonOp::Add)
        .into_iter()
        .collect();
    if let Some(old) = replace.filter(|old| *old != descriptor.transport_url.as_str()) {
        // the new copy takes the old one's place in the account too
        ops.push(AddonOp::Remove(old));
        ops.push(AddonOp::Reorder(user_order(&state).await));
    }
    commands::account::addons_changed(&app, ops).await;
    Ok(descriptor)
}

#[tauri::command]
pub async fn addon_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    updates: State<'_, UpdateCache>,
    transport_url: String,
) -> Result<(), IpcError> {
    let _profile = state.profile_op.lock().await;
    {
        let mut addons = state.addons.write().await;
        let before = addons.len();
        addons.retain(|d| d.transport_url.as_str() != transport_url);
        if addons.len() == before {
            return Err(IpcError::NotFound("addon is not installed".into()));
        }
    }
    state.persist_addons().await?;
    updates.forget(&[transport_url.as_str()]);
    commands::account::addons_changed(&app, vec![AddonOp::Remove(transport_url)]).await;
    Ok(())
}

/// The user's addons' transport URLs, in order.
async fn user_order(state: &AppState) -> Vec<String> {
    state
        .addons
        .read()
        .await
        .iter()
        .map(|d| d.transport_url.to_string())
        .collect()
}

/// The addons in effect, as the UI shows them: while adult content is hidden, their
/// catalogs have no adult options or adult-named catalogs (`catalog_load` refuses
/// those too), and an addon whose id or name is adult is flagged `adult` like one that
/// says so itself. A flagged addon keeps its catalogs here, for the Addons page; the
/// rooms leave them out.
#[tauri::command]
pub async fn addon_list(state: State<'_, AppState>) -> Result<Vec<Descriptor>, IpcError> {
    let addons = state.effective_addons().await;
    if state.settings.read().await.show_adult {
        return Ok(addons);
    }
    Ok(addons
        .into_iter()
        .map(|d| {
            let mut manifest = without_adult_options(d.manifest);
            manifest.behavior_hints.adult = is_adult_manifest(&manifest);
            Descriptor { manifest, ..d }
        })
        .collect())
}

/// `addons` in the order `order` gives (transport URLs), which must name each of them
/// exactly once.
pub(crate) fn reordered(
    addons: &[Descriptor],
    order: &[String],
) -> Result<Vec<Descriptor>, IpcError> {
    let refused = || {
        IpcError::Conflict(
            "Your addons changed meanwhile; the new order was not saved. Try again.".into(),
        )
    };
    if order.len() != addons.len() {
        return Err(refused());
    }
    let mut next = Vec::with_capacity(addons.len());
    for url in order {
        let found = addons
            .iter()
            .find(|d| d.transport_url.as_str() == url)
            .ok_or_else(refused)?;
        if next
            .iter()
            .any(|d: &Descriptor| d.transport_url == found.transport_url)
        {
            return Err(refused());
        }
        next.push(found.clone());
    }
    Ok(next)
}

/// Reorder the user's addons. Order is priority: details come from the first addon
/// that has them, and stream groups are listed in this order. Built-ins stay first.
#[tauri::command]
pub async fn addon_reorder(
    app: AppHandle,
    state: State<'_, AppState>,
    order: Vec<String>,
) -> Result<Vec<Descriptor>, IpcError> {
    let _profile = state.profile_op.lock().await;
    {
        let mut addons = state.addons.write().await;
        *addons = reordered(&addons, &order)?;
    }
    state.persist_addons().await?;
    commands::account::addons_changed(&app, vec![AddonOp::Reorder(order)]).await;
    Ok(state.effective_addons().await)
}

// ---- update detection ------------------------------------------------------------------

/// How long a check is reused unless forced.
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Addons asked at once, and how long each may take.
const UPDATE_CONCURRENCY: usize = 6;
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10);

/// An installed addon whose live manifest differs from the stored one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonUpdate {
    pub transport_url: String,
    pub masked_url: String,
    pub name: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub changes: Vec<String>,
    /// More than name, description, logo or version changed.
    pub capabilities_changed: bool,
    /// Streams, P2P, adult content or setup added, or the id changed.
    pub requires_review: bool,
}

/// The last update check, reused for six hours. Never locked across a network call:
/// installs and removals only touch `inner` (a check in progress is serialized by
/// `checking` alone), and what they `forget` while a check runs is dropped from its
/// result too.
#[derive(Default)]
pub struct UpdateCache {
    inner: std::sync::Mutex<UpdateCacheInner>,
    checking: Mutex<()>,
}

#[derive(Default)]
struct UpdateCacheInner {
    last: Option<(Instant, Vec<AddonUpdate>)>,
    /// Addons forgotten since the running check started.
    forgotten: Vec<String>,
    /// Cleared (a profile switch) since the running check started: drop its result.
    cleared: bool,
}

impl UpdateCache {
    fn inner(&self) -> std::sync::MutexGuard<'_, UpdateCacheInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Drop what the last check (and any check still running) said about these addons
    /// (installed, updated or removed since).
    pub fn forget(&self, transport_urls: &[&str]) {
        let mut inner = self.inner();
        if let Some((_, updates)) = inner.last.as_mut() {
            updates.retain(|u| !transport_urls.contains(&u.transport_url.as_str()));
        }
        inner
            .forgotten
            .extend(transport_urls.iter().map(|u| u.to_string()));
    }

    /// The last result, if it is younger than six hours.
    fn fresh(&self) -> Option<Vec<AddonUpdate>> {
        match &self.inner().last {
            Some((at, updates)) if at.elapsed() < UPDATE_CHECK_EVERY => Some(updates.clone()),
            _ => None,
        }
    }

    /// Forget everything (another profile's addons are in effect now).
    pub fn clear(&self) {
        let mut inner = self.inner();
        inner.last = None;
        inner.forgotten.clear();
        inner.cleared = true;
    }

    fn begin(&self) {
        let mut inner = self.inner();
        inner.forgotten.clear();
        inner.cleared = false;
    }

    /// Store a finished check's result, without what was forgotten meanwhile.
    fn finish(&self, mut updates: Vec<AddonUpdate>) -> Vec<AddonUpdate> {
        let mut inner = self.inner();
        let forgotten = std::mem::take(&mut inner.forgotten);
        if std::mem::take(&mut inner.cleared) {
            return Vec::new();
        }
        updates.retain(|u| !forgotten.contains(&u.transport_url));
        inner.last = Some((Instant::now(), updates.clone()));
        updates
    }
}

/// Fetch every user addon's manifest again (six at a time, ten seconds each) and report
/// the ones that changed. Reuses the last answer for six hours unless `force`. An addon
/// that doesn't answer is left out (its error names only its host, in the log).
#[tauri::command]
pub async fn addon_check_updates(
    state: State<'_, AppState>,
    cache: State<'_, UpdateCache>,
    force: bool,
) -> Result<Vec<AddonUpdate>, IpcError> {
    let _one = cache.checking.lock().await;
    if !force && let Some(updates) = cache.fresh() {
        return Ok(updates);
    }
    cache.begin();
    let installed = state.addons.read().await.clone();
    let client = state.client.clone();
    let updates: Vec<AddonUpdate> = futures::stream::iter(installed)
        .map(|addon| {
            let client = client.clone();
            async move {
                let fetched =
                    tokio::time::timeout(UPDATE_TIMEOUT, client.manifest(&addon.transport_url))
                        .await;
                let fresh = match fetched {
                    Ok(Ok(manifest)) => manifest,
                    Ok(Err(e)) => {
                        tracing::info!(
                            host = addon.transport_url.host_str().unwrap_or(""),
                            unreachable = e.is_unreachable(),
                            "update check failed"
                        );
                        return None;
                    }
                    Err(_) => return None,
                };
                let diff = manifest_changes(&addon.manifest, &fresh);
                (!diff.is_empty()).then(|| AddonUpdate {
                    transport_url: addon.transport_url.to_string(),
                    masked_url: mask_transport_url(&addon.transport_url),
                    name: addon.manifest.name.clone(),
                    current_version: addon.manifest.version.clone(),
                    latest_version: fresh.version.clone(),
                    changes: diff.changes,
                    capabilities_changed: diff.capabilities_changed,
                    requires_review: diff.requires_review,
                })
            }
        })
        .buffered(UPDATE_CONCURRENCY)
        .filter_map(|u| async move { u })
        .collect()
        .await;
    Ok(cache.finish(updates))
}

/// Apply an update that changes nothing but name, description, logo or version, in
/// one click. Anything more refuses with `conflict`, and the UI opens the full
/// install confirmation instead.
#[tauri::command]
pub async fn addon_refresh(
    app: AppHandle,
    state: State<'_, AppState>,
    cache: State<'_, UpdateCache>,
    transport_url: String,
) -> Result<Descriptor, IpcError> {
    let url = Url::parse(&transport_url)
        .map_err(|_| IpcError::InvalidInput("That is not an addon address.".into()))?;
    let current = state
        .addons
        .read()
        .await
        .iter()
        .find(|d| d.transport_url == url)
        .cloned()
        .ok_or_else(|| IpcError::NotFound("addon is not installed".into()))?;
    let fresh = state.client.descriptor(&url).await?;
    if manifest_changes(&current.manifest, &fresh.manifest).capabilities_changed {
        return Err(IpcError::Conflict(
            "This update changes what the addon can do. Review it before installing.".into(),
        ));
    }
    // in place, and NotFound if it was removed meanwhile (never added back); a switch
    // to another profile meanwhile makes it NotFound too
    let _profile = state.profile_op.lock().await;
    install_into(
        &mut *state.addons.write().await,
        fresh.clone(),
        Some(transport_url.as_str()),
    )?;
    state.persist_addons().await?;
    cache.forget(&[transport_url.as_str()]);
    commands::account::addons_changed(
        &app,
        account::to_raw(&fresh)
            .map(AddonOp::Add)
            .into_iter()
            .collect(),
    )
    .await;
    Ok(fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addon(url: &str, id: &str) -> Descriptor {
        serde_json::from_value(serde_json::json!({
            "manifest": { "id": id, "name": id },
            "transportUrl": url
        }))
        .unwrap()
    }

    fn urls(addons: &[Descriptor]) -> Vec<&str> {
        addons.iter().map(|d| d.transport_url.as_str()).collect()
    }

    #[test]
    fn masking_hides_paths_and_queries_but_keeps_the_host() {
        let mask = |s: &str| mask_transport_url(&Url::parse(s).unwrap());
        assert_eq!(
            mask("https://a.example/manifest.json"),
            "https://a.example/manifest.json"
        );
        assert_eq!(
            mask("https://a.example/secret-key%7B%7D/manifest.json"),
            "https://a.example/…/manifest.json"
        );
        assert_eq!(
            mask("http://127.0.0.1:7000/manifest.json?key=1"),
            "http://127.0.0.1:7000/…/manifest.json"
        );
    }

    #[test]
    fn matches_prefer_the_same_url_then_id_then_host() {
        let installed = [
            addon("https://a.example/one/manifest.json", "org.one"),
            addon("https://b.example/manifest.json", "org.two"),
        ];
        let kind = |url: &str, id: &str| find_match(&installed, &addon(url, id)).map(|m| m.kind);
        assert_eq!(
            kind("https://a.example/one/manifest.json", "org.other"),
            Some(MatchKind::SameUrl)
        );
        assert_eq!(
            kind("https://c.example/manifest.json", "org.two"),
            Some(MatchKind::SameId)
        );
        let host = find_match(
            &installed,
            &addon("https://a.example/new-key/manifest.json", "org.three"),
        )
        .unwrap();
        assert_eq!(host.kind, MatchKind::SameHost);
        assert_eq!(host.masked_url, "https://a.example/…/manifest.json");
        assert_eq!(kind("https://a.example:8443/manifest.json", "org.x"), None);
        assert_eq!(kind("https://d.example/manifest.json", "org.x"), None);
    }

    #[test]
    fn installing_appends_updates_in_place_or_replaces_keeping_the_position() {
        let mut addons = vec![
            addon("https://a.example/manifest.json", "a"),
            addon("https://b.example/old/manifest.json", "b"),
            addon("https://c.example/manifest.json", "c"),
        ];
        install_into(
            &mut addons,
            addon("https://d.example/manifest.json", "d"),
            None,
        )
        .unwrap();
        assert_eq!(urls(&addons)[3], "https://d.example/manifest.json");

        install_into(
            &mut addons,
            addon("https://a.example/manifest.json", "a2"),
            None,
        )
        .unwrap();
        assert_eq!(addons.len(), 4);
        assert_eq!(addons[0].manifest.id, "a2");

        install_into(
            &mut addons,
            addon("https://b.example/new/manifest.json", "b"),
            Some("https://b.example/old/manifest.json"),
        )
        .unwrap();
        assert_eq!(
            urls(&addons),
            [
                "https://a.example/manifest.json",
                "https://b.example/new/manifest.json",
                "https://c.example/manifest.json",
                "https://d.example/manifest.json"
            ]
        );

        // replacing with a URL that is already installed elsewhere leaves one copy, here
        install_into(
            &mut addons,
            addon("https://d.example/manifest.json", "d"),
            Some("https://a.example/manifest.json"),
        )
        .unwrap();
        assert_eq!(
            urls(&addons),
            [
                "https://d.example/manifest.json",
                "https://b.example/new/manifest.json",
                "https://c.example/manifest.json"
            ]
        );

        let missing = install_into(
            &mut addons,
            addon("https://e.example/manifest.json", "e"),
            Some("https://gone.example/manifest.json"),
        );
        assert!(matches!(missing, Err(IpcError::NotFound(_))));
        assert_eq!(addons.len(), 3);
    }

    #[test]
    fn reorder_accepts_only_an_exact_permutation() {
        let addons = [
            addon("https://a.example/manifest.json", "a"),
            addon("https://b.example/manifest.json", "b"),
            addon("https://c.example/manifest.json", "c"),
        ];
        let order = |urls: &[&str]| urls.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let next = reordered(
            &addons,
            &order(&[
                "https://c.example/manifest.json",
                "https://a.example/manifest.json",
                "https://b.example/manifest.json",
            ]),
        )
        .unwrap();
        assert_eq!(
            next.iter()
                .map(|d| d.manifest.id.as_str())
                .collect::<Vec<_>>(),
            ["c", "a", "b"]
        );
        for bad in [
            order(&[
                "https://a.example/manifest.json",
                "https://b.example/manifest.json",
            ]),
            order(&[
                "https://a.example/manifest.json",
                "https://a.example/manifest.json",
                "https://b.example/manifest.json",
            ]),
            order(&[
                "https://a.example/manifest.json",
                "https://b.example/manifest.json",
                "https://x.example/manifest.json",
            ]),
            order(&[
                "https://a.example/manifest.json",
                "https://b.example/manifest.json",
                "https://c.example/manifest.json",
                "https://c.example/manifest.json",
            ]),
        ] {
            assert!(
                matches!(reordered(&addons, &bad), Err(IpcError::Conflict(_))),
                "{bad:?}"
            );
        }
    }

    fn update(url: &str) -> AddonUpdate {
        AddonUpdate {
            transport_url: url.into(),
            masked_url: String::new(),
            name: String::new(),
            current_version: None,
            latest_version: None,
            changes: vec!["Version 1 → 2.".into()],
            capabilities_changed: false,
            requires_review: false,
        }
    }

    #[test]
    fn what_is_forgotten_during_a_check_is_left_out_of_its_result() {
        let cache = UpdateCache::default();
        cache.begin();
        cache.forget(&["https://a.example/manifest.json"]);
        let kept = cache.finish(vec![
            update("https://a.example/manifest.json"),
            update("https://b.example/manifest.json"),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            cache.fresh().unwrap()[0].transport_url,
            "https://b.example/manifest.json"
        );
        cache.forget(&["https://b.example/manifest.json"]);
        assert!(cache.fresh().unwrap().is_empty());
        // a later check starts with a clean slate
        cache.begin();
        assert_eq!(
            cache
                .finish(vec![update("https://a.example/manifest.json")])
                .len(),
            1
        );
    }

    #[test]
    fn configure_urls_only_for_configurable_addons() {
        let mut d = addon("https://a.example/key/manifest.json?x=1", "a");
        assert_eq!(configure_url(&d), None);
        d.manifest.behavior_hints.configurable = true;
        assert_eq!(
            configure_url(&d).as_deref(),
            Some("https://a.example/key/configure?x=1")
        );
    }
}
