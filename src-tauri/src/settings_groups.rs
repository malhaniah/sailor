//! The nested settings groups — `subtitles{}`, `audio{}` and `skip{}` — and the per-key lenient
//! reading they share with [`Settings`](crate::state::Settings).
//!
//! Every settings object (the top level and each group) is read the same way: start
//! from its defaults, apply each stored key on its own, strictly typed and checked, and
//! keep the default of a key whose value does not fit (with a warning). One bad
//! sub-field therefore resets only itself, never its group or the whole file (which
//! `read_json` would move aside). The derives generate the inherent, strict,
//! whole-object forms (`remote = "Self"`); [`settings_object!`] writes the lenient
//! trait impls around them.
//!
//! `settings_patch` merges an object value into the group it names, key by key, so the
//! player can change one style field without sending (or racing) the rest of the group.
//! It stays strict: a patch is refused unless every value it names survives the read
//! unchanged ([`survives`]), so a typo or an out-of-range value never silently resets
//! a setting.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::warn;

use crate::commands::mpv::validate::validate_property;

/// A settings object read leniently, key by key.
pub(crate) trait SettingsObject: Default + Clone {
    /// The strict, whole-object serialization.
    fn to_object(&self) -> Map<String, Value>;
    /// The strict, whole-object read: types and value rules.
    fn from_object(object: Map<String, Value>) -> Result<Self, serde_json::Error>;
}

pub(crate) enum KeyError {
    Unknown,
    Invalid(serde_json::Error),
}

/// A copy of `current` with `key` set to `value`. An object value is merged into the
/// key's current object, key by key (arrays and scalars replace). Checked strictly at
/// this level; nested groups read leniently.
pub(crate) fn with_key<T: SettingsObject>(
    current: &T,
    key: &str,
    value: Value,
) -> Result<T, KeyError> {
    let mut object = current.to_object();
    let Some(slot) = object.get_mut(key) else {
        return Err(KeyError::Unknown);
    };
    merge(slot, value);
    T::from_object(object).map_err(KeyError::Invalid)
}

fn merge(slot: &mut Value, value: Value) {
    match (slot, value) {
        (Value::Object(slot), Value::Object(patch)) => {
            for (key, value) in patch {
                match slot.get_mut(&key) {
                    Some(inner) => merge(inner, value),
                    None => {
                        slot.insert(key, value);
                    }
                }
            }
        }
        (slot, value) => *slot = value,
    }
}

/// The lenient read: defaults, then each stored key that fits.
pub(crate) fn read_lenient<T: SettingsObject>(object: Map<String, Value>, group: &str) -> T {
    let mut current = T::default();
    for (key, value) in object {
        match with_key(&current, &key, value) {
            Ok(next) => current = next,
            Err(KeyError::Unknown) => {}
            Err(KeyError::Invalid(e)) => {
                warn!(%group, key = %key, error = %e, "invalid stored setting; using its default");
            }
        }
    }
    current
}

/// Read `value`, an object naming some of `T`'s fields, as strictly as a settings
/// patch: every value it names must be valid (at any depth). The others keep their
/// defaults.
pub(crate) fn parse_strict<T: SettingsObject>(value: &Value) -> Result<T, String> {
    let Value::Object(object) = value else {
        return Err("it is not an object".into());
    };
    let mut parsed = T::default();
    for (key, value) in object {
        parsed = with_key(&parsed, key, value.clone()).map_err(|e| match e {
            KeyError::Unknown => format!("\"{key}\" is not a known field"),
            KeyError::Invalid(e) => format!("\"{key}\": {e}"),
        })?;
        if !parsed
            .to_object()
            .get(key)
            .is_some_and(|stored| survives(value, stored))
        {
            return Err(format!("\"{key}\" has an invalid value"));
        }
    }
    Ok(parsed)
}

/// Whether every value `patch` names came through a read unchanged in `stored` (the
/// patched settings, serialized). Numbers compare by value (`2` and `2.0` agree).
pub(crate) fn survives(patch: &Value, stored: &Value) -> bool {
    match (patch, stored) {
        (Value::Object(patch), Value::Object(stored)) => patch
            .iter()
            .all(|(key, value)| stored.get(key).is_some_and(|s| survives(value, s))),
        (Value::Array(patch), Value::Array(stored)) => {
            patch.len() == stored.len() && patch.iter().zip(stored).all(|(p, s)| survives(p, s))
        }
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (a, b) => a == b,
    }
}

/// Lenient `Deserialize`, strict-form `Serialize` and [`SettingsObject`] for a group
/// type with `remote = "Self"` derives and a `check(&self) -> Result<(), String>`.
macro_rules! settings_object {
    ($ty:ident, $group:literal) => {
        impl Serialize for $ty {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $ty::serialize(self, serializer)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let object = Map::<String, Value>::deserialize(deserializer)?;
                Ok(read_lenient(object, $group))
            }
        }

        impl SettingsObject for $ty {
            fn to_object(&self) -> Map<String, Value> {
                match $ty::serialize(self, serde_json::value::Serializer) {
                    Ok(Value::Object(object)) => object,
                    _ => unreachable!("a settings group serializes to a JSON object"),
                }
            }

            fn from_object(object: Map<String, Value>) -> Result<Self, serde_json::Error> {
                let value = $ty::deserialize(Value::Object(object))?;
                value.check().map_err(serde::de::Error::custom)?;
                Ok(value)
            }
        }
    };
}

// usable by groups defined in other modules (`home_rows.rs`)
pub(crate) use settings_object;

/// Most languages a preference list holds.
const MAX_LANGUAGES: usize = 12;

/// A language as the settings store it: an ISO 639-1 or 639-2 code, optionally with
/// BCP-47 subtags (`pt-BR`, `zh-Hant`, `es-419`).
pub(crate) fn is_language_code(code: &str) -> bool {
    let mut parts = code.split('-');
    let primary = parts.next().unwrap_or("");
    code.len() <= 16
        && matches!(primary.len(), 2 | 3)
        && primary.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn check_languages(what: &str, langs: &[String]) -> Result<(), String> {
    if langs.len() > MAX_LANGUAGES {
        return Err(format!("at most {MAX_LANGUAGES} {what} languages"));
    }
    match langs.iter().find(|l| !is_language_code(l)) {
        Some(bad) => Err(format!("\"{bad}\" is not a language code")),
        None => Ok(()),
    }
}

/// `subtitles{}`: which subtitles to pick and how they look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", rename_all = "camelCase", default)]
pub struct SubtitleSettings {
    /// Preferred subtitle languages, most preferred first. Empty: the system's
    /// language, then English (decided by the UI, which knows the system language).
    pub langs: Vec<String>,
    /// When a file starts, show the best subtitle in the first preferred language that
    /// has one (an embedded track, else one from a subtitle addon).
    pub auto_load: bool,
    /// Zeiler's look for text subtitles; every field unset means mpv's own default (or,
    /// for external mpv, the user's mpv config).
    pub style: SubtitleStyle,
}

impl Default for SubtitleSettings {
    fn default() -> Self {
        Self {
            langs: Vec::new(),
            auto_load: true,
            style: SubtitleStyle::default(),
        }
    }
}

impl SubtitleSettings {
    fn check(&self) -> Result<(), String> {
        check_languages("subtitle", &self.langs)
    }
}

settings_object!(SubtitleSettings, "subtitles");

/// How text subtitles look. Each field is an mpv property value, checked against the
/// same rules as the player's property writes (`commands/mpv/validate.rs`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", rename_all = "camelCase", default)]
pub struct SubtitleStyle {
    /// `sub-scale` (1 = mpv's size).
    pub scale: Option<f64>,
    /// `sub-font-size`, in mpv's scaled pixels (55 by default; `scale` multiplies it).
    pub font_size: Option<f64>,
    /// `sub-font`, one of the fonts the player allows.
    pub font: Option<String>,
    pub bold: Option<bool>,
    /// `#RRGGBB` or `#AARRGGBB` (mpv's alpha comes first).
    pub color: Option<String>,
    pub outline_color: Option<String>,
    pub outline_size: Option<f64>,
    /// `outline-and-shadow`, `opaque-box` or `background-box`.
    pub border_style: Option<String>,
    pub back_color: Option<String>,
    pub shadow_offset: Option<f64>,
    /// `sub-shadow-color`, `#RRGGBB` or `#AARRGGBB`. mpv 0.38+ reads it as another name
    /// for `sub-back-color` (the shadow's colour, or the box's), so it applies only
    /// while `back_color` is unset.
    pub shadow_color: Option<String>,
    /// `sub-pos`: 100 is the bottom of the video, lower is higher up.
    pub pos: Option<f64>,
    pub margin_y: Option<f64>,
    /// `sub-align-x`: `left`, `center` or `right`.
    pub align_x: Option<String>,
    /// `sub-align-y`: `top`, `center` or `bottom`.
    pub align_y: Option<String>,
    /// `sub-ass-override`; `force` applies this style to styled (ASS) subtitles too,
    /// which breaks their typesetting, so it is opt-in.
    pub ass_override: Option<String>,
}

impl SubtitleStyle {
    /// Each field that is set, as the mpv properties that take it (the first one the
    /// running mpv knows is used: mpv 0.38 renamed `sub-border-*` to `sub-outline-*`)
    /// and its value.
    pub fn properties(&self) -> Vec<(&'static [&'static str], Value)> {
        let mut out: Vec<(&'static [&'static str], Value)> = Vec::new();
        let mut put = |names: &'static [&'static str], value: Option<Value>| {
            if let Some(value) = value {
                out.push((names, value));
            }
        };
        let text = |s: &Option<String>| s.clone().map(Value::from);
        let number = |n: Option<f64>| n.map(Value::from);
        put(&["sub-scale"], number(self.scale));
        put(&["sub-font-size"], number(self.font_size));
        put(&["sub-font"], text(&self.font));
        put(&["sub-bold"], self.bold.map(Value::from));
        put(&["sub-color"], text(&self.color));
        put(
            &["sub-outline-color", "sub-border-color"],
            text(&self.outline_color),
        );
        put(
            &["sub-outline-size", "sub-border-size"],
            number(self.outline_size),
        );
        put(&["sub-border-style"], text(&self.border_style));
        put(&["sub-back-color"], text(&self.back_color));
        put(&["sub-shadow-offset"], number(self.shadow_offset));
        if self.back_color.is_none() {
            put(&["sub-shadow-color"], text(&self.shadow_color));
        }
        put(&["sub-pos"], number(self.pos));
        put(&["sub-margin-y"], number(self.margin_y));
        put(&["sub-align-x"], text(&self.align_x));
        put(&["sub-align-y"], text(&self.align_y));
        put(&["sub-ass-override"], text(&self.ass_override));
        out
    }

    fn check(&self) -> Result<(), String> {
        for (names, value) in self.properties() {
            validate_property(names[0], &value)?;
        }
        Ok(())
    }
}

settings_object!(SubtitleStyle, "subtitles.style");

/// `audio{}`: which audio track to prefer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", rename_all = "camelCase", default)]
pub struct AudioSettings {
    /// Preferred audio languages, most preferred first. Empty: the file's default track.
    pub langs: Vec<String>,
    /// Preferred audio languages for anime (Kitsu, MyAnimeList, AniList and AniDB
    /// titles), used instead of `langs`. Empty: the file's default track.
    pub anime_langs: Vec<String>,
    /// Even out the volume (a labelled `dynaudnorm` filter, `commands/mpv/audio.rs`).
    pub normalize: bool,
    /// The loudest the volume goes, in percent (mpv's `volume-max`, 100–200).
    pub volume_max: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            langs: Vec::new(),
            anime_langs: vec!["ja".into()],
            normalize: false,
            volume_max: 130,
        }
    }
}

impl AudioSettings {
    fn check(&self) -> Result<(), String> {
        check_languages("audio", &self.langs)?;
        check_languages("anime audio", &self.anime_langs)?;
        if !(100..=200).contains(&self.volume_max) {
            return Err("volumeMax must be 100 to 200".into());
        }
        Ok(())
    }
}

settings_object!(AudioSettings, "audio");

/// `skip{}`: skipping intros, recaps and credits (the player's skip button, and
/// skipping by itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", rename_all = "camelCase", default)]
pub struct SkipSettings {
    /// Show a Skip button while an intro, recap or credits play.
    pub show_button: bool,
    /// Skip these by themselves (with an Undo). Off by default: a segment can be
    /// wrong for the cut being played.
    pub auto_skip_intro: bool,
    pub auto_skip_recap: bool,
    pub auto_skip_credits: bool,
    /// Ask AniSkip (anime, through a Kitsu id mapping) and TheIntroDB (IMDb and TMDB
    /// titles) where they are. Off: only the file's own chapter names count.
    pub online_lookup: bool,
}

impl Default for SkipSettings {
    fn default() -> Self {
        Self {
            show_button: true,
            auto_skip_intro: false,
            auto_skip_recap: false,
            auto_skip_credits: false,
            online_lookup: true,
        }
    }
}

impl SkipSettings {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

settings_object!(SkipSettings, "skip");

/// `marketplaceSources{}`: which directories the opt-in addon marketplace lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self", rename_all = "camelCase", default)]
pub struct MarketplaceSources {
    /// The stremio-addons.net community directory (stars, categories, documentation).
    pub stremio_addons_net: bool,
    /// Stremio's community and official addon catalogs, served by Cinemeta.
    pub stremio_catalog: bool,
}

impl Default for MarketplaceSources {
    fn default() -> Self {
        Self {
            stremio_addons_net: true,
            stremio_catalog: true,
        }
    }
}

impl MarketplaceSources {
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

settings_object!(MarketplaceSources, "marketplaceSources");

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn groups_default_to_mpvs_own_choices() {
        let subtitles = SubtitleSettings::default();
        assert!(subtitles.langs.is_empty() && subtitles.auto_load);
        assert!(
            subtitles.style.properties().is_empty(),
            "no style is forced"
        );
        assert_eq!(
            serde_json::to_value(&subtitles).unwrap(),
            json!({
                "langs": [],
                "autoLoad": true,
                "style": {
                    "scale": null, "fontSize": null, "font": null, "bold": null,
                    "color": null, "outlineColor": null, "outlineSize": null,
                    "borderStyle": null, "backColor": null, "shadowOffset": null,
                    "shadowColor": null, "pos": null, "marginY": null, "alignX": null,
                    "alignY": null, "assOverride": null
                }
            })
        );
        assert_eq!(
            serde_json::to_value(AudioSettings::default()).unwrap(),
            json!({"langs": [], "animeLangs": ["ja"], "normalize": false, "volumeMax": 130})
        );
    }

    #[test]
    fn skipping_by_itself_is_off_by_default() {
        assert_eq!(
            serde_json::to_value(SkipSettings::default()).unwrap(),
            json!({
                "showButton": true,
                "autoSkipIntro": false,
                "autoSkipRecap": false,
                "autoSkipCredits": false,
                "onlineLookup": true
            })
        );
        let stored: SkipSettings =
            serde_json::from_value(json!({"autoSkipIntro": true, "onlineLookup": "no"})).unwrap();
        assert!(stored.auto_skip_intro);
        assert!(stored.online_lookup, "bad value → its default");
    }

    #[test]
    fn one_bad_field_resets_only_itself() {
        let stored: SubtitleSettings = serde_json::from_value(json!({
            "langs": ["ar", "en"],
            "autoLoad": "sometimes",
            "style": {
                "scale": 1.25,
                "color": "yellow",
                "pos": 500,
                "font": "Comic Sans MS",
                "bold": true,
                "borderStyle": "background-box",
                "outlineSize": "2"
            },
            "futureKey": 1
        }))
        .unwrap();
        assert_eq!(stored.langs, ["ar", "en"]);
        assert!(stored.auto_load, "bad value → its default (true)");
        let style = &stored.style;
        assert_eq!(style.scale, Some(1.25));
        assert_eq!(style.bold, Some(true));
        assert_eq!(style.border_style.as_deref(), Some("background-box"));
        assert_eq!(style.color, None, "not a color");
        assert_eq!(style.pos, None, "out of mpv's range");
        assert_eq!(style.font, None, "not an allowed font");
        assert_eq!(style.outline_size, None, "wrong type");

        // a group that is not an object at all: all defaults
        let bad: SubtitleSettings =
            serde_json::from_value(json!({"style": 5, "langs": "en"})).unwrap();
        assert_eq!(bad, SubtitleSettings::default());
        // a language list with one bad code is refused as a whole
        let langs: AudioSettings =
            serde_json::from_value(json!({"langs": ["en", "not a code"], "animeLangs": []}))
                .unwrap();
        assert!(langs.langs.is_empty());
        assert!(
            langs.anime_langs.is_empty(),
            "an explicit empty list is kept"
        );
    }

    #[test]
    fn style_fields_map_to_mpv_properties_with_the_old_names_as_fallbacks() {
        let style = SubtitleStyle {
            scale: Some(1.5),
            outline_size: Some(2.0),
            outline_color: Some("#FF000000".into()),
            ass_override: Some("force".into()),
            ..SubtitleStyle::default()
        };
        assert_eq!(
            style.properties(),
            vec![
                (&["sub-scale"][..], json!(1.5)),
                (
                    &["sub-outline-color", "sub-border-color"][..],
                    json!("#FF000000")
                ),
                (&["sub-outline-size", "sub-border-size"][..], json!(2.0)),
                (&["sub-ass-override"][..], json!("force")),
            ]
        );
    }

    /// mpv 0.38+ reads `sub-shadow-color` as another name for `sub-back-color`: with a
    /// box colour set, a shadow colour would replace it.
    #[test]
    fn a_shadow_colour_never_replaces_the_box_colour() {
        let shadow = SubtitleStyle {
            shadow_color: Some("#80000000".into()),
            shadow_offset: Some(1.5),
            ..SubtitleStyle::default()
        };
        assert!(
            shadow
                .properties()
                .iter()
                .any(|(names, _)| names[0] == "sub-shadow-color")
        );
        let boxed = SubtitleStyle {
            back_color: Some("#99000000".into()),
            ..shadow
        };
        let names: Vec<_> = boxed.properties().iter().map(|(n, _)| n[0]).collect();
        assert!(names.contains(&"sub-back-color"));
        assert!(!names.contains(&"sub-shadow-color"));
    }

    #[test]
    fn language_codes() {
        for ok in ["en", "eng", "pt-BR", "zh-Hant", "es-419", "fil"] {
            assert!(is_language_code(ok), "{ok}");
        }
        for bad in [
            "",
            "e",
            "english",
            "en_US",
            "en-",
            "1a",
            "en,fr",
            "pt-BRAZILIANS",
            "ar\n",
        ] {
            assert!(!is_language_code(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_partial_style_is_read_strictly() {
        let style: SubtitleStyle = parse_strict(&json!({"scale": 1.5, "pos": 90})).unwrap();
        assert_eq!(style.scale, Some(1.5));
        assert_eq!(style.pos, Some(90.0));
        assert_eq!(style.color, None);
        for bad in [
            json!({"scale": 99}),
            json!({"scale": "1"}),
            json!({"colour": "#FFFFFF"}),
            json!({"font": "C:\\evil.ttf"}),
            json!("scale"),
        ] {
            assert!(parse_strict::<SubtitleStyle>(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn merged_values_must_survive() {
        assert!(survives(
            &json!({"style": {"scale": 2}}),
            &json!({"style": {"scale": 2.0, "pos": null}})
        ));
        assert!(!survives(
            &json!({"style": {"scale": 2}}),
            &json!({"style": {"scale": null}})
        ));
        assert!(
            !survives(&json!({"stlye": {}}), &json!({"style": {}})),
            "unknown key"
        );
        assert!(survives(&json!(["en"]), &json!(["en"])));
        assert!(!survives(&json!(["en", "x"]), &json!(["en"])));
        assert!(survives(&json!(null), &json!(null)));
        assert!(!survives(&json!(null), &json!({})));
    }
}
