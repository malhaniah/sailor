//! What changed between two copies of an addon's manifest: the installed snapshot and
//! a fresh fetch (update detection), or a directory listing and the live manifest
//! (install from the marketplace).

use std::collections::BTreeSet;

use serde::Serialize;

use crate::manifest::Manifest;

/// The differences that matter to the user, as sentences, and whether any of them
/// needs a fresh look before it is accepted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestChanges {
    /// One sentence per change, most important first.
    pub changes: Vec<String>,
    /// Something beyond name, description, logo or version changed: what it serves,
    /// for which types or ids, its catalogs or its declared hints.
    pub capabilities_changed: bool,
    /// It now serves streams, declares P2P or adult content, needs setup, or its id
    /// changed: the update goes through the full confirmation, never one click.
    pub requires_review: bool,
    /// How many of `changes` (the last ones) are cosmetic: version, name,
    /// description or logo.
    pub cosmetic: usize,
}

impl ManifestChanges {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The changes beyond the cosmetic ones.
    pub fn capability_changes(&self) -> &[String] {
        &self.changes[..self.changes.len() - self.cosmetic]
    }
}

fn resource_names(m: &Manifest) -> BTreeSet<String> {
    m.resources.iter().map(|r| r.name().to_string()).collect()
}

fn catalog_keys(m: &Manifest) -> BTreeSet<String> {
    m.catalogs
        .iter()
        .map(|c| format!("{} “{}”", c.r#type, c.display_name()))
        .collect()
}

/// Per-resource types and id prefixes, so narrowing a resource is seen too.
fn routing(m: &Manifest) -> BTreeSet<String> {
    m.resources
        .iter()
        .map(|r| match r {
            crate::manifest::ManifestResource::Short(name) => name.clone(),
            crate::manifest::ManifestResource::Full {
                name,
                types,
                id_prefixes,
            } => format!("{name}:{types:?}:{id_prefixes:?}"),
        })
        .collect()
}

fn list(items: &BTreeSet<String>) -> String {
    items.iter().cloned().collect::<Vec<_>>().join(", ")
}

fn friendly(resource: &str) -> &str {
    match resource {
        "stream" => "streams",
        "meta" => "details",
        "catalog" => "catalogs",
        "subtitles" => "subtitles",
        "addon_catalog" => "addon catalogs",
        other => other,
    }
}

/// Compare `old` (what Zeiler has) with `new` (what the addon serves now).
pub fn manifest_changes(old: &Manifest, new: &Manifest) -> ManifestChanges {
    let mut review = Vec::new();
    let mut capability = Vec::new();
    let mut cosmetic = Vec::new();

    if old.id != new.id {
        review.push(format!(
            "Its id changed from {} to {}: it may be a different addon.",
            old.id, new.id
        ));
    }

    let (old_res, new_res) = (resource_names(old), resource_names(new));
    for added in new_res.difference(&old_res) {
        let line = format!("Now serves {}.", friendly(added));
        if added == "stream" {
            review.push(line);
        } else {
            capability.push(line);
        }
    }
    for removed in old_res.difference(&new_res) {
        capability.push(format!("No longer serves {}.", friendly(removed)));
    }

    let (oh, nh) = (&old.behavior_hints, &new.behavior_hints);
    if nh.p2p && !oh.p2p {
        review.push("Now declares P2P: your IP address is visible to peers.".into());
    }
    if nh.adult && !oh.adult {
        review.push("Now declares adult content.".into());
    }
    if nh.configuration_required && !oh.configuration_required {
        review.push("Now needs to be set up on the addon's page.".into());
    }
    if oh.p2p && !nh.p2p {
        capability.push("No longer declares P2P.".into());
    }
    if oh.adult && !nh.adult {
        capability.push("No longer declares adult content.".into());
    }
    if oh.configuration_required && !nh.configuration_required {
        capability.push("No longer needs setup.".into());
    }

    let (old_types, new_types): (BTreeSet<_>, BTreeSet<_>) = (
        old.types.iter().cloned().collect(),
        new.types.iter().cloned().collect(),
    );
    let added: BTreeSet<_> = new_types.difference(&old_types).cloned().collect();
    let removed: BTreeSet<_> = old_types.difference(&new_types).cloned().collect();
    if !added.is_empty() {
        capability.push(format!("Adds types: {}.", list(&added)));
    }
    if !removed.is_empty() {
        capability.push(format!("Drops types: {}.", list(&removed)));
    }
    if old.id_prefixes != new.id_prefixes || routing(old) != routing(new) {
        // only worth saying when the resource list itself didn't already change
        if old_res == new_res {
            capability.push("Changes which titles it answers for.".into());
        }
    }

    let (old_cat, new_cat) = (catalog_keys(old), catalog_keys(new));
    let added: BTreeSet<_> = new_cat.difference(&old_cat).cloned().collect();
    let removed: BTreeSet<_> = old_cat.difference(&new_cat).cloned().collect();
    if !added.is_empty() {
        capability.push(format!("New catalogs: {}.", list(&added)));
    }
    if !removed.is_empty() {
        capability.push(format!("Removed catalogs: {}.", list(&removed)));
    }

    if old.version != new.version {
        cosmetic.push(format!(
            "Version {} → {}.",
            old.version.as_deref().unwrap_or("unknown"),
            new.version.as_deref().unwrap_or("unknown")
        ));
    }
    if old.name != new.name {
        cosmetic.push(format!("Renamed from “{}” to “{}”.", old.name, new.name));
    }
    // a logo that appears where there was none is not news: older builds dropped
    // relative logos, which are now resolved
    let logo_changed = old.logo.is_some() && old.logo != new.logo;
    if old.description != new.description || logo_changed {
        cosmetic.push("New description or logo.".into());
    }

    let requires_review = !review.is_empty();
    let capabilities_changed = requires_review || !capability.is_empty();
    let cosmetic_count = cosmetic.len();
    ManifestChanges {
        cosmetic: cosmetic_count,
        changes: review
            .into_iter()
            .chain(capability)
            .chain(cosmetic)
            .collect(),
        capabilities_changed,
        requires_review,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest(value: serde_json::Value) -> Manifest {
        let mut base = json!({
            "id": "org.example.a", "name": "A", "version": "1.0.0",
            "types": ["movie"], "resources": ["catalog", "meta"],
            "catalogs": [{ "type": "movie", "id": "top", "name": "Top" }]
        });
        for (k, v) in value.as_object().unwrap() {
            base[k] = v.clone();
        }
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn identical_manifests_have_no_changes() {
        let a = manifest(json!({}));
        assert!(manifest_changes(&a, &a).is_empty());
    }

    #[test]
    fn a_version_bump_with_the_same_capabilities_is_cosmetic() {
        let old = manifest(json!({}));
        let new = manifest(json!({ "version": "1.1.0", "description": "Better." }));
        let changes = manifest_changes(&old, &new);
        assert_eq!(
            changes.changes,
            ["Version 1.0.0 → 1.1.0.", "New description or logo."]
        );
        assert!(!changes.capabilities_changed && !changes.requires_review);
        assert!(changes.capability_changes().is_empty());
    }

    #[test]
    fn a_logo_where_there_was_none_is_no_change() {
        // stored by an older build that dropped relative logos
        let old = manifest(json!({}));
        let new = manifest(json!({ "logo": "https://a.example/logo.png" }));
        assert!(manifest_changes(&old, &new).is_empty());
        let changed = manifest(json!({ "logo": "https://a.example/other.png" }));
        assert_eq!(
            manifest_changes(&new, &changed).changes,
            ["New description or logo."]
        );
    }

    #[test]
    fn new_catalogs_and_types_change_capabilities_without_review() {
        let old = manifest(json!({}));
        let new = manifest(json!({
            "types": ["movie", "series"],
            "catalogs": [
                { "type": "movie", "id": "top", "name": "Top" },
                { "type": "series", "id": "top", "name": "Top" }
            ]
        }));
        let changes = manifest_changes(&old, &new);
        assert!(changes.capabilities_changed);
        assert!(!changes.requires_review);
        assert!(changes.changes.contains(&"Adds types: series.".to_string()));
        assert!(
            changes
                .changes
                .contains(&"New catalogs: series “Top”.".to_string())
        );
    }

    #[test]
    fn escalations_require_review() {
        let old = manifest(json!({}));
        for (new, expected) in [
            (
                json!({ "resources": ["catalog", "meta", "stream"] }),
                "Now serves streams.",
            ),
            (
                json!({ "behaviorHints": { "p2p": true } }),
                "Now declares P2P: your IP address is visible to peers.",
            ),
            (
                json!({ "behaviorHints": { "adult": true } }),
                "Now declares adult content.",
            ),
            (
                json!({ "behaviorHints": { "configurationRequired": true } }),
                "Now needs to be set up on the addon's page.",
            ),
            (
                json!({ "id": "org.other" }),
                "Its id changed from org.example.a to org.other: it may be a different addon.",
            ),
        ] {
            let changes = manifest_changes(&old, &manifest(new.clone()));
            assert!(changes.requires_review, "{new}");
            assert_eq!(changes.changes[0], expected, "{new}");
        }
    }

    #[test]
    fn narrowing_a_resource_is_a_capability_change() {
        let old = manifest(json!({ "resources": ["meta"] }));
        let new = manifest(json!({
            "resources": [{ "name": "meta", "types": ["movie"], "idPrefixes": ["tt"] }]
        }));
        let changes = manifest_changes(&old, &new);
        assert_eq!(changes.changes, ["Changes which titles it answers for."]);
        assert!(changes.capabilities_changed && !changes.requires_review);
    }

    #[test]
    fn removals_are_reported() {
        let old = manifest(json!({ "behaviorHints": { "p2p": true } }));
        let new = manifest(json!({ "resources": ["catalog"], "catalogs": [] }));
        let changes = manifest_changes(&old, &new);
        assert!(
            changes
                .changes
                .contains(&"No longer serves details.".to_string())
        );
        assert!(
            changes
                .changes
                .contains(&"No longer declares P2P.".to_string())
        );
        assert!(
            changes
                .changes
                .contains(&"Removed catalogs: movie “Top”.".to_string())
        );
        assert!(!changes.requires_review);
    }
}
