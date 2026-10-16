//! Live integration tests against real public addons. Ignored by default (network);
//! run explicitly with: cargo test -p zeiler-addon --test live -- --ignored

use std::time::Duration;

use url::Url;
use zeiler_addon::{
    AddonClient, ExtraValue, ResourcePath, aggregate_streams, aggregate_subtitles, fetch_subtitle,
    find_meta, first_meta,
};

fn cinemeta() -> Url {
    Url::parse("https://v3-cinemeta.strem.io/manifest.json").unwrap()
}

#[tokio::test]
#[ignore = "network"]
async fn cinemeta_manifest_parses() {
    let client = AddonClient::new().unwrap();
    let manifest = client.manifest(&cinemeta()).await.unwrap();
    assert_eq!(manifest.id, "com.linvo.cinemeta");
    assert!(manifest.types.iter().any(|t| t == "movie"));
    assert!(!manifest.catalogs.is_empty());
    assert!(manifest.is_resource_supported(&ResourcePath::new("meta", "series", "tt0108778")));
    assert!(!manifest.is_resource_supported(&ResourcePath::new("meta", "series", "kitsu:1")));
}

#[tokio::test]
#[ignore = "network"]
async fn cinemeta_top_catalog_loads() {
    let client = AddonClient::new().unwrap();
    let response = client
        .catalog(&cinemeta(), &ResourcePath::new("catalog", "movie", "top"))
        .await
        .unwrap();
    assert!(
        response.metas.len() >= 20,
        "got {} metas",
        response.metas.len()
    );
    assert!(response.metas.iter().all(|m| m.id.starts_with("tt")));
}

#[tokio::test]
#[ignore = "network"]
async fn cinemeta_paged_genre_catalog_follows_redirects() {
    let client = AddonClient::new().unwrap();
    // This exact shape 307-redirects to cinemeta-catalogs.strem.io.
    let path = ResourcePath::new("catalog", "movie", "top")
        .with_extra("genre", "Action")
        .with_extra("skip", "100");
    let response = client.catalog(&cinemeta(), &path).await.unwrap();
    assert!(!response.metas.is_empty());
}

#[tokio::test]
#[ignore = "network"]
async fn cinemeta_search_catalog() {
    let client = AddonClient::new().unwrap();
    let path = ResourcePath::new("catalog", "movie", "top").with_extra("search", "the matrix");
    let response = client.catalog(&cinemeta(), &path).await.unwrap();
    assert!(
        response
            .metas
            .iter()
            .any(|m| m.name.to_lowercase().contains("matrix"))
    );
}

#[tokio::test]
#[ignore = "network"]
async fn cinemeta_series_meta_has_episode_videos() {
    let client = AddonClient::new().unwrap();
    let response = client
        .meta(
            &cinemeta(),
            &ResourcePath::new("meta", "series", "tt0108778"),
        )
        .await
        .unwrap();
    let meta = response.meta;
    assert_eq!(meta.name, "Friends");
    assert!(meta.videos.len() > 200);
    let pilot = meta
        .videos
        .iter()
        .find(|v| v.id == "tt0108778:1:1")
        .unwrap();
    assert_eq!(pilot.season, Some(1));
    assert_eq!(pilot.episode, Some(1));
    assert!(
        pilot.title.is_some(),
        "Cinemeta `name` alias should populate title"
    );
    // specials exist as season 0 and must parse too
    assert!(meta.videos.iter().any(|v| v.season == Some(0)));
}

#[tokio::test]
#[ignore = "network"]
async fn opensubtitles_manifest_and_subtitles() {
    let client = AddonClient::new().unwrap();
    let transport = Url::parse("https://opensubtitles-v3.strem.io/manifest.json").unwrap();
    let manifest = client.manifest(&transport).await.unwrap();
    // the built-in matches the user's own install by this id (src-tauri/src/builtin.rs)
    assert_eq!(manifest.id, "org.stremio.opensubtitlesv3");
    assert!(manifest.is_resource_supported(&ResourcePath::new(
        "subtitles",
        "series",
        "tt0108778:1:1"
    )));
    let response = client
        .subtitles(
            &transport,
            &ResourcePath::new("subtitles", "series", "tt0108778:1:1"),
        )
        .await
        .unwrap();
    assert!(!response.subtitles.is_empty());
    assert!(response.subtitles.iter().all(|s| !s.url.is_empty()));
}

/// The fan-out with a file's extras, and one subtitle downloaded and normalized the way
/// the player gets it (`player_subtitle_add`).
#[tokio::test]
#[ignore = "network"]
async fn opensubtitles_subtitles_download_and_decode() {
    let client = AddonClient::new().unwrap();
    let transport = Url::parse("https://opensubtitles-v3.strem.io/manifest.json").unwrap();
    let addon = client.descriptor(&transport).await.unwrap();
    let extra = [ExtraValue {
        name: "filename".into(),
        value: "Friends.S01E01.1080p.BluRay.x265.mkv".into(),
    }];
    let groups = aggregate_subtitles(
        &client,
        &[addon],
        "series",
        "tt0108778:1:1",
        &extra,
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].error, None);
    let english = groups[0]
        .subtitles
        .iter()
        .find(|s| s.lang == "eng")
        .expect("an English subtitle");
    let url = Url::parse(&english.url).unwrap();
    let file = fetch_subtitle(&client, &url, None).await.unwrap();
    assert_eq!(file.encoding, "UTF-8", "OpenSubtitles v3 serves UTF-8");
    assert!(
        file.text.contains("-->"),
        "a timed text: {:?}",
        &file.text[..200.min(file.text.len())]
    );
}

fn kitsu() -> Url {
    Url::parse("https://anime-kitsu.strem.fun/manifest.json").unwrap()
}

#[tokio::test]
#[ignore = "network"]
async fn kitsu_manifest_parses_and_serves_anime_catalogs() {
    let client = AddonClient::new().unwrap();
    let manifest = client.manifest(&kitsu()).await.unwrap();
    // the built-in matches the user's own install by this id (src-tauri/src/builtin.rs)
    assert_eq!(manifest.id, "community.anime.kitsu");
    let trending = manifest
        .catalogs
        .iter()
        .find(|c| c.r#type == "anime" && c.is_browsable())
        .expect("a browsable anime catalog");
    let page = client
        .catalog(
            &kitsu(),
            &ResourcePath::new("catalog", "anime", &trending.id),
        )
        .await
        .unwrap();
    assert!(!page.metas.is_empty());
    assert!(page.metas.iter().all(|m| m.id.starts_with("kitsu:")));
}

/// The Detail page asks with the catalog item's type; anime ids are retried under the
/// other types until an addon answers.
#[tokio::test]
#[ignore = "network"]
async fn kitsu_meta_is_found_for_anime_ids() {
    let client = AddonClient::new().unwrap();
    let addons = vec![client.descriptor(&kitsu()).await.unwrap()];
    for id in ["kitsu:1", "mal:1"] {
        let lookup = find_meta(&client, &addons, "anime", id, Duration::from_secs(15)).await;
        let meta = lookup
            .meta
            .unwrap_or_else(|| panic!("{id}: {:?}", lookup.errors));
        assert_eq!(meta.name, "Cowboy Bebop");
        assert!(!meta.videos.is_empty());
    }
}

/// The common "catalog addon + stream addon, no metadata addon" setup: details are
/// unavailable (nobody is asked), yet movie streams still resolve from the catalog id.
#[tokio::test]
#[ignore = "network"]
async fn catalog_only_setup_has_no_meta_but_movie_streams_work() {
    let client = AddonClient::new().unwrap();
    let catalogs = client
        .descriptor(
            &Url::parse(
                "https://7a82163c306e-stremio-netflix-catalog-addon.baby-beamup.club/manifest.json",
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let streams_addon = client
        .descriptor(&Url::parse("https://torrentio.strem.fun/manifest.json").unwrap())
        .await
        .unwrap();
    let addons = vec![catalogs.clone(), streams_addon];

    let page = client
        .catalog(
            &catalogs.transport_url,
            &ResourcePath::new("catalog", "movie", &catalogs.manifest.catalogs[0].id),
        )
        .await
        .unwrap();
    let item = &page.metas[0];
    assert!(item.id.starts_with("tt"));

    let lookup = first_meta(&client, &addons, "movie", &item.id, Duration::from_secs(15)).await;
    assert!(lookup.meta.is_none());
    assert!(lookup.asked.is_empty(), "no installed addon serves meta");

    let groups =
        aggregate_streams(&client, &addons, "movie", &item.id, Duration::from_secs(25)).await;
    assert_eq!(groups.len(), 1, "only the stream addon is asked");
    assert!(
        groups[0].error.is_none(),
        "stream addon failed: {:?}",
        groups[0].error
    );
}
