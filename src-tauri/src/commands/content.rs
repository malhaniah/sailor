use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;
use zeiler_addon::{
    AddonStreams, AddonSubtitles, Descriptor, ExtraValue, MetaLookup, MetaPreview, ResourcePath,
    aggregate_streams, aggregate_subtitles, build_resource_url, find_meta, is_adult_manifest,
    is_adult_meta, is_adult_text,
};

use crate::catalog_cache::CatalogCache;
use crate::error::IpcError;
use crate::state::AppState;

const META_TIMEOUT: Duration = Duration::from_secs(15);
const STREAMS_TIMEOUT: Duration = Duration::from_secs(25);
/// Per addon: subtitles are asked while the video opens and must not hold it up.
const SUBTITLES_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogArgs {
    pub transport_url: String,
    pub r#type: String,
    pub id: String,
    #[serde(default)]
    pub extra: Vec<ExtraValue>,
    /// Skip the cache (a pull to refresh).
    #[serde(default)]
    pub force: bool,
}

/// A catalog page as the UI gets it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    pub metas: Vec<MetaPreview>,
    /// How many titles the addon sent, before adult ones were taken out: the next
    /// page's `skip` counts these, so paging stays aligned with the addon's.
    pub fetched: usize,
}

/// Load one catalog page from one addon. Only from an addon in effect (installed or a
/// switched-on built-in), for a catalog it declares, with extras it declares; adult
/// catalogs, options and titles only while `showAdult` is on. Pages are cached for a
/// while ([`CatalogCache`]).
#[tauri::command]
pub async fn catalog_load(
    state: State<'_, AppState>,
    cache: State<'_, CatalogCache>,
    args: CatalogArgs,
) -> Result<CatalogPage, IpcError> {
    let addons = state.effective_addons().await;
    let show_adult = state.settings.read().await.show_adult;
    let mut path = ResourcePath::new("catalog", args.r#type, args.id);
    path.extra = args.extra;
    let addon = find_catalog_addon(&addons, &args.transport_url, &path, show_adult)?;
    let url = build_resource_url(&addon.transport_url, &path)?;

    let now = Instant::now();
    let cached = if args.force {
        None
    } else {
        cache.get(&url, now)
    };
    let mut response = match cached {
        Some(page) => page,
        None => {
            let page = state.client.catalog(&addon.transport_url, &path).await?;
            cache.put(url, page.clone(), now);
            page
        }
    };
    let fetched = response.metas.len();
    if !show_adult {
        response.metas.retain(|meta| !is_adult_meta(meta));
    }
    Ok(CatalogPage {
        metas: response.metas,
        fetched,
    })
}

/// The addon a catalog request may go to: one in effect with this transport URL that
/// declares this catalog and these extras (`is_resource_supported`). While adult
/// content is hidden, not an adult addon (`is_adult_manifest`), nor an adult option.
fn find_catalog_addon<'a>(
    addons: &'a [Descriptor],
    transport_url: &str,
    path: &ResourcePath,
    show_adult: bool,
) -> Result<&'a Descriptor, IpcError> {
    let addon = addons
        .iter()
        .find(|d| d.transport_url.as_str() == transport_url)
        .ok_or_else(|| IpcError::InvalidInput("That addon is not installed.".into()))?;
    if !addon.manifest.is_resource_supported(path) {
        return Err(IpcError::InvalidInput(format!(
            "{} has no {} catalog \"{}\" taking these filters.",
            addon.manifest.name, path.r#type, path.id
        )));
    }
    if !show_adult {
        let catalog = addon
            .manifest
            .catalogs
            .iter()
            .find(|c| c.r#type == path.r#type && c.id == path.id);
        let adult_catalog = is_adult_manifest(&addon.manifest)
            || catalog.is_some_and(|c| is_adult_text(c.display_name()));
        // only values picked from a list (genres, …): free text such as a search for
        // "Yuri on Ice" is not a rating, and the titles it finds are filtered anyway
        let options = catalog.map(|c| c.filter_props()).unwrap_or_default();
        let adult_option = path
            .extra
            .iter()
            .any(|e| options.iter().any(|p| p.name == e.name) && is_adult_text(&e.value));
        if adult_catalog || adult_option {
            return Err(IpcError::InvalidInput(
                "Adult content is hidden. You can show it in Settings → Content.".into(),
            ));
        }
    }
    Ok(addon)
}

/// Fetch the meta object for an item: first success across supporting addons, in
/// install order. An anime id (`kitsu:`, `mal:`, `anilist:`, `anidb:`) that nobody
/// answers under its catalog type is retried as series, movie and anime, since anime
/// addons file titles under types other than the catalog's. A missing meta is not an
/// error — the UI falls back to the catalog preview — so the outcome says who was asked
/// and what failed. Streams a built-in embeds in its videos are dropped.
#[tauri::command]
pub async fn meta_get(
    state: State<'_, AppState>,
    r#type: String,
    id: String,
) -> Result<MetaLookup, IpcError> {
    let addons = state.effective_addons().await;
    let lookup = find_meta(&state.client, &addons, &r#type, &id, META_TIMEOUT).await;
    Ok(crate::builtin::without_builtin_streams(lookup, &addons))
}

/// Fan out a stream request to every supporting addon; per-addon results, in install order.
#[tauri::command]
pub async fn streams_get(
    state: State<'_, AppState>,
    r#type: String,
    id: String,
) -> Result<Vec<AddonStreams>, IpcError> {
    let addons = state.effective_addons().await;
    Ok(aggregate_streams(&state.client, &addons, &r#type, &id, STREAMS_TIMEOUT).await)
}

/// Fan out a subtitles request to every addon that serves subtitles for this type and
/// video id (the built-in OpenSubtitles v3 when it is on); per-addon results, in
/// install order. What the stream says about its file — the OpenSubtitles hash, the
/// size and the file name — goes along, so addons can match the exact release; a
/// value that is not well-formed is left out rather than refusing the request.
#[tauri::command]
pub async fn subtitles_get(
    state: State<'_, AppState>,
    r#type: String,
    id: String,
    video_hash: Option<String>,
    video_size: Option<u64>,
    filename: Option<String>,
) -> Result<Vec<AddonSubtitles>, IpcError> {
    let extra = subtitle_extra(video_hash, video_size, filename);
    let addons = state.effective_addons().await;
    Ok(aggregate_subtitles(
        &state.client,
        &addons,
        &r#type,
        &id,
        &extra,
        SUBTITLES_TIMEOUT,
    )
    .await)
}

/// The `videoHash`, `videoSize`, `filename` extras, in Stremio's order, each only when
/// it is well-formed: a 16-digit hex OpenSubtitles hash, a size above zero, and a file
/// name (no path) without control characters.
fn subtitle_extra(
    video_hash: Option<String>,
    video_size: Option<u64>,
    filename: Option<String>,
) -> Vec<ExtraValue> {
    let extra = |name: &str, value: String| ExtraValue {
        name: name.to_string(),
        value,
    };
    let hash = video_hash
        .filter(|h| (1..=16).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_hexdigit()));
    let size = video_size.filter(|s| *s > 0);
    let filename = filename.filter(|f| {
        (1..=255).contains(&f.len()) && !f.contains(['/', '\\']) && !f.chars().any(char::is_control)
    });
    [
        hash.map(|h| extra("videoHash", h)),
        size.map(|s| extra("videoSize", s.to_string())),
        filename.map(|f| extra("filename", f)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CINEMETA: &str = "https://v3-cinemeta.strem.io/manifest.json";
    const KITSU: &str = "https://anime-kitsu.strem.fun/manifest.json";
    const ADULT: &str = "https://adult.example/manifest.json";

    fn addons() -> Vec<Descriptor> {
        let adult: Descriptor = serde_json::from_value(serde_json::json!({
            "transportUrl": ADULT,
            "manifest": {
                "id": "x.adult", "name": "Adult", "types": ["movie"], "resources": ["catalog"],
                "catalogs": [{"type": "movie", "id": "top"}],
                "behaviorHints": {"adult": true}
            }
        }))
        .unwrap();
        crate::builtin::with_builtins(vec![adult], &crate::state::Settings::default())
    }

    fn path(r#type: &str, id: &str, extra: &[(&str, &str)]) -> ResourcePath {
        let mut path = ResourcePath::new("catalog", r#type, id);
        for (name, value) in extra {
            path = path.with_extra(*name, *value);
        }
        path
    }

    #[test]
    fn catalog_requests_go_only_to_addons_in_effect_for_declared_catalogs_and_extras() {
        let addons = addons();
        let find = |url: &str, path: &ResourcePath| {
            find_catalog_addon(&addons, url, path, false).map(|d| d.manifest.id.clone())
        };
        assert_eq!(
            find(CINEMETA, &path("movie", "top", &[])).unwrap(),
            "com.linvo.cinemeta"
        );
        assert!(
            find(
                CINEMETA,
                &path("movie", "top", &[("genre", "Drama"), ("skip", "100")])
            )
            .is_ok()
        );
        assert!(find(CINEMETA, &path("movie", "year", &[("genre", "2025")])).is_ok());

        for (url, bad) in [
            (
                "https://evil.example/manifest.json",
                path("movie", "top", &[]),
            ),
            (CINEMETA, path("movie", "nope", &[])),
            (CINEMETA, path("anime", "top", &[])),
            (CINEMETA, path("movie", "top", &[("token", "x")])),
            (CINEMETA, path("movie", "year", &[])),
        ] {
            assert!(
                matches!(find(url, &bad), Err(IpcError::InvalidInput(_))),
                "{url} {bad:?}"
            );
        }
    }

    #[test]
    fn adult_catalogs_and_options_need_show_adult() {
        let addons = addons();
        let hentai = path("anime", "kitsu-anime-popular", &[("genre", "Hentai")]);
        let action = path("anime", "kitsu-anime-popular", &[("genre", "Action")]);
        let flagged = path("movie", "top", &[]);
        assert!(find_catalog_addon(&addons, KITSU, &action, false).is_ok());
        assert!(find_catalog_addon(&addons, KITSU, &hentai, false).is_err());
        assert!(find_catalog_addon(&addons, ADULT, &flagged, false).is_err());
        assert!(find_catalog_addon(&addons, KITSU, &hentai, true).is_ok());
        let search = path("anime", "kitsu-anime-list", &[("search", "yuri on ice")]);
        assert!(
            find_catalog_addon(&addons, KITSU, &search, false).is_ok(),
            "free text"
        );
        assert!(find_catalog_addon(&addons, ADULT, &flagged, true).is_ok());
    }

    #[test]
    fn subtitle_extras_keep_only_well_formed_values() {
        let names = |extra: Vec<ExtraValue>| -> Vec<String> {
            extra
                .into_iter()
                .map(|e| format!("{}={}", e.name, e.value))
                .collect()
        };
        assert_eq!(
            names(subtitle_extra(
                Some("8e245d9679d31e12".into()),
                Some(1_351_620_608),
                Some("Show.S01E01.1080p.WEB.mkv".into())
            )),
            [
                "videoHash=8e245d9679d31e12",
                "videoSize=1351620608",
                "filename=Show.S01E01.1080p.WEB.mkv"
            ]
        );
        assert!(names(subtitle_extra(None, None, None)).is_empty());
        assert!(
            names(subtitle_extra(
                Some("not hex".into()),
                Some(0),
                Some("../../etc/passwd".into())
            ))
            .is_empty()
        );
        assert!(names(subtitle_extra(None, None, Some("a\nb.mkv".into()))).is_empty());
        assert_eq!(
            names(subtitle_extra(Some("0123456789abcdef0".into()), None, None)).len(),
            0,
            "too long for an OpenSubtitles hash"
        );
    }
}
