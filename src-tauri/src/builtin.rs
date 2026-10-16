//! Built-in addons: manifest snapshots compiled into the app, so startup never depends
//! on the network.
//!
//! Policy (CONTRIBUTING.md): Zeiler never seeds a stream addon. A built-in may only be a
//! metadata or subtitles addon, and each one is switchable in Settings → Content. The
//! tests at the bottom of this file enforce "no `stream` resource" for every entry.
//! Each built-in also keeps only the resources of its role, so its switch covers all of
//! that traffic and nothing else.
//!
//! The snapshots are refreshed by `scripts/refresh-builtin-manifests.mjs`, which keeps
//! its own copy of this table (a test checks the two agree).

use std::sync::LazyLock;

use zeiler_addon::{Descriptor, DescriptorFlags, Manifest, MetaLookup};

use crate::state::Settings;

/// One built-in addon. Snapshots live under `src-tauri/builtin/`.
pub(crate) struct Builtin {
    /// The addon's real transport URL.
    pub transport_url: &'static str,
    /// The snapshot's file name under `src-tauri/builtin/` (what the refresh script
    /// rewrites; a test checks it is the file `manifest_json` includes).
    pub snapshot_file: &'static str,
    /// The compiled-in `manifest.json` snapshot.
    pub manifest_json: &'static str,
    /// The resources Zeiler uses from it (its role). Any other resource the snapshot
    /// declares is left out of the descriptor; the snapshot file itself stays as served.
    pub resources: &'static [&'static str],
    /// Whether this built-in is switched on.
    pub enabled: fn(&Settings) -> bool,
}

/// Every built-in, in the order they come before the user's addons. Metadata addons
/// come first, so meta lookups ask Cinemeta, then Kitsu, then the user's addons (their
/// id prefixes don't overlap: `tt` against `kitsu`/`mal`/`anilist`/`anidb`).
pub(crate) const BUILTINS: &[Builtin] = &[
    Builtin {
        // Cinemeta (Stremio): catalogs, details and episode lists for IMDb titles.
        transport_url: "https://v3-cinemeta.strem.io/manifest.json",
        snapshot_file: "cinemeta-manifest.json",
        manifest_json: include_str!("../builtin/cinemeta-manifest.json"),
        resources: &["catalog", "meta", "addon_catalog"],
        enabled: |settings| settings.builtin_cinemeta,
    },
    Builtin {
        // Anime Kitsu (community): anime catalogs and details for Kitsu, MyAnimeList,
        // AniList and AniDB ids. Its manifest also declares subtitles, which redirect to
        // OpenSubtitles v3; they are left out, so the OpenSubtitles switch covers every
        // subtitle request a built-in makes.
        transport_url: "https://anime-kitsu.strem.fun/manifest.json",
        snapshot_file: "kitsu-manifest.json",
        manifest_json: include_str!("../builtin/kitsu-manifest.json"),
        resources: &["catalog", "meta"],
        enabled: |settings| settings.builtin_kitsu,
    },
    Builtin {
        // OpenSubtitles v3 (Stremio): subtitles for IMDb titles.
        transport_url: "https://opensubtitles-v3.strem.io/manifest.json",
        snapshot_file: "opensubtitles-v3-manifest.json",
        manifest_json: include_str!("../builtin/opensubtitles-v3-manifest.json"),
        resources: &["subtitles"],
        enabled: |settings| settings.builtin_opensubtitles,
    },
];

impl Builtin {
    /// The descriptor for this built-in, trimmed to its `resources` and flagged
    /// `official` + `protected` so the UI shows it as built-in (switched off in Settings
    /// rather than removed).
    fn descriptor(&self) -> Descriptor {
        let mut manifest: Manifest = serde_json::from_str(self.manifest_json).unwrap_or_else(|e| {
            panic!(
                "built-in snapshot {} is invalid (tests check it): {e}",
                self.snapshot_file
            )
        });
        manifest
            .resources
            .retain(|resource| self.resources.contains(&resource.name()));
        // catalogs are routed by these lists, not by `resources`
        if !self.resources.contains(&"catalog") {
            manifest.catalogs.clear();
        }
        if !self.resources.contains(&"addon_catalog") {
            manifest.addon_catalogs.clear();
        }
        // a snapshot's artwork over plain HTTP (OpenSubtitles' logo) is fetched over HTTPS
        for image in [&mut manifest.logo, &mut manifest.background]
            .into_iter()
            .flatten()
        {
            if image.scheme() == "http" {
                let _ = image.set_scheme("https");
            }
        }
        Descriptor {
            manifest,
            transport_url: url::Url::parse(self.transport_url).expect("valid built-in URL"),
            flags: DescriptorFlags {
                official: true,
                protected: true,
            },
        }
    }
}

/// Parsed once; `BUILTINS[i]` pairs with `DESCRIPTORS[i]`.
static DESCRIPTORS: LazyLock<Vec<Descriptor>> =
    LazyLock::new(|| BUILTINS.iter().map(Builtin::descriptor).collect());

/// The addons in effect: enabled built-ins first (in table order), then the user's
/// addons in install order. A built-in the user also installed is left out, so their
/// copy is used in their order: the same transport URL, or the same manifest id as the
/// snapshot (a configured install, or the same addon from another host).
pub(crate) fn with_builtins(user: Vec<Descriptor>, settings: &Settings) -> Vec<Descriptor> {
    let mut all = Vec::with_capacity(BUILTINS.len() + user.len());
    for (builtin, descriptor) in BUILTINS.iter().zip(DESCRIPTORS.iter()) {
        let user_has_it = user.iter().any(|d| {
            d.transport_url.as_str() == builtin.transport_url
                || d.manifest.id == descriptor.manifest.id
        });
        if (builtin.enabled)(settings) && !user_has_it {
            all.push(descriptor.clone());
        }
    }
    all.extend(user);
    all
}

/// The built-ins as account sync knows them: never imported from, or pushed into, a
/// Stremio account's addon collection (by transport URL or manifest id).
pub(crate) fn builtins_for_sync() -> zeiler_sync::sync::Builtins {
    zeiler_sync::sync::Builtins {
        transport_urls: BUILTINS
            .iter()
            .map(|b| b.transport_url.to_string())
            .collect(),
        manifest_ids: DESCRIPTORS.iter().map(|d| d.manifest.id.clone()).collect(),
    }
}

/// A meta answered by a built-in, without the streams a meta may embed in its videos
/// (`videos[].streams`, which the Play picker offers directly): the manifest tests below
/// can't see those, and a built-in never serves streams. A user's own install of the
/// same addon (not `protected`) keeps them, like any addon they chose.
pub(crate) fn without_builtin_streams(mut lookup: MetaLookup, addons: &[Descriptor]) -> MetaLookup {
    let from_builtin = lookup.source.as_ref().is_some_and(|source| {
        BUILTINS.iter().any(|b| b.transport_url == source.as_str())
            && addons
                .iter()
                .any(|d| &d.transport_url == source && d.flags.protected)
    });
    if from_builtin && let Some(meta) = lookup.meta.as_mut() {
        for video in &mut meta.videos {
            video.streams.clear();
        }
    }
    lookup
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use zeiler_addon::ResourcePath;

    use super::*;

    /// Resource names a manifest declares, read from the raw JSON so that an entry the
    /// lenient parser would skip still counts.
    fn raw_resource_names(manifest_json: &str) -> Vec<String> {
        let value: serde_json::Value = serde_json::from_str(manifest_json).unwrap();
        value["resources"]
            .as_array()
            .map(|resources| {
                resources
                    .iter()
                    .filter_map(|r| r.as_str().or_else(|| r["name"].as_str()))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn builtin(manifest_id: &str) -> (&'static Builtin, &'static Descriptor) {
        BUILTINS
            .iter()
            .zip(DESCRIPTORS.iter())
            .find(|(_, d)| d.manifest.id == manifest_id)
            .unwrap_or_else(|| panic!("no built-in {manifest_id}"))
    }

    fn urls(addons: &[Descriptor]) -> Vec<&str> {
        addons.iter().map(|d| d.transport_url.as_str()).collect()
    }

    /// The content-neutrality guard: no built-in may declare a `stream` resource. Checked
    /// by resource name, not `is_resource_supported` (which says false whenever a sample
    /// id misses `idPrefixes`, so a stream addon for other ids would slip through).
    #[test]
    fn every_builtin_snapshot_parses_and_serves_no_streams() {
        assert!(!BUILTINS.is_empty());
        for (builtin, descriptor) in BUILTINS.iter().zip(DESCRIPTORS.iter()) {
            let url = builtin.transport_url;
            assert!(url.ends_with("/manifest.json"), "{url}: transport URL");
            assert!(url.starts_with("https://"), "{url}: built-ins use HTTPS");
            assert!(
                descriptor.flags.official && descriptor.flags.protected,
                "{url}"
            );
            assert!(!descriptor.manifest.resources.is_empty(), "{url}: parsed");

            let raw = raw_resource_names(builtin.manifest_json);
            assert!(!raw.is_empty(), "{url}: snapshot declares resources");
            assert!(
                !raw.iter().any(|name| name == "stream"),
                "{url}: built-ins must never serve streams"
            );
            assert!(
                !descriptor
                    .manifest
                    .resources
                    .iter()
                    .any(|r| r.name() == "stream"),
                "{url}: built-ins must never serve streams"
            );
        }
    }

    /// The policy as an allow-list: metadata (catalogs, details, addon catalogs) or
    /// subtitles, so a resource kind added to the protocol later can't slip in either.
    #[test]
    fn every_builtin_serves_only_metadata_or_subtitles() {
        const ALLOWED: [&str; 4] = ["catalog", "meta", "addon_catalog", "subtitles"];
        for builtin in BUILTINS {
            for name in raw_resource_names(builtin.manifest_json) {
                assert!(
                    ALLOWED.contains(&name.as_str()),
                    "{}: declares `{name}`",
                    builtin.transport_url
                );
            }
        }
    }

    /// Each built-in keeps exactly its role: resources its snapshot declares and the
    /// policy allows, and nothing else.
    #[test]
    fn each_builtin_keeps_only_the_resources_of_its_role() {
        const ALLOWED: [&str; 4] = ["catalog", "meta", "addon_catalog", "subtitles"];
        for (builtin, descriptor) in BUILTINS.iter().zip(DESCRIPTORS.iter()) {
            let url = builtin.transport_url;
            let declared = raw_resource_names(builtin.manifest_json);
            for role in builtin.resources {
                assert!(ALLOWED.contains(role), "{url}: role `{role}`");
                assert!(
                    declared.iter().any(|d| d == role),
                    "{url}: declares `{role}`"
                );
            }
            let kept: Vec<_> = descriptor
                .manifest
                .resources
                .iter()
                .map(|r| r.name())
                .collect();
            assert!(!kept.is_empty(), "{url}");
            assert!(
                kept.iter().all(|name| builtin.resources.contains(name)),
                "{url}: kept {kept:?}"
            );
            let manifest = &descriptor.manifest;
            if !builtin.resources.contains(&"catalog") {
                assert!(manifest.catalogs.is_empty(), "{url}: catalogs off-role");
            }
            if !builtin.resources.contains(&"addon_catalog") {
                assert!(
                    manifest.addon_catalogs.is_empty(),
                    "{url}: addon catalogs off-role"
                );
            }
        }
    }

    #[test]
    fn builtin_artwork_is_fetched_over_https() {
        for descriptor in DESCRIPTORS.iter() {
            let manifest = &descriptor.manifest;
            for image in [&manifest.logo, &manifest.background].into_iter().flatten() {
                assert_eq!(image.scheme(), "https", "{image}");
            }
        }
        let (_, opensubtitles) = builtin("org.stremio.opensubtitlesv3");
        assert_eq!(
            opensubtitles.manifest.logo.as_ref().map(url::Url::as_str),
            Some("https://www.strem.io/images/addons/opensubtitles-logo.png"),
            "the snapshot's http logo, upgraded"
        );
    }

    /// Settings → Content has a switch for every built-in (src/views/Settings.tsx keeps
    /// a copy of the table), so none can ship without one.
    #[test]
    fn settings_has_a_switch_for_every_builtin() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let settings = std::fs::read_to_string(root.join("../src/views/Settings.tsx"))
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for descriptor in DESCRIPTORS.iter() {
            let entry = format!(
                "transportUrl: \"{}\", manifestId: \"{}\"",
                descriptor.transport_url, descriptor.manifest.id
            );
            assert!(settings.contains(&entry), "Settings.tsx lists {entry}");
        }
    }

    /// Kitsu's manifest declares subtitles, which redirect to OpenSubtitles v3; the
    /// built-in leaves them out, so switching OpenSubtitles off stops every built-in
    /// subtitle request.
    #[test]
    fn kitsu_as_a_builtin_is_not_asked_for_subtitles() {
        let (builtin, kitsu) = builtin("community.anime.kitsu");
        assert!(raw_resource_names(builtin.manifest_json).contains(&"subtitles".to_owned()));
        let subtitles = ResourcePath::new("subtitles", "series", "kitsu:1:1");
        assert!(!kitsu.manifest.is_resource_supported(&subtitles));
        let meta = ResourcePath::new("meta", "series", "kitsu:1");
        assert!(kitsu.manifest.is_resource_supported(&meta));
    }

    fn lookup_with_embedded_streams(source: &str) -> MetaLookup {
        let meta = serde_json::from_value(serde_json::json!({
            "id": "kitsu:1", "type": "series", "name": "Cowboy Bebop",
            "videos": [{ "id": "kitsu:1:1", "title": "Episode 1", "season": 1, "episode": 1,
                         "streams": [{ "url": "https://example.invalid/e1.mp4" }] }]
        }))
        .unwrap();
        MetaLookup {
            meta: Some(meta),
            asked: vec!["Anime Kitsu".into()],
            errors: Vec::new(),
            source: Some(url::Url::parse(source).unwrap()),
        }
    }

    fn embedded_streams(lookup: &MetaLookup) -> usize {
        let meta = lookup.meta.as_ref().unwrap();
        meta.videos.iter().map(|v| v.streams.len()).sum()
    }

    #[test]
    fn a_builtin_meta_never_carries_streams() {
        let (kitsu_builtin, _) = builtin("community.anime.kitsu");
        let addons = with_builtins(Vec::new(), &Settings::default());
        let lookup = lookup_with_embedded_streams(kitsu_builtin.transport_url);
        assert_eq!(embedded_streams(&lookup), 1, "parsed");
        assert_eq!(
            embedded_streams(&without_builtin_streams(lookup, &addons)),
            0
        );

        // the user's own install at the same URL is their choice, streams and all
        let own = vec![user_addon(
            kitsu_builtin.transport_url,
            "community.anime.kitsu",
        )];
        let addons = with_builtins(own, &Settings::default());
        let lookup = lookup_with_embedded_streams(kitsu_builtin.transport_url);
        assert_eq!(
            embedded_streams(&without_builtin_streams(lookup, &addons)),
            1
        );

        // and so is any other addon
        let other = vec![user_addon(
            "https://example.invalid/manifest.json",
            "org.example.addon",
        )];
        let addons = with_builtins(other, &Settings::default());
        let lookup = lookup_with_embedded_streams("https://example.invalid/manifest.json");
        assert_eq!(
            embedded_streams(&without_builtin_streams(lookup, &addons)),
            1
        );
    }

    #[test]
    fn transport_urls_and_manifest_ids_are_unique() {
        let mut urls: Vec<_> = BUILTINS.iter().map(|b| b.transport_url).collect();
        urls.sort_unstable();
        urls.dedup();
        assert_eq!(urls.len(), BUILTINS.len());

        let mut ids: Vec<_> = DESCRIPTORS.iter().map(|d| d.manifest.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), BUILTINS.len());
    }

    /// `snapshot_file` names the file `manifest_json` includes, every snapshot in
    /// `src-tauri/builtin/` is in the table, and the refresh script knows every entry.
    #[test]
    fn the_snapshot_folder_and_the_refresh_script_match_the_table() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for builtin in BUILTINS {
            let on_disk = std::fs::read_to_string(root.join("builtin").join(builtin.snapshot_file))
                .unwrap_or_else(|e| panic!("{}: {e}", builtin.snapshot_file));
            assert_eq!(on_disk, builtin.manifest_json, "{}", builtin.snapshot_file);
        }

        // only snapshots: an OS file such as .DS_Store or desktop.ini is not one
        let mut files: Vec<String> = std::fs::read_dir(root.join("builtin"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".json"))
            .collect();
        files.sort_unstable();
        let mut listed: Vec<String> = BUILTINS
            .iter()
            .map(|b| b.snapshot_file.to_owned())
            .collect();
        listed.sort_unstable();
        assert_eq!(files, listed, "every snapshot file is a built-in");

        // whitespace-insensitive, so the formatter may wrap an entry
        let script = std::fs::read_to_string(root.join("../scripts/refresh-builtin-manifests.mjs"))
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for builtin in BUILTINS {
            let entry = format!(
                "url: \"{}\", file: \"{}\"",
                builtin.transport_url, builtin.snapshot_file
            );
            assert!(
                script.contains(&entry),
                "the refresh script lists {} as {}",
                builtin.transport_url,
                builtin.snapshot_file
            );
        }
    }

    #[test]
    fn cinemeta_serves_meta_for_imdb_ids() {
        let (_, cinemeta) = builtin("com.linvo.cinemeta");
        assert!(!cinemeta.manifest.catalogs.is_empty());
        let meta = ResourcePath::new("meta", "series", "tt0108778");
        assert!(cinemeta.manifest.is_resource_supported(&meta));
        let anime = ResourcePath::new("meta", "series", "kitsu:1");
        assert!(!cinemeta.manifest.is_resource_supported(&anime));
    }

    #[test]
    fn kitsu_serves_anime_catalogs_and_meta_for_anime_ids_under_any_anime_type() {
        let (_, kitsu) = builtin("community.anime.kitsu");
        assert!(
            kitsu
                .manifest
                .catalogs
                .iter()
                .any(|c| c.r#type == "anime" && c.is_browsable()),
            "fills the Anime room"
        );
        for id in ["kitsu:1", "mal:1", "anilist:1", "anidb:1"] {
            for meta_type in ["series", "movie", "anime"] {
                let meta = ResourcePath::new("meta", meta_type, id);
                assert!(
                    kitsu.manifest.is_resource_supported(&meta),
                    "{meta_type} {id}"
                );
            }
        }
        let imdb = ResourcePath::new("meta", "series", "tt0108778");
        assert!(
            !kitsu.manifest.is_resource_supported(&imdb),
            "leaves IMDb ids to Cinemeta"
        );
    }

    #[test]
    fn opensubtitles_serves_subtitles_for_imdb_ids_and_nothing_else() {
        let (_, opensubtitles) = builtin("org.stremio.opensubtitlesv3");
        assert!(opensubtitles.manifest.catalogs.is_empty());
        assert!(opensubtitles.manifest.addon_catalogs.is_empty());
        for (r#type, id) in [("movie", "tt0111161"), ("series", "tt0108778:1:1")] {
            let subtitles = ResourcePath::new("subtitles", r#type, id);
            assert!(opensubtitles.manifest.is_resource_supported(&subtitles));
            let meta = ResourcePath::new("meta", r#type, id);
            assert!(!opensubtitles.manifest.is_resource_supported(&meta));
        }
    }

    /// A user addon at `url` with its own manifest id.
    fn user_addon(url: &str, manifest_id: &str) -> Descriptor {
        let mut descriptor = DESCRIPTORS[0].clone();
        descriptor.manifest.id = manifest_id.to_owned();
        descriptor.transport_url = url::Url::parse(url).unwrap();
        descriptor.flags = DescriptorFlags::default();
        descriptor
    }

    #[test]
    fn builtins_come_first_in_table_order() {
        let user = vec![user_addon(
            "https://example.invalid/manifest.json",
            "org.example.addon",
        )];
        let all = with_builtins(user, &Settings::default());
        let mut expected: Vec<&str> = BUILTINS.iter().map(|b| b.transport_url).collect();
        expected.push("https://example.invalid/manifest.json");
        assert_eq!(urls(&all), expected);
        assert!(all[..BUILTINS.len()].iter().all(|d| d.flags.protected));
        assert!(!all[BUILTINS.len()].flags.protected);
    }

    #[test]
    fn each_toggle_switches_off_only_its_own_builtin() {
        type SwitchOff = fn(&mut Settings);
        let toggles: [(&str, SwitchOff); 3] = [
            ("com.linvo.cinemeta", |s| s.builtin_cinemeta = false),
            ("community.anime.kitsu", |s| s.builtin_kitsu = false),
            ("org.stremio.opensubtitlesv3", |s| {
                s.builtin_opensubtitles = false
            }),
        ];
        assert_eq!(toggles.len(), BUILTINS.len(), "every built-in has a toggle");
        for (id, switch_off) in toggles {
            let mut settings = Settings::default();
            switch_off(&mut settings);
            let all = with_builtins(Vec::new(), &settings);
            assert_eq!(all.len(), BUILTINS.len() - 1, "{id}");
            assert!(all.iter().all(|d| d.manifest.id != id), "{id} is off");
        }

        let none = Settings {
            builtin_cinemeta: false,
            builtin_kitsu: false,
            builtin_opensubtitles: false,
            ..Settings::default()
        };
        assert!(with_builtins(Vec::new(), &none).is_empty());
    }

    #[test]
    fn a_user_install_of_the_same_url_wins() {
        let (opensubtitles_builtin, opensubtitles) = builtin("org.stremio.opensubtitlesv3");
        let user = vec![user_addon(
            opensubtitles_builtin.transport_url,
            "org.example.fork",
        )];
        let all = with_builtins(user, &Settings::default());
        let copies: Vec<_> = all
            .iter()
            .filter(|d| d.transport_url == opensubtitles.transport_url)
            .collect();
        assert_eq!(copies.len(), 1);
        assert!(
            !copies[0].flags.protected,
            "the user's copy, not the built-in"
        );
        assert_eq!(all.len(), BUILTINS.len());
    }

    /// Matched by the id in the snapshot, not a guessed one: a configured Kitsu install
    /// (its own URL, the same manifest id) replaces the built-in, in the user's order.
    #[test]
    fn a_user_install_with_the_same_manifest_id_wins() {
        let configured = "https://anime-kitsu.strem.fun/lang=en/manifest.json";
        let user = vec![
            user_addon("https://example.invalid/manifest.json", "org.example.addon"),
            user_addon(configured, "community.anime.kitsu"),
        ];
        let all = with_builtins(user, &Settings::default());
        let kitsus: Vec<_> = all
            .iter()
            .filter(|d| d.manifest.id == "community.anime.kitsu")
            .collect();
        assert_eq!(kitsus.len(), 1);
        assert_eq!(kitsus[0].transport_url.as_str(), configured);
        assert_eq!(
            urls(&all).last(),
            Some(&configured),
            "kept in the user's order"
        );
        assert_eq!(all.len(), BUILTINS.len() + 1);
    }
}
