//! Built-in addons: manifest snapshots compiled into the app, so startup never depends
//! on the network.
//!
//! Policy: Zeiler never seeds a stream addon. A built-in may only be a metadata or
//! subtitles addon (CONTRIBUTING.md currently allows metadata only), and each one is
//! switchable in Settings. The test at the bottom of this file enforces "no `stream`
//! resource" for every entry.

use std::sync::LazyLock;

use zeiler_addon::{Descriptor, DescriptorFlags};

use crate::state::Settings;

/// One built-in addon. Snapshots live under `src-tauri/builtin/`.
pub(crate) struct Builtin {
    /// The addon's real transport URL. A user install of the same URL wins over the
    /// built-in copy.
    pub transport_url: &'static str,
    /// The compiled-in `manifest.json` snapshot.
    pub manifest_json: &'static str,
    /// Whether this built-in is switched on.
    pub enabled: fn(&Settings) -> bool,
}

/// Every built-in, in the order they come before the user's addons.
pub(crate) const BUILTINS: &[Builtin] = &[Builtin {
    // Cinemeta: catalogs, details and episode lists. Metadata only.
    transport_url: "https://v3-cinemeta.strem.io/manifest.json",
    manifest_json: include_str!("../builtin/cinemeta-manifest.json"),
    enabled: |settings| settings.builtin_cinemeta,
}];

impl Builtin {
    /// The descriptor for this built-in, flagged `official` + `protected` so the UI
    /// shows it as built-in (switched off in Settings rather than removed).
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            manifest: serde_json::from_str(self.manifest_json)
                .expect("bundled manifest snapshots are valid (checked by tests)"),
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

/// The addons in effect: enabled built-ins first (in table order, each skipped when the
/// user installed the same transport URL), then the user's addons in install order.
pub(crate) fn with_builtins(user: Vec<Descriptor>, settings: &Settings) -> Vec<Descriptor> {
    let mut all = Vec::with_capacity(BUILTINS.len() + user.len());
    for (builtin, descriptor) in BUILTINS.iter().zip(DESCRIPTORS.iter()) {
        let user_has_it = user
            .iter()
            .any(|d| d.transport_url.as_str() == builtin.transport_url);
        if (builtin.enabled)(settings) && !user_has_it {
            all.push(descriptor.clone());
        }
    }
    all.extend(user);
    all
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn transport_urls_are_unique() {
        let mut urls: Vec<_> = BUILTINS.iter().map(|b| b.transport_url).collect();
        urls.sort_unstable();
        urls.dedup();
        assert_eq!(urls.len(), BUILTINS.len());
    }

    #[test]
    fn cinemeta_serves_meta_for_imdb_ids() {
        let cinemeta = &DESCRIPTORS[0];
        assert_eq!(cinemeta.manifest.id, "com.linvo.cinemeta");
        assert!(!cinemeta.manifest.catalogs.is_empty());
        let meta = zeiler_addon::ResourcePath::new("meta", "series", "tt0108778");
        assert!(cinemeta.manifest.is_resource_supported(&meta));
    }

    fn user_addon(url: &str) -> Descriptor {
        let mut descriptor = DESCRIPTORS[0].clone();
        descriptor.transport_url = url::Url::parse(url).unwrap();
        descriptor.flags = DescriptorFlags::default();
        descriptor
    }

    #[test]
    fn builtins_come_first_and_respect_their_toggle() {
        let user = vec![user_addon("https://example.invalid/manifest.json")];

        let on = with_builtins(user.clone(), &Settings::default());
        assert_eq!(on.len(), 2);
        assert_eq!(on[0].transport_url.as_str(), BUILTINS[0].transport_url);
        assert!(on[0].flags.protected);

        let off = Settings {
            builtin_cinemeta: false,
            ..Settings::default()
        };
        let without = with_builtins(user, &off);
        assert_eq!(without.len(), 1);
        assert_eq!(
            without[0].transport_url.as_str(),
            "https://example.invalid/manifest.json"
        );
    }

    #[test]
    fn a_user_install_of_the_same_url_wins() {
        let user = vec![user_addon(BUILTINS[0].transport_url)];
        let all = with_builtins(user, &Settings::default());
        assert_eq!(all.len(), 1);
        assert!(!all[0].flags.protected, "the user's copy, not the built-in");
    }
}
