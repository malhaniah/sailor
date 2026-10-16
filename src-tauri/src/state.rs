use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use tokio::sync::{OnceCell, RwLock};
use tracing::{info, warn};
use zeiler_addon::{AddonClient, Descriptor};
use zeiler_engine::{Engine, EngineConfig};
use zeiler_player::{Player, PlayerBackend};

use crate::error::IpcError;
use crate::home_rows::HomeRows;
use crate::persist::{read_json, write_json};
use crate::settings_groups::{
    AudioSettings, KeyError, SettingsObject, SkipSettings, SubtitleSettings, survives, with_key,
};

/// Device settings, persisted as `settings.json` and mirrored wholesale to the UI (so
/// never put secrets here). The reserved key map for later features is in
/// docs/ARCHITECTURE.md ("Settings schema").
///
/// Evolution is additive only: new fields get a default, and an existing field's name
/// or type never changes. Reading is lenient per top-level key (see the `Deserialize`
/// impl), so one bad value resets only that setting; `settings_patch` is strict.
///
/// `remote = "Self"` makes the derives generate inherent `Settings::serialize` /
/// `Settings::deserialize` (the strict, whole-object forms) instead of the trait impls,
/// which are written by hand below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", rename_all = "camelCase", default)]
pub struct Settings {
    /// Explicit mpv binary path; None = search PATH.
    pub mpv_path: Option<String>,
    /// Incoming BitTorrent TCP port (0 = random). Applies when the engine starts.
    pub engine_listen_port: u16,
    pub engine_upnp: bool,
    /// KiB/s; None = unlimited.
    pub upload_limit_kibps: Option<u32>,
    pub download_limit_kibps: Option<u32>,
    /// Remove torrent data when playback of it is stopped from the UI.
    pub delete_on_stop: bool,
    /// UI theme preset id (see src/lib/theme.ts).
    pub theme: String,
    /// Play the first direct stream automatically instead of always showing the picker.
    pub instant_play: bool,
    /// Hide the Anime room from navigation.
    pub hide_anime: bool,
    /// Hide the Live TV room from navigation (its catalogs then go to Home and Shows).
    pub hide_live_tv: bool,
    /// Show adult catalogs, genres and titles (an addon flagged adult, and whatever
    /// `zeiler_addon::is_adult_text` matches). Off by default; turning it on asks the
    /// user to confirm their age.
    pub show_adult: bool,
    /// Home's catalog rows as the user arranged them (`home_rows.rs`).
    pub home_rows: HomeRows,
    /// Use the built-in Cinemeta metadata addon (catalogs, details, episode lists).
    /// Metadata only — it serves no streams.
    pub builtin_cinemeta: bool,
    /// Use the built-in Anime Kitsu metadata addon (anime catalogs and details for
    /// Kitsu, MyAnimeList, AniList and AniDB ids). Metadata only: the subtitles its
    /// manifest declares are left out (see `builtin.rs`).
    pub builtin_kitsu: bool,
    /// Use the built-in OpenSubtitles v3 addon (subtitles for IMDb titles). Subtitles
    /// only.
    pub builtin_opensubtitles: bool,
    /// Download updates in the background and install them on the next launch.
    pub auto_update: bool,
    /// Which player `player_start` uses when asked for `auto`: `auto` (built-in when
    /// this build has it and it starts, else external mpv), `embedded` or `external`.
    /// An unknown stored value resets just this setting (lenient read).
    pub player_backend: PlayerBackend,
    /// Subtitle languages, auto-load and look (`settings_groups.rs`). Read leniently
    /// per field; a patch names only the fields it changes.
    pub subtitles: SubtitleSettings,
    /// Preferred audio languages.
    pub audio: AudioSettings,
    /// The skip button, skipping by itself, and where skip times come from.
    pub skip: SkipSettings,
    /// Play the next episode by itself when one ends (when the same addon has a stream
    /// for it in the same `bingeGroup`; otherwise its stream picker opens).
    pub binge_watching: bool,
    /// How many seconds before the end the Up Next card shows (5–60).
    pub up_next_seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mpv_path: None,
            engine_listen_port: 4240,
            engine_upnp: true,
            upload_limit_kibps: None,
            download_limit_kibps: None,
            delete_on_stop: true,
            theme: "harbor-grey".to_string(),
            instant_play: false,
            hide_anime: false,
            hide_live_tv: false,
            show_adult: false,
            home_rows: HomeRows::default(),
            builtin_cinemeta: true,
            builtin_kitsu: true,
            builtin_opensubtitles: true,
            auto_update: true,
            player_backend: PlayerBackend::Auto,
            subtitles: SubtitleSettings::default(),
            audio: AudioSettings::default(),
            skip: SkipSettings::default(),
            binge_watching: true,
            up_next_seconds: 15,
        }
    }
}

impl Serialize for Settings {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Settings::serialize(self, serializer)
    }
}

/// Lenient read: any JSON object is accepted. Unknown keys are ignored, and a key whose
/// value has the wrong type keeps that setting's default (with a warning) instead of
/// failing the whole file, which would move all of settings.json aside.
impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let object = Map::<String, Value>::deserialize(deserializer)?;
        let has_kitsu = object.contains_key("builtinKitsu");
        let has_opensubtitles = object.contains_key("builtinOpensubtitles");
        let mut settings = Settings::default();
        for (key, value) in object {
            match with_key(&settings, &key, value) {
                Ok(next) => settings = next,
                Err(KeyError::Unknown) => {}
                Err(KeyError::Invalid(e)) => {
                    warn!(key = %key, error = %e, "invalid stored setting; using its default");
                }
            }
        }
        // A file from before these built-ins existed: someone who had switched off the
        // only built-in (Cinemeta) doesn't get two new third-party hosts switched on by
        // an update, and someone who hid the Anime room doesn't get anime rows.
        if !has_kitsu {
            settings.builtin_kitsu = settings.builtin_cinemeta && !settings.hide_anime;
        }
        if !has_opensubtitles {
            settings.builtin_opensubtitles = settings.builtin_cinemeta;
        }
        Ok(settings)
    }
}

impl SettingsObject for Settings {
    fn to_object(&self) -> Map<String, Value> {
        match Settings::serialize(self, serde_json::value::Serializer) {
            Ok(Value::Object(object)) => object,
            _ => unreachable!("Settings serializes to a JSON object"),
        }
    }

    fn from_object(object: Map<String, Value>) -> Result<Self, serde_json::Error> {
        let settings = Settings::deserialize(Value::Object(object))?;
        if !(5..=60).contains(&settings.up_next_seconds) {
            return Err(serde::de::Error::custom("upNextSeconds must be 5 to 60"));
        }
        Ok(settings)
    }
}

impl Settings {
    /// Merge a partial update (top-level camelCase keys) into a copy of these settings.
    /// A group's value (`subtitles`, `audio`) is merged into the group, so it names
    /// only the fields it changes. Strict: an unknown key, or a wrongly typed or
    /// out-of-range value at any depth, rejects the whole patch, so a buggy caller
    /// never silently resets a setting.
    pub fn patched(&self, patch: Value) -> Result<Settings, IpcError> {
        let Value::Object(patch) = patch else {
            return Err(IpcError::InvalidInput(
                "A settings update must be an object of setting names to values.".into(),
            ));
        };
        let mut next = self.clone();
        for (key, value) in patch {
            let wanted = value.clone();
            next = with_key(&next, &key, value).map_err(|e| match e {
                KeyError::Unknown => {
                    IpcError::InvalidInput(format!("\"{key}\" is not a known setting."))
                }
                KeyError::Invalid(e) => IpcError::InvalidInput(format!(
                    "The value for the setting \"{key}\" is invalid: {e}."
                )),
            })?;
            // a group reads leniently, so whatever it dropped was invalid
            if !next
                .to_object()
                .get(&key)
                .is_some_and(|stored| survives(&wanted, stored))
            {
                return Err(IpcError::InvalidInput(format!(
                    "The value for the setting \"{key}\" is invalid: {wanted}."
                )));
            }
        }
        Ok(next)
    }
}

/// Bundle identifier before the Sailor → Zeiler rename. Tauri keys the data dir on the
/// identifier, so without a migration an updated install starts with no addons.
const LEGACY_IDENTIFIER: &str = "app.sailor.desktop";
const MIGRATED_FILES: [&str; 2] = ["settings.json", "addons.json"];

/// One-time copy of settings and addons from the pre-rename data dir (a sibling of
/// `data_dir`) when this install has neither yet. Copies rather than moves, so a
/// still-installed Sailor keeps working. Best effort: failures only log.
async fn migrate_legacy_data(data_dir: &Path) {
    let Some(legacy) = data_dir.parent().map(|p| p.join(LEGACY_IDENTIFIER)) else {
        return;
    };
    for name in MIGRATED_FILES {
        if tokio::fs::try_exists(data_dir.join(name))
            .await
            .unwrap_or(true)
        {
            return;
        }
    }
    for name in MIGRATED_FILES {
        let from = legacy.join(name);
        if !tokio::fs::try_exists(&from).await.unwrap_or(false) {
            continue;
        }
        match tokio::fs::copy(&from, data_dir.join(name)).await {
            Ok(_) => info!("migrated {name} from {}", legacy.display()),
            Err(e) => warn!("could not migrate {}: {e}", from.display()),
        }
    }
}

pub struct AppState {
    pub client: AddonClient,
    pub addons: RwLock<Vec<Descriptor>>,
    pub settings: RwLock<Settings>,
    pub player: Player,
    engine: OnceCell<Arc<Engine>>,
    /// Serializes all persist operations: concurrent writers would otherwise race on
    /// the same temp file and rename stale state over fresh state.
    persist_lock: tokio::sync::Mutex<()>,
    data_dir: PathBuf,
    cache_dir: PathBuf,
}

impl AppState {
    pub async fn load(data_dir: PathBuf, cache_dir: PathBuf) -> anyhow::Result<Self> {
        tokio::fs::create_dir_all(&data_dir).await?;
        tokio::fs::create_dir_all(&cache_dir).await?;
        migrate_legacy_data(&data_dir).await;

        let settings: Settings = read_json(&data_dir.join("settings.json")).await?;
        let addons: Vec<Descriptor> = read_json(&data_dir.join("addons.json")).await?;
        let player = Player::new(settings.mpv_path.as_deref().map(std::path::Path::new));

        Ok(Self {
            client: AddonClient::new()?,
            addons: RwLock::new(addons),
            settings: RwLock::new(settings),
            player,
            engine: OnceCell::new(),
            persist_lock: tokio::sync::Mutex::new(()),
            data_dir,
            cache_dir,
        })
    }

    /// The addons in effect: enabled built-ins first (each skipped when the user
    /// installed the same addon: same transport URL or manifest id), then the user's
    /// addons in install order.
    pub async fn effective_addons(&self) -> Vec<Descriptor> {
        let user = self.addons.read().await.clone();
        let settings = self.settings.read().await.clone();
        crate::builtin::with_builtins(user, &settings)
    }

    /// Where per-profile files (library, profile preferences, …) live. Until profiles
    /// exist this is the data dir itself; once they do it becomes
    /// `data_dir/profiles/<active id>/`, so new per-user stores must take their path
    /// from here rather than from the data dir. Device-wide files (settings.json,
    /// engine state) stay in the data dir.
    pub fn profile_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    /// The torrent engine, started on first use (avoids a firewall prompt at app launch).
    pub async fn engine(&self) -> Result<Arc<Engine>, IpcError> {
        self.engine
            .get_or_try_init(|| async {
                let settings = self.settings.read().await.clone();
                let config = EngineConfig {
                    cache_dir: self.cache_dir.join("torrents"),
                    state_dir: self.data_dir.join("engine"),
                    listen_port: settings.engine_listen_port,
                    enable_upnp: settings.engine_upnp,
                    upload_limit_bps: settings.upload_limit_kibps.map(|k| k.saturating_mul(1024)),
                    download_limit_bps: settings
                        .download_limit_kibps
                        .map(|k| k.saturating_mul(1024)),
                };
                Engine::new(config)
                    .await
                    .map(Arc::new)
                    .map_err(|e| IpcError::Engine(format!("{e:#}")))
            })
            .await
            .cloned()
    }

    pub fn engine_if_running(&self) -> Option<Arc<Engine>> {
        self.engine.get().cloned()
    }

    pub async fn persist_addons(&self) -> Result<(), IpcError> {
        let _guard = self.persist_lock.lock().await;
        let snapshot = self.addons.read().await.clone();
        write_json(&self.data_dir.join("addons.json"), &snapshot).await
    }

    /// Compute the next settings from the current ones, persist them, and only then
    /// publish them in memory, like `JsonStore::update`: if the write fails, memory is
    /// left unchanged, so it never runs ahead of disk. The settings write lock is held
    /// throughout, so concurrent updates serialize; `on_commit` (side effects such as
    /// relocating the player, and the change event) runs under it after the write.
    pub async fn update_settings(
        &self,
        next: impl FnOnce(&Settings) -> Result<Settings, IpcError>,
        on_commit: impl FnOnce(&Settings),
    ) -> Result<Settings, IpcError> {
        let mut current = self.settings.write().await;
        let next = next(&current)?;
        {
            let _guard = self.persist_lock.lock().await;
            write_json(&self.data_dir.join("settings.json"), &next).await?;
        }
        *current = next.clone();
        on_commit(&next);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    #[test]
    fn settings_round_trip_as_camel_case() {
        let settings = Settings {
            mpv_path: Some("/usr/bin/mpv".into()),
            upload_limit_kibps: Some(512),
            ..Settings::default()
        };
        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value["mpvPath"], "/usr/bin/mpv");
        assert_eq!(value["builtinCinemeta"], true);
        let back: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn missing_keys_take_their_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"theme":"ocean"}"#).unwrap();
        assert_eq!(settings.theme, "ocean");
        assert_eq!(settings.engine_listen_port, 4240);
        assert!(settings.builtin_cinemeta);
    }

    #[test]
    fn a_wrong_typed_key_falls_back_to_its_default_and_keeps_the_rest() {
        let settings: Settings = serde_json::from_value(json!({
            "theme": "ocean",
            "engineListenPort": "not a port",
            "builtinCinemeta": "yes",
            "deleteOnStop": false,
            "mpvPath": 42,
            "someFutureKey": {"nested": true}
        }))
        .unwrap();
        assert_eq!(settings.theme, "ocean");
        assert!(!settings.delete_on_stop);
        let defaults = Settings::default();
        assert_eq!(settings.engine_listen_port, defaults.engine_listen_port);
        assert_eq!(settings.builtin_cinemeta, defaults.builtin_cinemeta);
        assert_eq!(settings.mpv_path, None);
    }

    #[test]
    fn player_backend_defaults_to_auto_and_an_unknown_value_resets_only_itself() {
        assert_eq!(Settings::default().player_backend, PlayerBackend::Auto);
        let stored: Settings = serde_json::from_value(json!({
            "theme": "ocean",
            "playerBackend": "vlc",
            "hideAnime": true
        }))
        .unwrap();
        assert_eq!(stored.player_backend, PlayerBackend::Auto);
        assert_eq!(stored.theme, "ocean");
        assert!(stored.hide_anime);

        let external: Settings =
            serde_json::from_value(json!({"playerBackend": "external"})).unwrap();
        assert_eq!(external.player_backend, PlayerBackend::External);
        assert_eq!(
            serde_json::to_value(&external).unwrap()["playerBackend"],
            "external"
        );

        let patched = Settings::default()
            .patched(json!({"playerBackend": "embedded"}))
            .unwrap();
        assert_eq!(patched.player_backend, PlayerBackend::Embedded);
        assert!(
            Settings::default()
                .patched(json!({"playerBackend": "vlc"}))
                .is_err(),
            "patch stays strict"
        );
    }

    #[test]
    fn builtin_toggles_default_on_and_are_lenient_per_key() {
        let defaults = Settings::default();
        assert!(defaults.builtin_kitsu && defaults.builtin_opensubtitles);
        let value = serde_json::to_value(&defaults).unwrap();
        assert_eq!(value["builtinKitsu"], true);
        assert_eq!(value["builtinOpensubtitles"], true);

        // a settings.json from before these keys existed: on, unless its owner had
        // switched the only built-in off (or hid the Anime room, for Kitsu)
        let old: Settings = serde_json::from_value(json!({"builtinCinemeta": true})).unwrap();
        assert!(old.builtin_kitsu && old.builtin_opensubtitles);
        let empty: Settings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty, defaults);
        let opted_out: Settings =
            serde_json::from_value(json!({"builtinCinemeta": false})).unwrap();
        assert!(!opted_out.builtin_kitsu && !opted_out.builtin_opensubtitles);
        let no_anime: Settings = serde_json::from_value(json!({"hideAnime": true})).unwrap();
        assert!(!no_anime.builtin_kitsu);
        assert!(no_anime.builtin_opensubtitles);
        // once stored, a key is the user's own choice
        let chosen: Settings = serde_json::from_value(json!({
            "builtinCinemeta": false,
            "hideAnime": true,
            "builtinKitsu": true,
            "builtinOpensubtitles": true
        }))
        .unwrap();
        assert!(chosen.builtin_kitsu && chosen.builtin_opensubtitles);

        // a bad value resets only that toggle
        let stored: Settings = serde_json::from_value(json!({
            "builtinKitsu": "off",
            "builtinOpensubtitles": false
        }))
        .unwrap();
        assert!(stored.builtin_kitsu, "bad value → default");
        assert!(!stored.builtin_opensubtitles);

        let patched = defaults
            .patched(json!({"builtinKitsu": false, "builtinOpensubtitles": false}))
            .unwrap();
        assert!(!patched.builtin_kitsu && !patched.builtin_opensubtitles);
        assert!(patched.builtin_cinemeta);
        assert!(defaults.patched(json!({"builtinKitsu": 0})).is_err());
    }

    #[test]
    fn subtitle_and_audio_groups_read_per_field_and_patch_one_field_at_a_time() {
        let stored: Settings = serde_json::from_value(json!({
            "theme": "ocean",
            "subtitles": {"langs": ["ar"], "autoLoad": false, "style": {"scale": 9, "bold": true}},
            "audio": {"langs": ["en"], "animeLangs": "ja"}
        }))
        .unwrap();
        assert_eq!(stored.theme, "ocean");
        assert_eq!(stored.subtitles.langs, ["ar"]);
        assert!(!stored.subtitles.auto_load);
        assert_eq!(stored.subtitles.style.scale, None, "out of range → unset");
        assert_eq!(stored.subtitles.style.bold, Some(true));
        assert_eq!(stored.audio.langs, ["en"]);
        assert_eq!(stored.audio.anime_langs, ["ja"], "wrong type → default");

        // one style field, without the rest of the group: merged, the rest kept
        let patched = stored
            .patched(json!({"subtitles": {"style": {"scale": 1.25}}}))
            .unwrap();
        assert_eq!(patched.subtitles.style.scale, Some(1.25));
        assert_eq!(patched.subtitles.style.bold, Some(true));
        assert_eq!(patched.subtitles.langs, ["ar"]);
        assert!(!patched.subtitles.auto_load);
        // null unsets a field; a list is replaced, not merged
        let patched = patched
            .patched(json!({"subtitles": {"style": {"bold": null}, "langs": ["en", "fr"]}}))
            .unwrap();
        assert_eq!(patched.subtitles.style.bold, None);
        assert_eq!(patched.subtitles.style.scale, Some(1.25));
        assert_eq!(patched.subtitles.langs, ["en", "fr"]);

        // strict at any depth: an unknown field, a wrong type, a value mpv refuses
        for bad in [
            json!({"subtitles": {"stlye": {"scale": 1}}}),
            json!({"subtitles": {"style": {"scale": "big"}}}),
            json!({"subtitles": {"style": {"scale": 9}}}),
            json!({"subtitles": {"style": {"font": "/tmp/evil.ttf"}}}),
            json!({"subtitles": {"style": {"color": "red"}}}),
            json!({"subtitles": {"langs": ["en", "en;fr"]}}),
            json!({"subtitles": {"autoLoad": 1}}),
            json!({"subtitles": 5}),
            json!({"audio": {"langs": "en"}}),
        ] {
            let result = patched.patched(bad.clone());
            assert!(
                matches!(&result, Err(IpcError::InvalidInput(m)) if m.contains('"')),
                "{bad} must be refused: {result:?}"
            );
        }
        // a refused patch changes nothing, even its valid keys
        assert!(
            patched
                .patched(json!({"theme": "dusk", "subtitles": {"autoLoad": "x"}}))
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_settings_file_with_one_bad_field_is_not_moved_aside() {
        let dir = crate::persist::tests::temp_dir("settings-lenient");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"theme":"ocean","autoUpdate":"sometimes"}"#).unwrap();

        let settings: Settings = read_json(&path).await.unwrap();

        assert_eq!(settings.theme, "ocean");
        assert!(settings.auto_update, "bad value → that setting's default");
        assert!(path.exists());
        assert!(!dir.join("settings.json.corrupt.bak").exists());
    }

    #[test]
    fn patch_merges_only_the_given_keys() {
        let current = Settings {
            theme: "ocean".into(),
            hide_anime: true,
            ..Settings::default()
        };
        let next = current
            .patched(json!({"instantPlay": true, "mpvPath": "/opt/mpv"}))
            .unwrap();
        assert!(next.instant_play);
        assert_eq!(next.mpv_path.as_deref(), Some("/opt/mpv"));
        assert_eq!(next.theme, "ocean", "untouched keys keep their value");
        assert!(next.hide_anime);

        let cleared = next.patched(json!({"mpvPath": null})).unwrap();
        assert_eq!(cleared.mpv_path, None);
    }

    #[test]
    fn skip_and_binge_settings_default_and_check_their_range() {
        let defaults = Settings::default();
        assert!(defaults.binge_watching);
        assert_eq!(defaults.up_next_seconds, 15);
        assert!(!defaults.skip.auto_skip_intro && defaults.skip.online_lookup);
        let next = defaults
            .patched(json!({"skip": {"autoSkipIntro": true}, "upNextSeconds": 30}))
            .unwrap();
        assert!(next.skip.auto_skip_intro);
        assert!(next.skip.show_button, "the rest of the group stays");
        assert_eq!(next.up_next_seconds, 30);
        for bad in [
            json!({"upNextSeconds": 0}),
            json!({"upNextSeconds": 600}),
            json!({"skip": {"autoSkipIntro": "yes"}}),
            json!({"bingeWatching": 1}),
        ] {
            assert!(defaults.patched(bad.clone()).is_err(), "{bad}");
        }
        let stored: Settings =
            serde_json::from_value(json!({"upNextSeconds": 999, "bingeWatching": false})).unwrap();
        assert_eq!(stored.up_next_seconds, 15, "out of range → its default");
        assert!(!stored.binge_watching);
    }

    #[test]
    fn patch_rejects_unknown_keys_wrong_types_and_non_objects() {
        let current = Settings::default();
        let unknown = current.patched(json!({"themes": "ocean"})).unwrap_err();
        assert!(matches!(&unknown, IpcError::InvalidInput(m) if m.contains("\"themes\"")));
        let wrong = current
            .patched(json!({"theme": "ocean", "engineListenPort": 70000}))
            .unwrap_err();
        assert!(matches!(&wrong, IpcError::InvalidInput(m) if m.contains("engineListenPort")));
        assert!(current.patched(json!(["theme"])).is_err());
    }

    #[tokio::test]
    async fn settings_update_persists_before_publishing() {
        let dir = crate::persist::tests::temp_dir("settings-update");
        let state = AppState::load(dir.join("data"), dir.join("cache"))
            .await
            .unwrap();

        let ok = state
            .update_settings(|s| s.patched(json!({"hideAnime": true})), |_| {})
            .await
            .unwrap();
        assert!(ok.hide_anime);
        let on_disk: Settings = read_json(&dir.join("data/settings.json")).await.unwrap();
        assert!(on_disk.hide_anime);

        // a directory where the temp file should go makes the write fail
        std::fs::create_dir_all(dir.join("data/settings.json.tmp")).unwrap();
        let mut committed = false;
        let failed = state
            .update_settings(
                |s| s.patched(json!({"builtinCinemeta": false})),
                |_| committed = true,
            )
            .await;

        assert!(matches!(failed, Err(IpcError::Storage(_))));
        assert!(!committed, "no side effects or event after a failed write");
        let in_memory = state.settings.read().await.clone();
        assert!(
            in_memory.builtin_cinemeta,
            "memory never runs ahead of disk"
        );
        assert!(in_memory.hide_anime);
    }

    #[tokio::test]
    async fn profile_dir_is_the_data_dir_until_profiles_exist() {
        let dir = crate::persist::tests::temp_dir("profile-dir");
        let state = AppState::load(dir.join("data"), dir.join("cache"))
            .await
            .unwrap();
        assert_eq!(state.profile_dir(), dir.join("data"));
    }

    /// `<tmp>/<test>/{app.sailor.desktop,io.zeiler.desktop}`, mirroring the OS data dir.
    fn data_dirs(test: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("zeiler-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let legacy = root.join(LEGACY_IDENTIFIER);
        let current = root.join("io.zeiler.desktop");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&current).unwrap();
        (legacy, current)
    }

    #[tokio::test]
    async fn migrates_sailor_data_into_an_empty_install() {
        let (legacy, current) = data_dirs("migrate-empty");
        std::fs::write(legacy.join("settings.json"), r#"{"theme":"ocean"}"#).unwrap();
        std::fs::write(legacy.join("addons.json"), "[]").unwrap();

        migrate_legacy_data(&current).await;

        let settings = std::fs::read_to_string(current.join("settings.json")).unwrap();
        assert_eq!(settings, r#"{"theme":"ocean"}"#);
        assert!(current.join("addons.json").exists());
        assert!(legacy.join("addons.json").exists(), "copied, not moved");
    }

    #[tokio::test]
    async fn leaves_an_existing_install_alone() {
        let (legacy, current) = data_dirs("migrate-existing");
        std::fs::write(legacy.join("settings.json"), r#"{"theme":"ocean"}"#).unwrap();
        std::fs::write(legacy.join("addons.json"), "[]").unwrap();
        std::fs::write(current.join("settings.json"), "{}").unwrap();

        migrate_legacy_data(&current).await;

        assert_eq!(
            std::fs::read_to_string(current.join("settings.json")).unwrap(),
            "{}"
        );
        assert!(!current.join("addons.json").exists());
    }
}
