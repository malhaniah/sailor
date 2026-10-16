//! Subtitle files for the player and the preferences that pick them.
//!
//! - **Files** ([`SubtitleStore`]). Every subtitle the player loads — downloaded from a
//!   subtitle addon, attached to a stream, or picked from disk — is first normalized to
//!   UTF-8 (`zeiler_addon::decode_subtitle`: gzip, archives refused, BOM, detection,
//!   format) and written under `<cache_dir>/subs/` with a random name, then added to
//!   mpv (`commands/mpv/subtitles.rs`). The directory is emptied at startup and when a
//!   playback session starts or stops; mpv reads a text subtitle whole when it adds it.
//!   The store remembers where each file came from (in memory only), so the UI can ask
//!   for the same subtitle in another encoding without knowing the path or URL again.
//! - **Languages** ([`languages`]): the `alang`/`slang` lists mpv gets, with each
//!   language's ISO 639-2 codes (older mpv compares codes literally).
//! - **Per-title choices** ([`prefs`]).

pub mod languages;
pub mod prefs;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::warn;
use url::Url;
use zeiler_addon::SubtitleFile;

use crate::error::IpcError;
use crate::library::progress::PlaybackContext;
use crate::state::Settings;

/// The pre-load mpv options for a playback (`PlaybackRequest::options`), so mpv's own
/// track selection already follows the user's choices when the file opens:
/// - `alang`: the title's remembered audio language, then the preferred audio languages
///   (anime's own list for anime);
/// - `sid`: `no` when the title is remembered as "subtitles off", or when subtitles do
///   not show by themselves (`subtitles.autoLoad` off) and nothing is remembered for it;
///   otherwise `auto`. Always sent: options stay set in a core across loads.
/// - `slang` (unless `sid` is `no`): the title's remembered subtitle language, then the
///   preferred subtitle languages.
///
/// An empty language list is not sent, so external mpv keeps the user's own
/// `alang`/`slang`.
pub(crate) fn player_options(
    settings: &Settings,
    context: Option<&PlaybackContext>,
    title: Option<&prefs::TitlePrefs>,
) -> Vec<(String, String)> {
    let anime = context.is_some_and(|c| languages::is_anime(&c.meta.id, &c.meta.r#type));
    let remembered_audio = title.and_then(|t| t.audio_lang.clone());
    let audio: Vec<String> = remembered_audio
        .into_iter()
        .chain(if anime {
            settings.audio.anime_langs.iter().cloned()
        } else {
            settings.audio.langs.iter().cloned()
        })
        .collect();
    let remembered_sub = title.and_then(|t| t.sub_lang.clone());
    let off = match remembered_sub.as_deref() {
        Some(lang) => lang == "off",
        None => !settings.subtitles.auto_load,
    };
    let subtitles: Vec<String> = if off {
        Vec::new()
    } else {
        remembered_sub
            .into_iter()
            .chain(settings.subtitles.langs.iter().cloned())
            .collect()
    };
    let mut options = vec![(
        "sid".to_string(),
        if off { "no" } else { "auto" }.to_string(),
    )];
    for (name, langs) in [("alang", audio), ("slang", subtitles)] {
        let list = languages::mpv_language_list(&langs);
        if !list.is_empty() {
            options.push((name.to_string(), list));
        }
    }
    options
}

/// Where an added subtitle came from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Origin {
    Url(Url),
    Local(PathBuf),
}

/// One subtitle file written for the player.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Added {
    pub origin: Origin,
    /// The track title and language it was added with.
    pub title: Option<String>,
    pub lang: Option<String>,
}

/// The subtitle files of the current playback (see the module docs).
pub struct SubtitleStore {
    dir: PathBuf,
    added: Mutex<HashMap<String, Added>>,
}

impl SubtitleStore {
    /// The store under `cache_dir`, emptied (files of an earlier run are stale).
    pub fn new(cache_dir: &Path) -> Self {
        let store = Self {
            dir: cache_dir.join("subs"),
            added: Mutex::new(HashMap::new()),
        };
        store.purge();
        store
    }

    /// Forget and delete every file (a playback session started or stopped).
    pub fn purge(&self) {
        self.added.lock().unwrap().clear();
        if let Err(e) = std::fs::remove_dir_all(&self.dir)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!(error = %e, "could not empty the subtitle cache");
        }
    }

    /// Write `file` under a new random name; returns the name and the full path.
    pub(crate) async fn write(
        &self,
        file: &SubtitleFile,
        added: Added,
    ) -> Result<(String, PathBuf), IpcError> {
        tokio::fs::create_dir_all(&self.dir).await.map_err(|e| {
            IpcError::Storage(format!("Could not create the subtitle cache folder: {e}"))
        })?;
        let name = format!(
            "{}.{}",
            uuid::Uuid::new_v4().simple(),
            file.format.extension()
        );
        let path = self.dir.join(&name);
        tokio::fs::write(&path, file.text.as_bytes())
            .await
            .map_err(|e| IpcError::Storage(format!("Could not save the subtitle file: {e}")))?;
        self.added.lock().unwrap().insert(name.clone(), added);
        Ok((name, path))
    }

    /// Where the file `name` came from, if it is one of this playback's.
    pub(crate) fn added(&self, name: &str) -> Option<Added> {
        self.added.lock().unwrap().get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeiler_addon::decode_subtitle;

    fn context(id: &str, r#type: &str) -> PlaybackContext {
        serde_json::from_value(serde_json::json!({
            "meta": {"id": id, "type": r#type, "name": "A title"},
            "videoId": id,
        }))
        .unwrap()
    }

    fn options(
        settings: &Settings,
        context: Option<&PlaybackContext>,
        title: Option<&prefs::TitlePrefs>,
    ) -> Vec<(String, String)> {
        player_options(settings, context, title)
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn mpv_selects_what_the_user_chose_from_the_start() {
        let mut settings = Settings::default();
        // nothing chosen: subtitles may show (sid auto), no language lists sent
        assert_eq!(options(&settings, None, None), pairs(&[("sid", "auto")]));

        settings.subtitles.langs = vec!["en".into()];
        settings.audio.langs = vec!["de".into()];
        let show = context("tt1", "series");
        assert_eq!(
            options(&settings, Some(&show), None),
            pairs(&[
                ("sid", "auto"),
                ("alang", "de,ger,deu"),
                ("slang", "en,eng")
            ])
        );
        // anime uses its own audio list (["ja"] by default)
        assert_eq!(
            options(&settings, Some(&context("kitsu:1", "series")), None)[1],
            ("alang".to_string(), "ja,jpn".to_string())
        );

        // the title's remembered languages come first
        let remembered = prefs::TitlePrefs {
            meta_id: "tt1".into(),
            sub_lang: Some("ar".into()),
            audio_lang: Some("ja".into()),
            ..prefs::TitlePrefs::default()
        };
        assert_eq!(
            options(&settings, Some(&show), Some(&remembered)),
            pairs(&[
                ("sid", "auto"),
                ("alang", "ja,jpn,de,ger,deu"),
                ("slang", "ar,ara,en,eng")
            ])
        );

        // remembered "off": no subtitle shows by itself
        let off = prefs::TitlePrefs {
            sub_lang: Some("off".into()),
            ..remembered.clone()
        };
        assert_eq!(
            options(&settings, Some(&show), Some(&off)),
            pairs(&[("sid", "no"), ("alang", "ja,jpn,de,ger,deu")])
        );

        // automatic subtitles off: none shows, unless the title has a remembered one
        settings.subtitles.auto_load = false;
        assert_eq!(
            options(&settings, Some(&show), None),
            pairs(&[("sid", "no"), ("alang", "de,ger,deu")])
        );
        assert_eq!(
            options(&settings, Some(&show), Some(&remembered))[0],
            ("sid".to_string(), "auto".to_string())
        );
    }

    #[tokio::test]
    async fn files_get_random_names_and_a_purge_removes_them() {
        let dir = crate::persist::tests::temp_dir("subtitle-store");
        std::fs::create_dir_all(dir.join("subs")).unwrap();
        std::fs::write(dir.join("subs/stale.srt"), "old").unwrap();
        let store = SubtitleStore::new(&dir);
        assert!(!dir.join("subs/stale.srt").exists(), "emptied at startup");

        let file = decode_subtitle(b"WEBVTT\n\n00:01.000 --> 00:02.000\nHi\n", None).unwrap();
        let added = Added {
            origin: Origin::Url(Url::parse("https://subs.example/1.vtt").unwrap()),
            title: Some("English".into()),
            lang: Some("en".into()),
        };
        let (name, path) = store.write(&file, added.clone()).await.unwrap();
        assert!(name.ends_with(".vtt") && name.len() == 36, "{name}");
        assert_eq!(path, dir.join("subs").join(&name));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), file.text);
        assert_eq!(store.added(&name), Some(added));
        let (other, _) = store
            .write(
                &file,
                Added {
                    origin: Origin::Local(dir.join("a.vtt")),
                    title: None,
                    lang: None,
                },
            )
            .await
            .unwrap();
        assert_ne!(name, other);

        store.purge();
        assert!(!path.exists());
        assert_eq!(store.added(&name), None);
        store.purge(); // nothing left: still fine
    }
}
