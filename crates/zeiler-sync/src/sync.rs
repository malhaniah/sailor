//! The sync rules, pure: what to pull and push between the local library and the
//! account's, and how a local addon change lands in the account's collection.
//!
//! The library rules port stremio-core's `plan_sync_with_api` and its merge (MIT,
//! Copyright (c) 2019 SmartCode OOD): per item, the newer `_mtime` wins, compared at
//! one-second precision, which is what Stremio's own apps do. There is no field-level
//! merge.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Utc};
use zeiler_account::{LibraryItem, LibraryItemModified};

use crate::types::RawDescriptor;

/// Whether a local item goes to the account at all (stremio-core): never type
/// `other`, and an item the user removed only for a year after its last change.
pub fn should_sync(item: &LibraryItem, now: DateTime<Utc>) -> bool {
    item.r#type != "other" && (!item.removed || item.mtime > now - Duration::days(365))
}

/// What a library sync does.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LibraryPlan {
    /// Ids to fetch from the account (newer there, or missing here).
    pub pull: Vec<String>,
    /// Ids to send to the account (newer here, or missing there).
    pub push: Vec<String>,
}

/// Compare the local items with the account's `datastoreMeta`.
pub fn plan_library_sync(
    local: &BTreeMap<String, LibraryItem>,
    remote: &[LibraryItemModified],
    now: DateTime<Utc>,
) -> LibraryPlan {
    let secs = |ms: i64| ms.div_euclid(1000);
    let remote_mtimes: HashMap<&str, i64> = remote
        .iter()
        .map(|LibraryItemModified(id, ms)| (id.as_str(), secs(*ms)))
        .collect();
    let pull = remote
        .iter()
        .filter(|LibraryItemModified(id, ms)| match local.get(id) {
            Some(item) => secs(*ms) > secs(item.mtime.timestamp_millis()),
            None => true,
        })
        .map(|LibraryItemModified(id, _)| id.clone())
        .collect();
    let push = local
        .values()
        .filter(|item| should_sync(item, now))
        .filter(|item| match remote_mtimes.get(item.id.as_str()) {
            Some(remote) => secs(item.mtime.timestamp_millis()) > *remote,
            None => true,
        })
        .map(|item| item.id.clone())
        .collect();
    LibraryPlan { pull, push }
}

/// Take pulled items that are at least as new as the local copy and differ from it.
/// Returns the ids that changed.
pub fn merge_pulled(
    local: &mut BTreeMap<String, LibraryItem>,
    pulled: Vec<LibraryItem>,
) -> Vec<String> {
    let mut changed = Vec::new();
    for item in pulled {
        let take = match local.get(&item.id) {
            Some(existing) => item.mtime >= existing.mtime && *existing != item,
            None => true,
        };
        if take {
            changed.push(item.id.clone());
            local.insert(item.id.clone(), item);
        }
    }
    changed
}

// ---- addons --------------------------------------------------------------------------

/// Whether two transport URLs name the same addon: equal, or equal once parsed
/// (Zeiler keeps them parsed, so `HTTPS://A.example/…` and `https://a.example/…` agree).
pub fn same_url(a: &str, b: &str) -> bool {
    a == b
        || match (url::Url::parse(a), url::Url::parse(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

/// A local change to the user's addons, replayed onto the account's current collection.
#[derive(Debug, Clone)]
pub enum AddonOp {
    /// Installed or updated (same transport URL: replaced in place).
    Add(RawDescriptor),
    /// Removed, by transport URL.
    Remove(String),
    /// The user's addons in their new order (transport URLs).
    Reorder(Vec<String>),
}

/// Zeiler's own built-in addons, which are never pushed into an account that doesn't
/// have them (and are left alone in one that does).
#[derive(Debug, Clone, Default)]
pub struct Builtins {
    pub transport_urls: Vec<String>,
    pub manifest_ids: Vec<String>,
}

impl Builtins {
    pub fn matches(&self, addon: &RawDescriptor) -> bool {
        self.transport_urls.contains(&addon.transport_url)
            || addon
                .manifest_id()
                .is_some_and(|id| self.manifest_ids.iter().any(|b| b == id))
    }
}

/// The account's collection after `op`. Entries Stremio protects are never removed;
/// `flags` and unknown fields of untouched entries are kept byte for byte; a built-in
/// is never added.
pub fn rebase_addon_op(
    remote: &[RawDescriptor],
    op: &AddonOp,
    builtins: &Builtins,
) -> Vec<RawDescriptor> {
    let mut next = remote.to_vec();
    match op {
        AddonOp::Add(addon) => {
            match next
                .iter_mut()
                .find(|d| same_url(&d.transport_url, &addon.transport_url))
            {
                Some(existing) => {
                    // the account's flags stay; the manifest is the fresh one
                    existing.manifest = addon.manifest.clone();
                }
                None if builtins.matches(addon) => {}
                None => next.push(addon.clone()),
            }
        }
        AddonOp::Remove(url) => {
            next.retain(|d| !same_url(&d.transport_url, url) || d.is_protected())
        }
        AddonOp::Reorder(order) => {
            let wanted = |d: &RawDescriptor| order.iter().any(|u| same_url(u, &d.transport_url));
            let slots: Vec<usize> = next
                .iter()
                .enumerate()
                .filter(|(_, d)| wanted(d))
                .map(|(i, _)| i)
                .collect();
            let mut moved: Vec<RawDescriptor> = order
                .iter()
                .filter_map(|url| {
                    next.iter()
                        .find(|d| same_url(&d.transport_url, url))
                        .cloned()
                })
                .collect();
            moved.truncate(slots.len());
            for (slot, addon) in slots.into_iter().zip(moved) {
                next[slot] = addon;
            }
        }
    }
    next
}

/// The account entries Zeiler installs as the user's own addons: everything but
/// Zeiler's built-ins (those stay under Settings → Content), in the account's order.
pub fn importable(remote: &[RawDescriptor], builtins: &Builtins) -> Vec<RawDescriptor> {
    remote
        .iter()
        .filter(|d| !builtins.matches(d))
        .cloned()
        .collect()
}

/// How the first sign-in on a profile with addons of its own treats the two lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstSync {
    /// The account's addons replace the local ones.
    UseAccount,
    /// The account's addons, then the local ones it lacks (uploaded).
    Merge,
}

/// The collection after the first sign-in's choice (built-ins never added).
pub fn first_sync(
    local: &[RawDescriptor],
    remote: &[RawDescriptor],
    choice: FirstSync,
    builtins: &Builtins,
) -> Vec<RawDescriptor> {
    let mut next = remote.to_vec();
    if choice == FirstSync::Merge {
        for addon in local {
            if !builtins.matches(addon)
                && !next.iter().any(|d| d.transport_url == addon.transport_url)
            {
                next.push(addon.clone());
            }
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zeiler_account::parse_time;

    fn item(id: &str, mtime: &str, removed: bool) -> LibraryItem {
        serde_json::from_value(json!({
            "_id": id, "name": id, "type": "movie", "removed": removed, "temp": false,
            "_mtime": mtime, "state": {}, "behaviorHints": {}
        }))
        .unwrap()
    }

    fn local(items: &[LibraryItem]) -> BTreeMap<String, LibraryItem> {
        items.iter().map(|i| (i.id.clone(), i.clone())).collect()
    }

    fn ms(s: &str) -> i64 {
        parse_time(s).unwrap().timestamp_millis()
    }

    #[test]
    fn the_newer_side_wins_at_second_precision() {
        let now = parse_time("2026-10-17T00:00:00Z").unwrap();
        let mine = local(&[
            item("tt1", "2026-10-01T10:00:00.900Z", false),
            item("tt2", "2026-10-01T10:00:00Z", false),
            item("tt3", "2026-10-01T10:00:00Z", false),
            item("tt4", "2026-10-01T10:00:05Z", false),
        ]);
        let remote = [
            LibraryItemModified("tt1".into(), ms("2026-10-01T10:00:00.100Z")),
            LibraryItemModified("tt2".into(), ms("2026-10-02T10:00:00Z")),
            LibraryItemModified("tt4".into(), ms("2026-10-01T10:00:01Z")),
            LibraryItemModified("tt9".into(), ms("2026-09-01T00:00:00Z")),
        ];
        let plan = plan_library_sync(&mine, &remote, now);
        // tt1: the same second on both sides: nothing to do
        assert_eq!(plan.pull, ["tt2", "tt9"]);
        assert_eq!(plan.push, ["tt3", "tt4"]);
    }

    #[test]
    fn old_removed_items_and_other_types_stay_local() {
        let now = parse_time("2026-10-17T00:00:00Z").unwrap();
        let mut other = item("x", "2026-10-01T00:00:00Z", false);
        other.r#type = "other".into();
        let mine = local(&[
            item("old", "2024-01-01T00:00:00Z", true),
            item("recent", "2026-06-01T00:00:00Z", true),
            other,
        ]);
        assert_eq!(plan_library_sync(&mine, &[], now).push, ["recent"]);
    }

    #[test]
    fn pulled_items_replace_older_or_equal_differing_copies() {
        let mut mine = local(&[
            item("a", "2026-10-01T00:00:00Z", false),
            item("b", "2026-10-05T00:00:00Z", false),
        ]);
        let mut newer_a = item("a", "2026-10-02T00:00:00Z", false);
        newer_a.name = "A".into();
        let older_b = item("b", "2026-10-01T00:00:00Z", true);
        let same_b = mine["b"].clone();
        let changed = merge_pulled(
            &mut mine,
            vec![
                newer_a,
                older_b,
                same_b,
                item("c", "2026-10-01T00:00:00Z", false),
            ],
        );
        assert_eq!(changed, ["a", "c"]);
        assert_eq!(mine["a"].name, "A");
        assert!(!mine["b"].removed, "older pulled copy ignored");
    }

    fn raw(url: &str, id: &str, protected: bool) -> RawDescriptor {
        serde_json::from_value(json!({
            "manifest": {"id": id, "name": id},
            "transportUrl": url,
            "flags": {"official": protected, "protected": protected}
        }))
        .unwrap()
    }

    fn urls(list: &[RawDescriptor]) -> Vec<&str> {
        list.iter().map(|d| d.transport_url.as_str()).collect()
    }

    fn builtins() -> Builtins {
        Builtins {
            transport_urls: vec!["https://v3-cinemeta.strem.io/manifest.json".into()],
            manifest_ids: vec!["com.linvo.cinemeta".into()],
        }
    }

    #[test]
    fn adds_append_update_in_place_and_never_push_builtins() {
        let remote = vec![raw("https://a/manifest.json", "a", false)];
        let next = rebase_addon_op(
            &remote,
            &AddonOp::Add(raw("https://b/manifest.json", "b", false)),
            &builtins(),
        );
        assert_eq!(
            urls(&next),
            ["https://a/manifest.json", "https://b/manifest.json"]
        );

        let mut updated = raw("https://a/manifest.json", "a", false);
        updated.manifest["name"] = json!("A2");
        updated.flags = None;
        let next = rebase_addon_op(&remote, &AddonOp::Add(updated), &builtins());
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].manifest["name"], "A2");
        assert!(next[0].flags.is_some(), "the account's flags are kept");

        for builtin in [
            raw("https://v3-cinemeta.strem.io/manifest.json", "x", false),
            raw(
                "https://elsewhere/manifest.json",
                "com.linvo.cinemeta",
                false,
            ),
        ] {
            let next = rebase_addon_op(&remote, &AddonOp::Add(builtin), &builtins());
            assert_eq!(urls(&next), ["https://a/manifest.json"]);
        }
    }

    #[test]
    fn removes_spare_protected_entries() {
        let remote = vec![
            raw(
                "https://v3-cinemeta.strem.io/manifest.json",
                "com.linvo.cinemeta",
                true,
            ),
            raw("https://a/manifest.json", "a", false),
        ];
        let next = rebase_addon_op(
            &remote,
            &AddonOp::Remove("https://a/manifest.json".into()),
            &builtins(),
        );
        assert_eq!(urls(&next), ["https://v3-cinemeta.strem.io/manifest.json"]);
        let next = rebase_addon_op(
            &remote,
            &AddonOp::Remove("https://v3-cinemeta.strem.io/manifest.json".into()),
            &builtins(),
        );
        assert_eq!(next.len(), 2);
    }

    #[test]
    fn reorder_moves_only_the_users_addons_within_their_slots() {
        let remote = vec![
            raw(
                "https://v3-cinemeta.strem.io/manifest.json",
                "com.linvo.cinemeta",
                true,
            ),
            raw("https://a/manifest.json", "a", false),
            raw("https://local-files/manifest.json", "files", true),
            raw("https://b/manifest.json", "b", false),
        ];
        let order = vec![
            "https://b/manifest.json".into(),
            "https://a/manifest.json".into(),
        ];
        let next = rebase_addon_op(&remote, &AddonOp::Reorder(order), &builtins());
        assert_eq!(
            urls(&next),
            [
                "https://v3-cinemeta.strem.io/manifest.json",
                "https://b/manifest.json",
                "https://local-files/manifest.json",
                "https://a/manifest.json"
            ]
        );
        // an order naming addons the account doesn't have changes nothing else
        let next = rebase_addon_op(
            &remote,
            &AddonOp::Reorder(vec!["https://zzz/manifest.json".into()]),
            &builtins(),
        );
        assert_eq!(next, remote);
    }

    #[test]
    fn urls_match_once_parsed() {
        assert!(same_url(
            "HTTPS://A.example/x/manifest.json",
            "https://a.example/x/manifest.json"
        ));
        assert!(!same_url(
            "https://a.example/x/manifest.json",
            "https://a.example/y/manifest.json"
        ));
        let remote = vec![raw("https://A.example:443/manifest.json", "a", false)];
        let next = rebase_addon_op(
            &remote,
            &AddonOp::Remove("https://a.example/manifest.json".into()),
            &builtins(),
        );
        assert!(next.is_empty(), "removed despite the different spelling");
        let next = rebase_addon_op(
            &remote,
            &AddonOp::Add(raw("https://a.example/manifest.json", "a", false)),
            &builtins(),
        );
        assert_eq!(next.len(), 1, "updated in place, not duplicated");
    }

    #[test]
    fn import_and_first_sync_leave_builtins_out() {
        let remote = vec![
            raw(
                "https://v3-cinemeta.strem.io/manifest.json",
                "com.linvo.cinemeta",
                true,
            ),
            raw("https://a/manifest.json", "a", false),
        ];
        assert_eq!(
            urls(&importable(&remote, &builtins())),
            ["https://a/manifest.json"]
        );
        let local = vec![
            raw("https://b/manifest.json", "b", false),
            raw("https://a/manifest.json", "a", false),
            raw(
                "https://elsewhere/manifest.json",
                "com.linvo.cinemeta",
                false,
            ),
        ];
        assert_eq!(
            first_sync(&local, &remote, FirstSync::UseAccount, &builtins()),
            remote
        );
        assert_eq!(
            urls(&first_sync(&local, &remote, FirstSync::Merge, &builtins())),
            [
                "https://v3-cinemeta.strem.io/manifest.json",
                "https://a/manifest.json",
                "https://b/manifest.json"
            ]
        );
    }
}
