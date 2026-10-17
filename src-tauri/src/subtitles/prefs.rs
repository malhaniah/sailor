//! Per-title playback choices: the subtitle and audio language the user picked for a
//! show (or that they switched subtitles off), where that subtitle came from (the video
//! itself, the stream, or a subtitle addon), its subtitle delay and its speed, so the
//! next episode starts the same way. Kept in `player-prefs.json` under `profile_dir()`,
//! for the 200 titles used most recently.
//!
//! Read leniently (a bad entry or field is dropped, never the file); written strictly
//! (`player_prefs_set` refuses an unknown field or a value outside the player's rules).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::{DefaultOnError, VecSkipError, serde_as};

use crate::error::IpcError;
use crate::persist::JsonStore;
use crate::settings_groups::is_language_code;

/// How many titles are remembered.
const MAX_TITLES: usize = 200;
const FILE_VERSION: u32 = 1;

#[serde_as]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrefsFile {
    version: u32,
    /// Most recently used first.
    #[serde_as(deserialize_as = "VecSkipError<_>")]
    titles: Vec<TitlePrefs>,
}

/// The choices remembered for one title (a show or a movie, by meta id).
#[serde_as]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TitlePrefs {
    pub meta_id: String,
    /// The language of the subtitle picked last, or `"off"`.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub sub_lang: Option<String>,
    /// Where the subtitle picked last came from: `embedded` (in the video), `stream`
    /// (listed by the stream) or `addon` (a subtitle addon). The next episode's automatic
    /// pick prefers that source in that language.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub sub_source: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub audio_lang: Option<String>,
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub sub_delay_sec: Option<f64>,
    /// Playback speed.
    #[serde_as(deserialize_as = "DefaultOnError")]
    #[serde(default)]
    pub rate: Option<f64>,
}

impl TitlePrefs {
    /// The fields that pass the rules (a stored value from a damaged file is dropped).
    fn sanitized(mut self) -> Self {
        if self.sub_lang.as_deref().is_some_and(|l| !valid_sub_lang(l)) {
            self.sub_lang = None;
        }
        if self
            .sub_source
            .as_deref()
            .is_some_and(|s| !valid_sub_source(s))
        {
            self.sub_source = None;
        }
        if self
            .audio_lang
            .as_deref()
            .is_some_and(|l| !is_language_code(l))
        {
            self.audio_lang = None;
        }
        if self.sub_delay_sec.is_some_and(|d| !valid_delay(d)) {
            self.sub_delay_sec = None;
        }
        if self.rate.is_some_and(|r| !valid_rate(r)) {
            self.rate = None;
        }
        self
    }
}

fn valid_sub_lang(lang: &str) -> bool {
    lang == "off" || is_language_code(lang)
}

fn valid_sub_source(source: &str) -> bool {
    matches!(source, "embedded" | "stream" | "addon")
}

fn valid_delay(delay: f64) -> bool {
    delay.is_finite() && (-600.0..=600.0).contains(&delay)
}

fn valid_rate(rate: f64) -> bool {
    rate.is_finite() && (0.25..=4.0).contains(&rate)
}

fn check_meta_id(meta_id: &str) -> Result<(), IpcError> {
    if meta_id.is_empty() || meta_id.len() > 256 || meta_id.chars().any(char::is_control) {
        return Err(IpcError::InvalidInput("That title id is not valid.".into()));
    }
    Ok(())
}

/// The per-title store, managed by Tauri.
pub struct PlayerPrefs {
    store: JsonStore<PrefsFile>,
    /// [`MAX_TITLES`] (smaller in tests)
    max_titles: usize,
}

impl PlayerPrefs {
    pub async fn load(profile_dir: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            store: JsonStore::load(profile_dir.join("player-prefs.json")).await?,
            max_titles: MAX_TITLES,
        })
    }

    /// The choices remembered for `meta_id`.
    pub async fn get(&self, meta_id: &str) -> Result<Option<TitlePrefs>, IpcError> {
        check_meta_id(meta_id)?;
        Ok(self
            .store
            .read(|file| file.titles.iter().find(|t| t.meta_id == meta_id).cloned())
            .await
            .map(TitlePrefs::sanitized))
    }

    /// Merge `patch` (`subLang`, `subSource`, `audioLang`, `subDelaySec`, `rate`; `null` forgets a
    /// choice) into `meta_id`'s choices and make it the most recently used title.
    pub async fn set(&self, meta_id: &str, patch: Value) -> Result<TitlePrefs, IpcError> {
        check_meta_id(meta_id)?;
        let Value::Object(patch) = patch else {
            return Err(IpcError::InvalidInput(
                "A preferences update must be an object.".into(),
            ));
        };
        let invalid =
            |key: &str| IpcError::InvalidInput(format!("The value for \"{key}\" is not valid."));
        let text = |key: &str, value: &Value, ok: fn(&str) -> bool| match value {
            Value::Null => Ok(None),
            Value::String(s) if ok(s) => Ok(Some(s.clone())),
            _ => Err(invalid(key)),
        };
        let number = |key: &str, value: &Value, ok: fn(f64) -> bool| match value {
            Value::Null => Ok(None),
            value => value
                .as_f64()
                .filter(|n| ok(*n))
                .map(Some)
                .ok_or_else(|| invalid(key)),
        };
        let updated = self
            .store
            .try_update(|file| {
                file.version = FILE_VERSION;
                let at = file.titles.iter().position(|t| t.meta_id == meta_id);
                let mut entry = at
                    .map(|i| file.titles.remove(i).sanitized())
                    .unwrap_or_else(|| TitlePrefs {
                        meta_id: meta_id.to_string(),
                        ..TitlePrefs::default()
                    });
                let before = entry.clone();
                for (key, value) in &patch {
                    match key.as_str() {
                        "subLang" => entry.sub_lang = text(key, value, valid_sub_lang)?,
                        "subSource" => entry.sub_source = text(key, value, valid_sub_source)?,
                        "audioLang" => entry.audio_lang = text(key, value, is_language_code)?,
                        "subDelaySec" => entry.sub_delay_sec = number(key, value, valid_delay)?,
                        "rate" => entry.rate = number(key, value, valid_rate)?,
                        other => {
                            return Err(IpcError::InvalidInput(format!(
                                "\"{other}\" is not a player preference."
                            )));
                        }
                    }
                }
                file.titles.insert(0, entry.clone());
                file.titles.truncate(self.max_titles);
                // unchanged and already the most recent: nothing to write
                Ok((at != Some(0) || entry != before).then_some(entry))
            })
            .await?;
        match updated {
            Some(entry) => Ok(entry),
            None => Ok(self.get(meta_id).await?.unwrap_or_default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn prefs(test: &str) -> (PlayerPrefs, std::path::PathBuf) {
        let dir = crate::persist::tests::temp_dir(test);
        (PlayerPrefs::load(&dir).await.unwrap(), dir)
    }

    #[tokio::test]
    async fn choices_are_merged_kept_per_title_and_persisted() {
        let (prefs, dir) = prefs("player-prefs").await;
        assert_eq!(prefs.get("tt1").await.unwrap(), None);

        prefs
            .set(
                "tt1",
                json!({"subLang": "ar", "subSource": "addon", "subDelaySec": 1.5}),
            )
            .await
            .unwrap();
        let entry = prefs.set("tt1", json!({"rate": 1.25})).await.unwrap();
        assert_eq!(entry.sub_lang.as_deref(), Some("ar"));
        assert_eq!(entry.sub_source.as_deref(), Some("addon"));
        assert_eq!(entry.sub_delay_sec, Some(1.5));
        assert_eq!(entry.rate, Some(1.25));
        let off = prefs
            .set("tt1", json!({"subLang": "off", "subDelaySec": null}))
            .await
            .unwrap();
        assert_eq!(off.sub_lang.as_deref(), Some("off"));
        assert_eq!(off.sub_delay_sec, None);
        prefs
            .set("kitsu:1", json!({"audioLang": "ja"}))
            .await
            .unwrap();

        let reloaded = PlayerPrefs::load(&dir).await.unwrap();
        assert_eq!(reloaded.get("tt1").await.unwrap(), Some(off));
        assert_eq!(
            reloaded
                .get("kitsu:1")
                .await
                .unwrap()
                .unwrap()
                .audio_lang
                .as_deref(),
            Some("ja")
        );
    }

    #[tokio::test]
    async fn bad_updates_are_refused_and_change_nothing() {
        let (prefs, _) = prefs("player-prefs-bad").await;
        prefs.set("tt1", json!({"rate": 2})).await.unwrap();
        for patch in [
            json!({"rate": 10}),
            json!({"rate": "fast"}),
            json!({"subDelaySec": 601}),
            json!({"subLang": "en,fr"}),
            json!({"audioLang": "english"}),
            json!({"subSource": "local"}),
            json!({"subSource": 1}),
            json!({"volume": 50}),
            json!(["rate"]),
        ] {
            assert!(
                matches!(
                    prefs.set("tt1", patch.clone()).await,
                    Err(IpcError::InvalidInput(_))
                ),
                "{patch}"
            );
        }
        assert_eq!(prefs.get("tt1").await.unwrap().unwrap().rate, Some(2.0));
        assert!(prefs.get("").await.is_err());
        assert!(prefs.set("a\nb", json!({})).await.is_err());
    }

    #[tokio::test]
    async fn only_the_most_recent_titles_are_kept_and_damage_is_dropped() {
        let (mut prefs, dir) = prefs("player-prefs-lru").await;
        prefs.max_titles = 4;
        for i in 0..prefs.max_titles + 5 {
            prefs
                .set(&format!("tt{i}"), json!({"rate": 1.5}))
                .await
                .unwrap();
        }
        assert_eq!(prefs.get("tt0").await.unwrap(), None, "the oldest went");
        assert!(prefs.get("tt8").await.unwrap().is_some());
        // touching one moves it to the front, so it outlives newer ones
        prefs.set("tt5", json!({})).await.unwrap();
        for i in 0..3 {
            prefs.set(&format!("new{i}"), json!({})).await.unwrap();
        }
        assert!(prefs.get("tt5").await.unwrap().is_some());
        assert_eq!(prefs.get("tt8").await.unwrap(), None);

        std::fs::write(
            dir.join("player-prefs.json"),
            r#"{"version": 1, "titles": [
                {"metaId": "tt1", "subLang": "not a code", "rate": 99, "subDelaySec": "x", "audioLang": "en", "subSource": "elsewhere"},
                {"subLang": "en"},
                5
            ]}"#,
        )
        .unwrap();
        let damaged = PlayerPrefs::load(&dir).await.unwrap();
        assert_eq!(
            damaged.get("tt1").await.unwrap(),
            Some(TitlePrefs {
                meta_id: "tt1".into(),
                audio_lang: Some("en".into()),
                ..TitlePrefs::default()
            })
        );
    }
}
