//! `homeRows{}`: the user's order, hidden set and names for Home's catalog rows.
//!
//! Rows are keyed by `src/lib/catalogs.ts` `catalogKey`: the addon's manifest id, a short
//! hash of its transport URL, the catalog's type and id, and its extras. Never the URL
//! itself, which can carry an addon's configuration (tokens) and is not something to
//! keep in settings.json, which the webview mirrors.
//!
//! Every field is a list, so a patch replaces it whole (lists are never merged), and a
//! rename is removed by leaving it out.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::settings_groups::{SettingsObject, read_lenient, settings_object};

/// Most rows a list names; far more than any Home has.
const MAX_ROWS: usize = 500;
const MAX_KEY_LEN: usize = 512;
const MAX_NAME_CHARS: usize = 80;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", rename_all = "camelCase", default)]
pub struct HomeRows {
    /// Row keys in the user's order; rows not named here follow in their natural order.
    pub order: Vec<String>,
    /// Row keys the user hid.
    pub hidden: Vec<String>,
    /// The user's own row titles.
    pub renamed: Vec<RowName>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowName {
    pub key: String,
    pub name: String,
}

impl HomeRows {
    fn check(&self) -> Result<(), String> {
        for (what, keys) in [("order", &self.order), ("hidden", &self.hidden)] {
            if keys.len() > MAX_ROWS {
                return Err(format!("{what} names more than {MAX_ROWS} rows"));
            }
            if let Some(bad) = keys.iter().find(|k| !is_row_key(k)) {
                return Err(format!("{what}: \"{bad}\" is not a row key"));
            }
        }
        if self.renamed.len() > MAX_ROWS {
            return Err(format!("renamed names more than {MAX_ROWS} rows"));
        }
        for (i, RowName { key, name }) in self.renamed.iter().enumerate() {
            if !is_row_key(key) {
                return Err(format!("renamed: \"{key}\" is not a row key"));
            }
            let chars = name.chars().count();
            if name.trim().is_empty()
                || chars > MAX_NAME_CHARS
                || name.chars().any(char::is_control)
            {
                return Err(format!(
                    "renamed: a row name has 1 to {MAX_NAME_CHARS} characters, no control characters"
                ));
            }
            if self.renamed[..i].iter().any(|r| &r.key == key) {
                return Err(format!("renamed: \"{key}\" is named twice"));
            }
        }
        Ok(())
    }
}

fn is_row_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_KEY_LEN && !key.chars().any(char::is_control)
}

settings_object!(HomeRows, "homeRows");

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::settings_groups::parse_strict;
    use crate::state::Settings;

    use super::*;

    #[test]
    fn defaults_are_empty_and_round_trip() {
        let rows = HomeRows::default();
        let value = serde_json::to_value(&rows).unwrap();
        assert_eq!(value, json!({"order": [], "hidden": [], "renamed": []}));
        let back: HomeRows = serde_json::from_value(value).unwrap();
        assert_eq!(back, rows);
    }

    #[test]
    fn stored_values_read_per_field() {
        let stored: HomeRows = serde_json::from_value(json!({
            "order": ["a|1|movie|top|", "b|2|series|top|"],
            "hidden": "not a list",
            "renamed": [{"key": "a|1|movie|top|", "name": "Mine"}]
        }))
        .unwrap();
        assert_eq!(stored.order.len(), 2);
        assert!(stored.hidden.is_empty(), "wrong type → default");
        assert_eq!(stored.renamed[0].name, "Mine");
    }

    #[test]
    fn patches_replace_lists_and_refuse_bad_values() {
        let settings = Settings::default()
            .patched(json!({"homeRows": {"hidden": ["k1", "k2"], "renamed": [{"key": "k1", "name": "One"}]}}))
            .unwrap();
        assert_eq!(settings.home_rows.hidden, ["k1", "k2"]);
        let settings = settings
            .patched(json!({"homeRows": {"renamed": []}}))
            .unwrap();
        assert!(
            settings.home_rows.renamed.is_empty(),
            "a rename is removed by leaving it out"
        );
        assert_eq!(settings.home_rows.hidden, ["k1", "k2"], "other fields kept");

        for bad in [
            json!({"homeRows": {"order": [""]}}),
            json!({"homeRows": {"hidden": ["a\nb"]}}),
            json!({"homeRows": {"renamed": [{"key": "k", "name": " "}]}}),
            json!({"homeRows": {"renamed": [{"key": "k", "name": "x".repeat(81)}]}}),
            json!({"homeRows": {"renamed": [{"key": "k", "name": "a"}, {"key": "k", "name": "b"}]}}),
            json!({"homeRows": {"renamed": [{"key": "k", "name": "a", "extra": 1}]}}),
            json!({"homeRows": {"order": "k"}}),
            json!({"homeRows": {"sort": []}}),
        ] {
            assert!(settings.patched(bad.clone()).is_err(), "{bad}");
        }
        assert!(parse_strict::<HomeRows>(&json!({"order": ["k"]})).is_ok());
    }
}
