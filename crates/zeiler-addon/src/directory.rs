//! Third-party addon directories, for the opt-in marketplace: Stremio's own
//! `addon_catalog` resource (served by Cinemeta) and the stremio-addons.net community
//! API.
//!
//! Listings are untrusted, display-only data. A [`DirectoryEntry`] is never installed
//! from: installing refetches the live manifest from the listed transport URL and goes
//! through the normal confirmation dialog. Remote `flags` (`official`, `protected`) are
//! dropped on the way in, so a listing can never mark itself as a built-in.
//!
//! The endpoints, query parameters and the merge-by-manifest-id approach follow Harbor's
//! `src/lib/providers/stremio-addons.ts` and `src/lib/addons-store/community.ts` (MIT,
//! Copyright (c) 2026 Harbor). Zeiler ports no curation: no featured rails, no
//! recommendations, no bundled list; the only orderings are the directory's own stars,
//! listing date, or the name.

use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, DefaultOnNull, VecSkipError, serde_as};
use url::Url;

use crate::adult::is_adult_manifest;
use crate::client::AddonClient;
use crate::error::AddonError;
use crate::manifest::Manifest;
use crate::request::{ResourcePath, validate_transport_url};
use crate::resource::AddonCatalogResponse;

/// The stremio-addons.net API root.
pub const STREMIO_ADDONS_NET_API: &str = "https://stremio-addons.net/api/v0/";
/// Pages of the stremio-addons.net index fetched at most (100 listings each).
pub const STREMIO_ADDONS_NET_MAX_PAGES: u32 = 10;
/// The category stremio-addons.net files adult addons under.
pub const NSFW_CATEGORY: &str = "nsfw";

/// Where a listing came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectorySource {
    /// Cinemeta's `addon_catalog/all/community`.
    StremioCommunity,
    /// Cinemeta's `addon_catalog/all/official`.
    StremioOfficial,
    /// The stremio-addons.net community API.
    StremioAddonsNet,
}

impl DirectorySource {
    pub fn label(self) -> &'static str {
        match self {
            DirectorySource::StremioCommunity => "Stremio community catalog",
            DirectorySource::StremioOfficial => "Stremio official catalog",
            DirectorySource::StremioAddonsNet => "stremio-addons.net",
        }
    }
}

/// A stremio-addons.net category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    pub slug: String,
}

/// The categories to offer when stremio-addons.net doesn't answer (its list as of
/// 2026-10, without `nsfw`, which callers add only when adult content is shown).
pub fn fallback_categories() -> Vec<Category> {
    [
        ("anime", "anime"),
        ("asian drama", "asian+drama"),
        ("bollywood", "bollywood"),
        ("debrid support", "debrid+support"),
        ("http streams", "http+streams"),
        ("live tv", "live+tv"),
        ("metadata", "metadata"),
        ("misc", "misc"),
        ("movies", "movies"),
        ("music", "music"),
        ("radios", "radios"),
        ("subtitles", "subtitles"),
        ("torrents", "torrents"),
        ("tracking", "tracking"),
        ("tv shows", "tv+shows"),
        ("usenet", "usenet"),
    ]
    .into_iter()
    .map(|(name, slug)| Category {
        name: name.into(),
        slug: slug.into(),
    })
    .collect()
}

/// One addon as a directory lists it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryEntry {
    /// The manifest id: the dedupe key, and how the detail page is addressed.
    pub key: String,
    /// The listing's copy of the manifest. For display only; it may be stale or wrong.
    pub manifest: Manifest,
    pub transport_url: Url,
    pub sources: Vec<DirectorySource>,
    /// stremio-addons.net stars.
    pub stars: Option<u32>,
    pub categories: Vec<Category>,
    /// When stremio-addons.net first listed it (RFC 3339).
    pub listed_at: Option<String>,
    pub updated_at: Option<String>,
    /// stremio-addons.net slug, for its detail endpoint and page.
    pub slug: Option<String>,
    /// Declared adult, filed as `nsfw`, or named like adult content.
    pub adult: bool,
    /// Declared P2P (`behaviorHints.p2p`). Self-declared: many torrent addons don't say.
    pub p2p: bool,
    pub configurable: bool,
    pub configuration_required: bool,
}

impl DirectoryEntry {
    fn new(manifest: Manifest, transport_url: Url, source: DirectorySource) -> Self {
        let mut entry = DirectoryEntry {
            key: manifest.id.clone(),
            transport_url,
            sources: vec![source],
            stars: None,
            categories: Vec::new(),
            listed_at: None,
            updated_at: None,
            slug: None,
            adult: false,
            p2p: false,
            configurable: false,
            configuration_required: false,
            manifest,
        };
        entry.classify();
        entry
    }

    fn classify(&mut self) {
        let hints = &self.manifest.behavior_hints;
        self.adult = is_adult_manifest(&self.manifest)
            || self.categories.iter().any(|c| c.slug == NSFW_CATEGORY);
        self.p2p = hints.p2p;
        self.configurable = hints.configurable || hints.configuration_required;
        self.configuration_required = hints.configuration_required;
    }

    /// The resource names it serves; `catalog` / `addon_catalog` when it lists any.
    pub fn resource_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .manifest
            .resources
            .iter()
            .map(|r| r.name().to_string())
            .collect();
        if !self.manifest.catalogs.is_empty() {
            names.push("catalog".into());
        }
        if !self.manifest.addon_catalogs.is_empty() {
            names.push("addon_catalog".into());
        }
        names.sort();
        names.dedup();
        names
    }

    /// The content types it covers: the manifest's, its resources' and its catalogs'.
    pub fn types(&self) -> Vec<String> {
        let mut types = self.manifest.types.clone();
        for resource in &self.manifest.resources {
            if let crate::manifest::ManifestResource::Full {
                types: Some(more), ..
            } = resource
            {
                types.extend(more.iter().cloned());
            }
        }
        types.extend(self.manifest.catalogs.iter().map(|c| c.r#type.clone()));
        types.sort();
        types.dedup();
        types
    }

    fn matches_search(&self, needle: &str) -> bool {
        let m = &self.manifest;
        [
            Some(m.name.as_str()),
            Some(m.id.as_str()),
            m.description.as_deref(),
            self.slug.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|field| field.to_lowercase().contains(needle))
    }
}

// ---- stremio-addons.net shapes (lenient) ---------------------------------------------

/// One stremio-addons.net listing.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaAddon {
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub manifest_url: Option<String>,
    /// Kept raw until the transport URL is known, so a relative logo can be resolved.
    #[serde(default)]
    pub manifest: Option<serde_json::Value>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub slug: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub stars: Option<u32>,
    #[serde_as(deserialize_as = "DefaultOnError<DefaultOnNull<VecSkipError<_>>>")]
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Markdown (may contain HTML): show as plain text only.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub documentation: Option<String>,
}

#[serde_as]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaPagination {
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub has_next_page: bool,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub total_pages: Option<u32>,
}

/// A page of `GET /addons`.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct SaList {
    #[serde_as(deserialize_as = "DefaultOnError<DefaultOnNull<VecSkipError<_>>>")]
    #[serde(default)]
    pub addons: Vec<SaAddon>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub pagination: SaPagination,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
struct SaCategories {
    #[serde_as(deserialize_as = "DefaultOnError<DefaultOnNull<VecSkipError<_>>>")]
    #[serde(default)]
    categories: Vec<Category>,
}

impl SaAddon {
    /// The listing as an entry; `None` without a manifest or a usable transport URL.
    pub fn into_entry(self) -> Option<DirectoryEntry> {
        let transport_url = Url::parse(self.manifest_url?.trim()).ok()?;
        validate_transport_url(&transport_url).ok()?;
        let manifest = parse_listed_manifest(self.manifest?, &transport_url)?;
        let mut entry =
            DirectoryEntry::new(manifest, transport_url, DirectorySource::StremioAddonsNet);
        entry.stars = self.stars;
        entry.categories = self.categories;
        entry.listed_at = self.created_at;
        entry.updated_at = self.updated_at;
        entry.slug = self.slug.filter(|s| is_slug(s));
        entry.classify();
        Some(entry)
    }
}

/// A listing's manifest, with a relative logo resolved against its transport URL (as
/// a live fetch does, so the two compare equal); `None` when it isn't a manifest.
fn parse_listed_manifest(mut raw: serde_json::Value, transport_url: &Url) -> Option<Manifest> {
    crate::client::resolve_relative_images(&mut raw, transport_url);
    serde_json::from_value(raw).ok()
}

/// Slugs go into a URL path segment: keep them to what stremio-addons.net uses.
fn is_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 128
        && slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// An `addon_catalog` response as entries, without the listings' remote flags.
pub fn entries_from_addon_catalog(
    response: AddonCatalogResponse,
    source: DirectorySource,
) -> Vec<DirectoryEntry> {
    response
        .addons
        .into_iter()
        .filter(|d| validate_transport_url(&d.transport_url).is_ok())
        .map(|d| DirectoryEntry::new(d.manifest, d.transport_url, source))
        .collect()
}

/// Merge listings from several sources: one entry per manifest id, the first seen
/// kept (pass stremio-addons.net first, sorted by stars, so the most-starred instance
/// of an id wins), with every source that listed it, and the stremio-addons.net
/// details (stars, categories, slug, dates) from whichever listing has them.
pub fn merge_entries(lists: impl IntoIterator<Item = Vec<DirectoryEntry>>) -> Vec<DirectoryEntry> {
    let mut merged: Vec<DirectoryEntry> = Vec::new();
    let mut index = std::collections::HashMap::<String, usize>::new();
    for entry in lists.into_iter().flatten() {
        match index.get(&entry.key) {
            Some(&at) => {
                let kept = &mut merged[at];
                for source in entry.sources {
                    if !kept.sources.contains(&source) {
                        kept.sources.push(source);
                    }
                }
                if kept.stars.is_none() && entry.stars.is_some() {
                    kept.stars = entry.stars;
                    kept.categories = entry.categories;
                    kept.slug = entry.slug;
                    kept.listed_at = entry.listed_at;
                    kept.updated_at = entry.updated_at;
                    kept.classify();
                }
                kept.adult |= entry.adult;
            }
            None => {
                index.insert(entry.key.clone(), merged.len());
                merged.push(entry);
            }
        }
    }
    merged
}

// ---- filtering -------------------------------------------------------------------------

/// How a directory page is ordered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectorySort {
    /// Most stremio-addons.net stars first (unstarred last, then by name).
    #[default]
    Stars,
    /// Most recently listed first.
    Newest,
    /// A–Z by name.
    Name,
}

/// What to show of the index. Facets combine with AND; values within a facet with OR.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DirectoryQuery {
    pub search: Option<String>,
    pub types: Vec<String>,
    /// `stream`, `subtitles`, `catalog`, `meta`, `addon_catalog`.
    pub resources: Vec<String>,
    pub category: Option<String>,
    pub sort: DirectorySort,
}

/// Visibility rules that come from settings, not from the query.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirectoryVisibility {
    pub show_adult: bool,
    pub hide_p2p: bool,
}

/// The entries `query` selects, in its order. Adult entries are dropped unless shown,
/// and P2P ones only when hidden; the `nsfw` category selects nothing unless adult
/// content is shown.
pub fn filter_entries<'a>(
    entries: &'a [DirectoryEntry],
    query: &DirectoryQuery,
    visibility: DirectoryVisibility,
) -> Vec<&'a DirectoryEntry> {
    let needle = query
        .search
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let mut selected: Vec<&DirectoryEntry> = entries
        .iter()
        .filter(|e| visibility.show_adult || !e.adult)
        .filter(|e| !(visibility.hide_p2p && e.p2p))
        .filter(|e| needle.as_deref().is_none_or(|n| e.matches_search(n)))
        .filter(|e| {
            query.types.is_empty() || {
                let types = e.types();
                query.types.iter().any(|t| types.contains(t))
            }
        })
        .filter(|e| {
            query.resources.is_empty() || {
                let names = e.resource_names();
                query.resources.iter().any(|r| names.contains(r))
            }
        })
        .filter(|e| {
            query
                .category
                .as_deref()
                .is_none_or(|slug| e.categories.iter().any(|c| c.slug == slug))
        })
        .collect();
    let by_name = |a: &&DirectoryEntry, b: &&DirectoryEntry| {
        a.manifest
            .name
            .to_lowercase()
            .cmp(&b.manifest.name.to_lowercase())
    };
    match query.sort {
        DirectorySort::Stars => selected.sort_by(|a, b| {
            b.stars
                .unwrap_or(0)
                .cmp(&a.stars.unwrap_or(0))
                .then_with(|| by_name(a, b))
        }),
        DirectorySort::Newest => selected.sort_by(|a, b| {
            // RFC 3339 in UTC sorts as text; unlisted last
            b.listed_at
                .as_deref()
                .unwrap_or("")
                .cmp(a.listed_at.as_deref().unwrap_or(""))
                .then_with(|| by_name(a, b))
        }),
        DirectorySort::Name => selected.sort_by(by_name),
    }
    selected
}

// ---- fetching --------------------------------------------------------------------------

/// The listing site's own page for a slug.
pub fn stremio_addons_net_page(slug: &str) -> Option<Url> {
    is_slug(slug)
        .then(|| Url::parse(&format!("https://stremio-addons.net/addons/{slug}")).ok())
        .flatten()
}

/// `GET {api}addons?page=…&limit=…&sort_by=stars&order=desc[&nsfw=exclude]`. The
/// index asks with adult listings included, so their `nsfw` category is known and
/// the same addon from another directory is hidden too.
pub async fn fetch_stremio_addons_net_page(
    client: &AddonClient,
    api: &Url,
    page: u32,
    limit: u32,
    include_adult: bool,
) -> Result<SaList, AddonError> {
    let mut url = api
        .join("addons")
        .map_err(|e| AddonError::Other(format!("bad directory URL: {e}")))?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("page", &page.to_string())
            .append_pair("limit", &limit.to_string())
            .append_pair("sort_by", "stars")
            .append_pair("order", "desc");
        if !include_adult {
            query.append_pair("nsfw", "exclude");
        }
    }
    client.get_json(url).await
}

/// Every page of the stremio-addons.net index, up to [`STREMIO_ADDONS_NET_MAX_PAGES`],
/// adult listings included (callers filter on `adult`, which knows their category).
/// A failure on the first page fails; a later one ends the walk with what it has.
pub async fn fetch_stremio_addons_net_index(
    client: &AddonClient,
    api: &Url,
) -> Result<Vec<DirectoryEntry>, AddonError> {
    let mut entries = Vec::new();
    for page in 1..=STREMIO_ADDONS_NET_MAX_PAGES {
        let list = match fetch_stremio_addons_net_page(client, api, page, 100, true).await {
            Ok(list) => list,
            Err(e) if page == 1 => return Err(e),
            Err(e) => {
                tracing::warn!("stremio-addons.net page {page} failed: {e}");
                break;
            }
        };
        let done = !list.pagination.has_next_page || list.addons.is_empty();
        entries.extend(list.addons.into_iter().filter_map(SaAddon::into_entry));
        if done {
            break;
        }
    }
    Ok(entries)
}

/// `GET {api}addons/{slug}`: one listing with its documentation.
pub async fn fetch_stremio_addons_net_detail(
    client: &AddonClient,
    api: &Url,
    slug: &str,
) -> Result<SaAddon, AddonError> {
    if !is_slug(slug) {
        return Err(AddonError::Other("not a directory slug".into()));
    }
    let url = api
        .join(&format!("addons/{slug}"))
        .map_err(|e| AddonError::Other(format!("bad directory URL: {e}")))?;
    client.get_json(url).await
}

/// `GET {api}categories`.
pub async fn fetch_stremio_addons_net_categories(
    client: &AddonClient,
    api: &Url,
) -> Result<Vec<Category>, AddonError> {
    let url = api
        .join("categories")
        .map_err(|e| AddonError::Other(format!("bad directory URL: {e}")))?;
    let categories: SaCategories = client.get_json(url).await?;
    Ok(categories
        .categories
        .into_iter()
        .filter(|c| is_slug(&c.slug.replace('+', "-")))
        .collect())
}

/// `addon_catalog/all/{id}` from an addon that declares it (Cinemeta: `community`,
/// `official`).
pub async fn fetch_addon_catalog(
    client: &AddonClient,
    transport_url: &Url,
    id: &str,
    source: DirectorySource,
) -> Result<Vec<DirectoryEntry>, AddonError> {
    let response: AddonCatalogResponse = client
        .resource(
            transport_url,
            &ResourcePath::new("addon_catalog", "all", id),
        )
        .await?;
    Ok(entries_from_addon_catalog(response, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SA_LIST: &str = include_str!("../tests/fixtures/stremio-addons-net-list.json");
    const COMMUNITY: &str = include_str!("../tests/fixtures/addon-catalog-community.json");

    fn sa_entries() -> Vec<DirectoryEntry> {
        let list: SaList = serde_json::from_str(SA_LIST).unwrap();
        list.addons
            .into_iter()
            .filter_map(SaAddon::into_entry)
            .collect()
    }

    fn community_entries() -> Vec<DirectoryEntry> {
        let response: AddonCatalogResponse = serde_json::from_str(COMMUNITY).unwrap();
        entries_from_addon_catalog(response, DirectorySource::StremioCommunity)
    }

    fn keys(entries: &[&DirectoryEntry]) -> Vec<String> {
        entries.iter().map(|e| e.key.clone()).collect()
    }

    #[test]
    fn stremio_addons_net_listings_parse_leniently() {
        let list: SaList = serde_json::from_str(SA_LIST).unwrap();
        assert!(list.pagination.has_next_page);
        assert_eq!(list.pagination.total_pages, Some(7));
        // garbage rows are skipped or dropped, never fatal
        let entries = sa_entries();
        assert_eq!(
            entries.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            [
                "org.example.subs",
                "org.example.streams",
                "org.example.mirror-streams",
                "org.example.night",
                "org.example.meta"
            ]
        );
        let subs = &entries[0];
        assert_eq!(subs.stars, Some(120));
        assert_eq!(subs.slug.as_deref(), Some("example-subs"));
        assert_eq!(subs.categories[0].slug, "subtitles");
        assert_eq!(subs.listed_at.as_deref(), Some("2026-10-10T08:00:00.000Z"));
        assert_eq!(subs.sources, [DirectorySource::StremioAddonsNet]);
        // a relative logo resolves against the listing's transport URL, as a live
        // fetch of the manifest does
        assert_eq!(
            subs.manifest.logo.as_ref().map(|u| u.as_str()),
            Some("https://subs.example.org/images/logo.png")
        );
        // a slug that isn't one is dropped rather than put in a URL
        assert_eq!(entries[4].slug, None);
        // null categories and a string for stars read as absent
        assert!(entries[4].categories.is_empty());
        assert_eq!(entries[4].stars, None);
    }

    #[test]
    fn classification_covers_adult_p2p_and_setup() {
        let entries = sa_entries();
        let get = |key: &str| entries.iter().find(|e| e.key == key).unwrap();
        assert!(get("org.example.streams").p2p);
        assert!(get("org.example.streams").configurable);
        assert!(!get("org.example.streams").configuration_required);
        assert!(get("org.example.mirror-streams").configuration_required);
        assert!(get("org.example.mirror-streams").configurable);
        // filed as nsfw, without saying so in its manifest
        assert!(get("org.example.night").adult);
        assert!(!get("org.example.subs").adult);
    }

    #[test]
    fn addon_catalog_entries_lose_their_remote_flags_and_bad_urls() {
        let entries = community_entries();
        // the ftp:// one and the one without /manifest.json are dropped
        assert_eq!(
            entries.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
            [
                "org.example.subs",
                "org.example.radio",
                "org.example.adult-hint"
            ]
        );
        // the fixture's radio listing claims {"official": true, "protected": true}; an
        // entry has no flags at all, and serializing it shows none
        let json = serde_json::to_value(&entries[1]).unwrap();
        assert!(json.get("flags").is_none());
        assert!(entries[2].adult, "behaviorHints.adult");
    }

    #[test]
    fn merging_dedupes_by_id_and_keeps_directory_details() {
        let merged = merge_entries([sa_entries(), community_entries()]);
        let subs: Vec<_> = merged
            .iter()
            .filter(|e| e.key == "org.example.subs")
            .collect();
        assert_eq!(subs.len(), 1);
        assert_eq!(
            subs[0].sources,
            [
                DirectorySource::StremioAddonsNet,
                DirectorySource::StremioCommunity
            ]
        );
        assert_eq!(subs[0].stars, Some(120));
        assert!(merged.iter().any(|e| e.key == "org.example.radio"));

        // community first: the stremio-addons.net details still land on the entry
        let merged = merge_entries([community_entries(), sa_entries()]);
        let subs = merged.iter().find(|e| e.key == "org.example.subs").unwrap();
        assert_eq!(subs.stars, Some(120));
        assert_eq!(subs.slug.as_deref(), Some("example-subs"));
    }

    #[test]
    fn adult_entries_are_hidden_unless_shown_and_p2p_only_when_hidden() {
        let merged = merge_entries([sa_entries(), community_entries()]);
        let all = DirectoryQuery {
            sort: DirectorySort::Name,
            ..Default::default()
        };
        let default = filter_entries(&merged, &all, DirectoryVisibility::default());
        assert!(default.iter().all(|e| !e.adult));
        assert!(default.iter().any(|e| e.p2p), "P2P is flagged, not hidden");

        let shown = filter_entries(
            &merged,
            &all,
            DirectoryVisibility {
                show_adult: true,
                hide_p2p: true,
            },
        );
        assert!(shown.iter().any(|e| e.adult));
        assert!(shown.iter().all(|e| !e.p2p));

        // the nsfw category selects nothing while adult content is hidden
        let nsfw = DirectoryQuery {
            category: Some(NSFW_CATEGORY.into()),
            ..Default::default()
        };
        assert!(filter_entries(&merged, &nsfw, DirectoryVisibility::default()).is_empty());
    }

    #[test]
    fn facets_search_and_sorts() {
        let merged = merge_entries([sa_entries(), community_entries()]);
        let visible = DirectoryVisibility::default();
        let query = |q: DirectoryQuery| keys(&filter_entries(&merged, &q, visible));

        assert_eq!(
            query(DirectoryQuery {
                resources: vec!["subtitles".into()],
                ..Default::default()
            }),
            ["org.example.subs"]
        );
        // catalog: from the catalogs list, not the resources list
        assert_eq!(
            query(DirectoryQuery {
                resources: vec!["catalog".into()],
                sort: DirectorySort::Name,
                ..Default::default()
            }),
            ["org.example.meta", "org.example.radio"]
        );
        // a type declared only on an object-form resource counts
        assert_eq!(
            query(DirectoryQuery {
                types: vec!["anime".into()],
                ..Default::default()
            }),
            ["org.example.streams"]
        );
        assert_eq!(
            query(DirectoryQuery {
                category: Some("subtitles".into()),
                ..Default::default()
            }),
            ["org.example.subs"]
        );
        // search: name, id, description or slug, case-insensitive
        assert_eq!(
            query(DirectoryQuery {
                search: Some("  RADIO ".into()),
                ..Default::default()
            }),
            ["org.example.radio"]
        );
        assert_eq!(
            query(DirectoryQuery {
                search: Some("example-subs".into()),
                ..Default::default()
            }),
            ["org.example.subs"]
        );
        // stars: most first, unstarred last
        let stars = query(DirectoryQuery::default());
        assert_eq!(
            stars.first().map(String::as_str),
            Some("org.example.streams")
        );
        assert_eq!(stars.last().map(String::as_str), Some("org.example.radio"));
        // newest listing first
        let newest = query(DirectoryQuery {
            sort: DirectorySort::Newest,
            ..Default::default()
        });
        assert_eq!(newest.first().map(String::as_str), Some("org.example.subs"));
    }

    #[test]
    fn slugs_are_checked_before_they_reach_a_url() {
        assert!(is_slug("example-subs"));
        assert!(!is_slug("../admin"));
        assert!(!is_slug("a/b"));
        assert!(!is_slug(""));
        assert_eq!(
            stremio_addons_net_page("example-subs").unwrap().as_str(),
            "https://stremio-addons.net/addons/example-subs"
        );
        assert!(stremio_addons_net_page("a?b").is_none());
    }

    #[test]
    fn categories_fall_back_without_nsfw() {
        assert!(
            fallback_categories()
                .iter()
                .all(|c| c.slug != NSFW_CATEGORY)
        );
        assert!(fallback_categories().iter().any(|c| c.slug == "subtitles"));
    }
}
