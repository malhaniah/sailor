use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnError, DefaultOnNull, VecSkipError, serde_as};
use url::Url;

use crate::request::{ExtraValue, ResourcePath};

/// An installed (or previewed) addon: its manifest plus the transport URL it was loaded
/// from. Identity is the transport URL, not the manifest id.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub manifest: Manifest,
    pub transport_url: Url,
    #[serde(default)]
    pub flags: DescriptorFlags,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptorFlags {
    #[serde(default)]
    pub official: bool,
    #[serde(default)]
    pub protected: bool,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    /// Kept as a plain string: the spec says semver, but real addons ship values like "1.0".
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub version: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub description: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub contact_email: Option<String>,
    /// Empty or invalid URLs become `None` instead of failing the manifest.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub logo: Option<Url>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub background: Option<Url>,
    #[serde_as(deserialize_as = "DefaultOnNull")]
    #[serde(default)]
    pub types: Vec<String>,
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub resources: Vec<ManifestResource>,
    #[serde(default)]
    pub id_prefixes: Option<Vec<String>>,
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub catalogs: Vec<ManifestCatalog>,
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    #[serde(default)]
    pub addon_catalogs: Vec<ManifestCatalog>,
    #[serde(default)]
    pub behavior_hints: ManifestBehaviorHints,
}

impl Manifest {
    /// Can this addon serve the given request? Mirrors the reference semantics:
    /// `catalog`/`addon_catalog` are routed by the catalogs lists alone; every other
    /// resource is routed by the `resources` entries, where the short string form inherits
    /// manifest-level `types`/`idPrefixes` and the object form uses its own. A missing
    /// `types` means unsupported; an absent or empty `idPrefixes` matches every id.
    pub fn is_resource_supported(&self, path: &ResourcePath) -> bool {
        match path.resource.as_str() {
            "catalog" => self.catalogs.iter().any(|c| {
                c.r#type == path.r#type && c.id == path.id && c.is_extra_supported(&path.extra)
            }),
            "addon_catalog" => self
                .addon_catalogs
                .iter()
                .any(|c| c.r#type == path.r#type && c.id == path.id),
            resource => {
                let Some(entry) = self.resources.iter().find(|r| r.name() == resource) else {
                    return false;
                };
                let (types, prefixes): (Option<&[String]>, Option<&[String]>) = match entry {
                    ManifestResource::Short(_) => {
                        (Some(self.types.as_slice()), self.id_prefixes.as_deref())
                    }
                    ManifestResource::Full {
                        types, id_prefixes, ..
                    } => (types.as_deref(), id_prefixes.as_deref()),
                };
                let Some(types) = types else {
                    return false;
                };
                if !types.iter().any(|t| t == &path.r#type) {
                    return false;
                }
                match prefixes {
                    None => true,
                    Some([]) => true,
                    Some(prefixes) => prefixes.iter().any(|p| path.id.starts_with(p.as_str())),
                }
            }
        }
    }
}

/// An entry of `manifest.resources`: either a bare resource name or an object narrowing
/// the types/id prefixes it serves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ManifestResource {
    Short(String),
    #[serde(rename_all = "camelCase")]
    Full {
        name: String,
        #[serde(default)]
        types: Option<Vec<String>>,
        #[serde(default)]
        id_prefixes: Option<Vec<String>>,
    },
}

impl ManifestResource {
    pub fn name(&self) -> &str {
        match self {
            ManifestResource::Short(name) => name,
            ManifestResource::Full { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestCatalog {
    #[serde(rename = "type")]
    pub r#type: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: ManifestExtra,
}

impl ManifestCatalog {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    /// The normalized extra-property declarations (modern `extra` wins over the legacy
    /// `extraSupported`/`extraRequired` short form).
    pub fn extra_props(&self) -> Vec<ExtraProp> {
        match &self.extra {
            ManifestExtra::Full { props } => props.clone(),
            ManifestExtra::Short {
                extra_required,
                extra_supported,
            } => extra_supported
                .iter()
                .map(|name| ExtraProp {
                    name: name.clone(),
                    is_required: extra_required.contains(name),
                    options: Vec::new(),
                    options_limit: 1,
                })
                .collect(),
        }
    }

    /// True when every supplied extra is declared and every required extra is supplied.
    pub fn is_extra_supported(&self, extra: &[ExtraValue]) -> bool {
        let props = self.extra_props();
        let supplied_declared = extra
            .iter()
            .all(|ev| props.iter().any(|p| p.name == ev.name));
        let required_supplied = props
            .iter()
            .filter(|p| p.is_required)
            .all(|p| extra.iter().any(|ev| ev.name == p.name));
        supplied_declared && required_supplied
    }

    /// Browsable without supplying anything (shown on a board/home row).
    pub fn is_browsable(&self) -> bool {
        self.extra_props().iter().all(|p| !p.is_required)
    }

    pub fn supports_extra(&self, name: &str) -> bool {
        self.extra_props().iter().any(|p| p.name == name)
    }

    /// The extras that make this catalog loadable with no user input, as stremio-core's
    /// Discover does: each required prop takes its first option. `None` when a required
    /// prop has no options to pick from (`search`, `lastVideosIds`, …), so the catalog
    /// can't be shown as a row; an empty list when nothing is required.
    pub fn default_extra(&self) -> Option<Vec<ExtraValue>> {
        self.extra_props()
            .into_iter()
            .filter(|p| p.is_required)
            .map(|p| {
                p.options.first().map(|first| ExtraValue {
                    name: p.name.clone(),
                    value: first.clone(),
                })
            })
            .collect()
    }

    /// Shown as a row (or in Browse) with [`default_extra`](Self::default_extra). Wider
    /// than [`is_browsable`](Self::is_browsable): Cinemeta's "New" needs a year, which
    /// its options supply.
    pub fn is_boardable(&self) -> bool {
        self.default_extra().is_some()
    }

    /// Keep only the extra options `keep` accepts (the legacy short form has none).
    pub fn retain_options(&mut self, mut keep: impl FnMut(&str) -> bool) {
        if let ManifestExtra::Full { props } = &mut self.extra {
            for prop in props {
                prop.options.retain(|option| keep(option));
            }
        }
    }

    /// The props a filter UI offers: those with options to choose from, except the
    /// paging and free-text ones.
    pub fn filter_props(&self) -> Vec<ExtraProp> {
        self.extra_props()
            .into_iter()
            .filter(|p| !p.options.is_empty() && p.name != "search" && p.name != "skip")
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ManifestExtra {
    /// Modern form; takes precedence when the `extra` key is present.
    Full {
        #[serde(rename = "extra")]
        props: Vec<ExtraProp>,
    },
    /// Legacy short form.
    #[serde(rename_all = "camelCase")]
    Short {
        #[serde(default)]
        extra_required: Vec<String>,
        #[serde(default)]
        extra_supported: Vec<String>,
    },
}

impl Default for ManifestExtra {
    fn default() -> Self {
        ManifestExtra::Short {
            extra_required: Vec::new(),
            extra_supported: Vec::new(),
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraProp {
    pub name: String,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub is_required: bool,
    #[serde_as(deserialize_as = "DefaultOnNull")]
    #[serde(default)]
    pub options: Vec<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default = "default_options_limit")]
    pub options_limit: usize,
}

fn default_options_limit() -> usize {
    1
}

#[serde_as]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestBehaviorHints {
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub adult: bool,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub p2p: bool,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub configurable: bool,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub configuration_required: bool,
    /// Unknown hints (e.g. Cinemeta's `newEpisodeNotifications`) are kept, not rejected.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Manifest {
        serde_json::from_str(json).expect("manifest should parse")
    }

    const MINIMAL: &str = r#"{
        "id": "org.stremio.example",
        "version": "0.0.1",
        "name": "Example Addon",
        "description": "Example Stremio Addon",
        "resources": ["catalog", "stream"],
        "types": ["movie", "series"],
        "catalogs": [{ "type": "movie", "id": "moviecatalog" }],
        "idPrefixes": ["tt"]
    }"#;

    #[test]
    fn parses_minimal_manifest() {
        let m = parse(MINIMAL);
        assert_eq!(m.id, "org.stremio.example");
        assert_eq!(m.version.as_deref(), Some("0.0.1"));
        assert_eq!(m.catalogs.len(), 1);
        assert_eq!(m.catalogs[0].display_name(), "moviecatalog");
        assert!(m.catalogs[0].is_browsable());
    }

    #[test]
    fn resource_routing_short_form_inherits_manifest_types_and_prefixes() {
        let m = parse(MINIMAL);
        assert!(m.is_resource_supported(&ResourcePath::new("stream", "movie", "tt123")));
        assert!(m.is_resource_supported(&ResourcePath::new("stream", "series", "tt123:1:1")));
        assert!(!m.is_resource_supported(&ResourcePath::new("stream", "movie", "kitsu:1")));
        assert!(!m.is_resource_supported(&ResourcePath::new("stream", "channel", "tt123")));
        assert!(!m.is_resource_supported(&ResourcePath::new("meta", "movie", "tt123")));
    }

    #[test]
    fn catalog_routing_ignores_resources_and_prefixes() {
        let m = parse(MINIMAL);
        assert!(m.is_resource_supported(&ResourcePath::new("catalog", "movie", "moviecatalog")));
        assert!(!m.is_resource_supported(&ResourcePath::new("catalog", "series", "moviecatalog")));
        assert!(!m.is_resource_supported(&ResourcePath::new("catalog", "movie", "other")));
    }

    #[test]
    fn object_form_resources_use_their_own_types_and_prefixes() {
        let m = parse(
            r#"{
            "id": "x", "name": "X", "version": "1.0.0",
            "types": ["movie"],
            "resources": [
                "catalog",
                { "name": "stream", "types": ["series"], "idPrefixes": ["kitsu"] },
                { "name": "meta", "types": ["series"] }
            ],
            "catalogs": []
        }"#,
        );
        assert!(m.is_resource_supported(&ResourcePath::new("stream", "series", "kitsu:1:2")));
        assert!(!m.is_resource_supported(&ResourcePath::new("stream", "series", "tt1:1:2")));
        assert!(!m.is_resource_supported(&ResourcePath::new("stream", "movie", "kitsu:1")));
        // object form without idPrefixes matches every id
        assert!(m.is_resource_supported(&ResourcePath::new("meta", "series", "anything")));
    }

    #[test]
    fn object_form_without_types_is_unsupported() {
        let m = parse(
            r#"{
            "id": "x", "name": "X",
            "types": ["movie"],
            "resources": [{ "name": "stream" }],
            "catalogs": []
        }"#,
        );
        assert!(!m.is_resource_supported(&ResourcePath::new("stream", "movie", "tt1")));
    }

    #[test]
    fn empty_id_prefixes_matches_all() {
        let m = parse(
            r#"{
            "id": "x", "name": "X",
            "types": ["movie"],
            "idPrefixes": [],
            "resources": ["stream"],
            "catalogs": []
        }"#,
        );
        assert!(m.is_resource_supported(&ResourcePath::new("stream", "movie", "whatever")));
    }

    #[test]
    fn cinemeta_style_catalog_with_both_extra_forms_prefers_full() {
        let m = parse(
            r#"{
            "id": "com.linvo.cinemeta", "name": "Cinemeta", "version": "3.0.14",
            "types": ["movie", "series"],
            "resources": ["catalog", "meta", "addon_catalog"],
            "idPrefixes": ["tt"],
            "behaviorHints": { "newEpisodeNotifications": true },
            "catalogs": [
                {
                    "type": "movie", "id": "top", "name": "Popular",
                    "genres": ["Action", "Comedy"],
                    "extra": [
                        { "name": "genre", "options": ["Action", "Comedy"] },
                        { "name": "search" },
                        { "name": "skip" }
                    ],
                    "extraSupported": ["search", "genre", "skip"]
                },
                {
                    "type": "movie", "id": "year", "name": "New",
                    "extra": [
                        { "name": "genre", "options": ["2026", "2025"], "isRequired": true },
                        { "name": "skip" }
                    ],
                    "extraRequired": ["genre"],
                    "extraSupported": ["genre", "skip"]
                }
            ]
        }"#,
        );
        let top = &m.catalogs[0];
        assert!(top.is_browsable());
        assert!(top.supports_extra("search"));
        assert!(top.supports_extra("skip"));
        assert_eq!(top.extra_props()[0].options, vec!["Action", "Comedy"]);

        let year = &m.catalogs[1];
        assert!(!year.is_browsable());
        assert!(year.is_extra_supported(&[ExtraValue {
            name: "genre".into(),
            value: "2026".into()
        }]));
        // required genre missing -> unsupported
        assert!(!year.is_extra_supported(&[ExtraValue {
            name: "skip".into(),
            value: "100".into()
        }]));
        // undeclared extra -> unsupported
        assert!(!top.is_extra_supported(&[ExtraValue {
            name: "date".into(),
            value: "2026-10-15".into()
        }]));
        // unknown behaviorHints keys preserved
        assert!(
            m.behavior_hints
                .other
                .contains_key("newEpisodeNotifications")
        );
    }

    #[test]
    fn legacy_short_extra_form() {
        let m = parse(
            r#"{
            "id": "x", "name": "X", "types": ["movie"], "resources": ["catalog"],
            "catalogs": [{
                "type": "movie", "id": "search-only",
                "extraRequired": ["search"], "extraSupported": ["search", "skip"]
            }]
        }"#,
        );
        let c = &m.catalogs[0];
        assert!(!c.is_browsable());
        assert!(c.is_extra_supported(&[ExtraValue {
            name: "search".into(),
            value: "q".into()
        }]));
        let props = c.extra_props();
        assert_eq!(props.len(), 2);
        assert!(props.iter().any(|p| p.name == "search" && p.is_required));
        assert!(props.iter().any(|p| p.name == "skip" && !p.is_required));
    }

    #[test]
    fn lenient_fields_do_not_fail_the_manifest() {
        let m = parse(
            r#"{
            "id": "x", "name": "X",
            "version": 1,
            "logo": "",
            "background": "not a url",
            "types": null,
            "resources": ["subtitles", 42],
            "catalogs": [{ "type": "movie", "id": "ok" }, { "bogus": true }]
        }"#,
        );
        assert_eq!(m.version, None);
        assert_eq!(m.logo, None);
        assert_eq!(m.background, None);
        assert!(m.types.is_empty());
        assert_eq!(m.resources.len(), 1);
        assert_eq!(m.catalogs.len(), 1);
    }

    #[test]
    fn default_and_filter_extras_follow_the_bundled_cinemeta_and_kitsu_snapshots() {
        let catalog = |json: &str, r#type: &str, id: &str| -> ManifestCatalog {
            parse(json)
                .catalogs
                .into_iter()
                .find(|c| c.r#type == r#type && c.id == id)
                .unwrap_or_else(|| panic!("no {type}/{id}"))
        };
        let cinemeta = include_str!("../../../src-tauri/builtin/cinemeta-manifest.json");
        let kitsu = include_str!("../../../src-tauri/builtin/kitsu-manifest.json");

        // "New" needs a year: its first option, so it can be a row
        let year = catalog(cinemeta, "movie", "year");
        assert!(!year.is_browsable());
        assert!(year.is_boardable());
        let default = year.default_extra().unwrap();
        assert_eq!(default.len(), 1);
        assert_eq!(default[0].name, "genre");
        assert!(default[0].value.chars().all(|c| c.is_ascii_digit()));
        assert!(year.is_extra_supported(&default));

        // nothing required: loadable as is
        let top = catalog(cinemeta, "movie", "top");
        assert_eq!(top.default_extra(), Some(Vec::new()));
        let names: Vec<_> = top.filter_props().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["genre"], "search and skip are not filters");

        // a required list of ids, or a required search: never a row
        assert_eq!(
            catalog(cinemeta, "series", "last-videos").default_extra(),
            None
        );
        let search_only = catalog(kitsu, "anime", "kitsu-anime-list");
        assert!(!search_only.is_boardable());
        assert!(search_only.filter_props().is_empty());
        assert!(
            catalog(kitsu, "anime", "kitsu-anime-trending")
                .filter_props()
                .is_empty()
        );
        assert!(
            !catalog(kitsu, "anime", "kitsu-anime-popular").filter_props()[0]
                .options
                .is_empty()
        );
    }

    #[test]
    fn descriptor_roundtrip() {
        let d: Descriptor = serde_json::from_str(&format!(
            r#"{{ "manifest": {MINIMAL}, "transportUrl": "https://a.example/manifest.json" }}"#
        ))
        .unwrap();
        assert!(!d.flags.official);
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"transportUrl\""));
    }
}
