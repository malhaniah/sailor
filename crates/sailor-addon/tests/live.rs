//! Live integration tests against real public addons. Ignored by default (network);
//! run explicitly with: cargo test -p sailor-addon --test live -- --ignored

use sailor_addon::{AddonClient, ResourcePath};
use url::Url;

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
