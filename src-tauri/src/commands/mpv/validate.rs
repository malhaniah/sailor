//! What the webview may do to mpv, as data.
//!
//! The webview is our own code, but addon-controlled strings flow through it, and
//! mpv properties and commands reach the filesystem, the network and arbitrary
//! code (`run`, scripts, filters). So every property write, property read and
//! command from the UI is checked here against a table: the name must be listed and
//! every value must fit its rule. Anything else is refused with `invalidInput`.
//!
//! Never listed, by design: `loadfile`, `sub-add`, `af`, `vf`, `glsl-shaders`,
//! `run`, `script-message`, `keybind`, `define-section` and friends. Loading goes
//! through `player_start`/`player_load` (scheme-checked requests) and subtitles
//! through `player_subtitle_*` (`subtitles.rs`: only files Zeiler wrote); later
//! features that need the others get their own dedicated, validated commands.

use serde_json::Value;

/// How a settable property's value is checked.
#[derive(Debug, Clone, Copy)]
enum Rule {
    Bool,
    /// Inclusive numeric range.
    Range(f64, f64),
    /// `#RRGGBB` or `#AARRGGBB`.
    Color,
    Choice(&'static [&'static str]),
    /// A track id (integer ≥ 1, or its decimal string) or `no`/`auto`.
    Track,
    /// One of [`FONTS`].
    Font,
}

/// Properties the UI may set, and how each value is checked.
const SETTABLE: &[(&str, Rule)] = &[
    ("pause", Rule::Bool),
    ("mute", Rule::Bool),
    ("sub-visibility", Rule::Bool),
    ("secondary-sub-visibility", Rule::Bool),
    ("sub-bold", Rule::Bool),
    // the external mpv window's fullscreen (the Player screen's `f` key); embedded
    // video has no window of its own (a parented or render-API VO ignores it), so
    // the UI makes the Zeiler window fullscreen instead
    ("fullscreen", Rule::Bool),
    // mpv's own ceiling for volume-max; mpv itself rejects values above the
    // session's volume-max (130 by default)
    ("volume", Rule::Range(0.0, 1000.0)),
    ("speed", Rule::Range(0.25, 4.0)),
    ("sub-delay", Rule::Range(-600.0, 600.0)),
    ("audio-delay", Rule::Range(-600.0, 600.0)),
    ("sub-pos", Rule::Range(0.0, 150.0)),
    ("sub-scale", Rule::Range(0.3, 3.0)),
    ("sub-font-size", Rule::Range(10.0, 120.0)),
    ("sub-margin-y", Rule::Range(0.0, 200.0)),
    // mpv 0.38 renamed sub-border-* to sub-outline-*; both spellings are accepted
    // because the external mpv can be any version
    ("sub-border-size", Rule::Range(0.0, 10.0)),
    ("sub-outline-size", Rule::Range(0.0, 10.0)),
    ("sub-shadow-offset", Rule::Range(0.0, 10.0)),
    ("sub-color", Rule::Color),
    ("sub-border-color", Rule::Color),
    ("sub-outline-color", Rule::Color),
    ("sub-back-color", Rule::Color),
    ("sub-shadow-color", Rule::Color),
    (
        "sub-ass-override",
        Rule::Choice(&["no", "yes", "scale", "force", "strip"]),
    ),
    ("sub-align-x", Rule::Choice(&["left", "center", "right"])),
    ("sub-align-y", Rule::Choice(&["top", "center", "bottom"])),
    (
        "sub-border-style",
        Rule::Choice(&["outline-and-shadow", "opaque-box", "background-box"]),
    ),
    ("aid", Rule::Track),
    ("sid", Rule::Track),
    ("vid", Rule::Track),
    ("secondary-sid", Rule::Track),
    ("sub-font", Rule::Font),
];

/// Subtitle fonts the UI may pick: generic families plus fonts that ship with the
/// major desktop OSes. Settings offers the same list (src/lib/subtitle-style.ts; a
/// test checks they agree).
const FONTS: &[&str] = &[
    "sans-serif",
    "serif",
    "monospace",
    "Arial",
    "Helvetica",
    "Verdana",
    "Tahoma",
    "Trebuchet MS",
    "Segoe UI",
    "Georgia",
    "Times New Roman",
    "Courier New",
    "Noto Sans",
    "DejaVu Sans",
    "Liberation Sans",
    "Roboto",
];

/// Properties the UI may read on demand (besides the observed set, which it may
/// also read, e.g. to resync after a reload). Diagnostics only: nothing here exposes
/// paths, URLs or headers.
const READABLE: &[&str] = &[
    "video-codec",
    "audio-codec",
    "hwdec-current",
    "estimated-vf-fps",
    "container-fps",
    "video-bitrate",
    "audio-bitrate",
    "frame-drop-count",
    "vo-drop-frame-count",
    "demuxer-cache-duration",
    "cache-speed",
    "mpv-version",
];

/// How a command argument (always a string in argv form) is checked.
#[derive(Debug, Clone, Copy)]
enum Arg {
    /// A finite decimal number.
    Number,
    /// `seek` flags: `+`-joined `relative|absolute|absolute-percent|relative-percent|
    /// keyframes|exact`, at most one of each.
    SeekFlags,
    /// A property from [`SETTABLE`]: `cycle` must not reach around the table.
    SettableProperty,
    Choice(&'static [&'static str]),
    /// A track id: decimal integer ≥ 1.
    TrackId,
}

struct CommandRule {
    name: &'static str,
    required: &'static [Arg],
    optional: &'static [Arg],
}

/// Commands the UI may run, with their argument rules.
const COMMANDS: &[CommandRule] = &[
    CommandRule {
        name: "seek",
        required: &[Arg::Number],
        optional: &[Arg::SeekFlags],
    },
    CommandRule {
        name: "cycle",
        required: &[Arg::SettableProperty],
        optional: &[Arg::Choice(&["up", "down"])],
    },
    CommandRule {
        name: "stop",
        required: &[],
        optional: &[],
    },
    CommandRule {
        name: "frame-step",
        required: &[],
        optional: &[],
    },
    CommandRule {
        name: "frame-back-step",
        required: &[],
        optional: &[],
    },
    CommandRule {
        name: "sub-remove",
        required: &[Arg::TrackId],
        optional: &[],
    },
    CommandRule {
        name: "sub-reload",
        required: &[Arg::TrackId],
        optional: &[],
    },
];

const SEEK_FLAGS: &[&str] = &[
    "relative",
    "absolute",
    "absolute-percent",
    "relative-percent",
    "keyframes",
    "exact",
];

/// Check a property write from the UI. The error is a sentence for the UI.
pub fn validate_property(name: &str, value: &Value) -> Result<(), String> {
    let Some((_, rule)) = SETTABLE.iter().find(|(n, _)| *n == name) else {
        return Err(format!(
            "The player property \"{name}\" cannot be set from the UI."
        ));
    };
    if value_fits(*rule, value) {
        Ok(())
    } else {
        Err(format!(
            "{value} is not a valid value for the player property \"{name}\"."
        ))
    }
}

fn value_fits(rule: Rule, value: &Value) -> bool {
    match rule {
        Rule::Bool => value.is_boolean(),
        Rule::Range(min, max) => value
            .as_f64()
            .is_some_and(|n| n.is_finite() && (min..=max).contains(&n)),
        Rule::Color => value.as_str().is_some_and(is_color),
        Rule::Choice(choices) => value.as_str().is_some_and(|s| choices.contains(&s)),
        Rule::Track => match value {
            Value::Number(n) => n
                .as_u64()
                .is_some_and(|id| (1..=u64::from(u32::MAX)).contains(&id)),
            Value::String(s) => s == "no" || s == "auto" || is_track_id(s),
            _ => false,
        },
        Rule::Font => value.as_str().is_some_and(|s| FONTS.contains(&s)),
    }
}

fn is_color(s: &str) -> bool {
    s.strip_prefix('#')
        .is_some_and(|hex| matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn is_track_id(s: &str) -> bool {
    (1..=9).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u32>().is_ok_and(|id| id >= 1)
}

/// Check a property read from the UI.
pub fn validate_readable(name: &str) -> Result<(), String> {
    let observed = zeiler_player::OBSERVED.iter().any(|(n, _)| *n == name);
    if observed || READABLE.contains(&name) {
        Ok(())
    } else {
        Err(format!(
            "The player property \"{name}\" cannot be read from the UI."
        ))
    }
}

/// Check a command (argv form) from the UI.
pub fn validate_command(args: &[String]) -> Result<(), String> {
    let Some((head, rest)) = args.split_first() else {
        return Err("The player command is empty.".into());
    };
    let Some(rule) = COMMANDS.iter().find(|c| c.name == head) else {
        return Err(format!(
            "The player command \"{head}\" cannot be run from the UI."
        ));
    };
    let max = rule.required.len() + rule.optional.len();
    if rest.len() < rule.required.len() || rest.len() > max {
        return Err(format!(
            "The player command \"{head}\" takes {} to {max} arguments, not {}.",
            rule.required.len(),
            rest.len()
        ));
    }
    let rules = rule.required.iter().chain(rule.optional);
    for (arg, rule) in rest.iter().zip(rules) {
        if !arg_fits(*rule, arg) {
            return Err(format!(
                "\"{arg}\" is not a valid argument for the player command \"{head}\"."
            ));
        }
    }
    Ok(())
}

fn arg_fits(rule: Arg, arg: &str) -> bool {
    match rule {
        Arg::Number => {
            // plain decimals only ("inf", "nan", hex and exponents are refused)
            arg.bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.'))
                && arg.parse::<f64>().is_ok_and(f64::is_finite)
        }
        Arg::SeekFlags => {
            let flags: Vec<&str> = arg.split('+').collect();
            let mut unique = flags.clone();
            unique.sort_unstable();
            unique.dedup();
            unique.len() == flags.len() && flags.iter().all(|f| SEEK_FLAGS.contains(f))
        }
        Arg::SettableProperty => SETTABLE.iter().any(|(n, _)| *n == arg),
        Arg::Choice(choices) => choices.contains(&arg),
        Arg::TrackId => is_track_id(arg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cmd(args: &[&str]) -> Result<(), String> {
        validate_command(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn accepts_every_property_write_the_ui_makes() {
        let ok = [
            ("pause", json!(true)),
            ("mute", json!(false)),
            ("sub-visibility", json!(true)),
            ("secondary-sub-visibility", json!(false)),
            ("sub-bold", json!(true)),
            ("fullscreen", json!(true)),
            ("volume", json!(0)),
            ("volume", json!(100)),
            ("volume", json!(57.5)),
            ("speed", json!(0.25)),
            ("speed", json!(4)),
            ("sub-delay", json!(-600)),
            ("audio-delay", json!(1.25)),
            ("sub-pos", json!(150)),
            ("sub-scale", json!(0.3)),
            ("sub-font-size", json!(55)),
            ("sub-margin-y", json!(22)),
            ("sub-border-size", json!(3)),
            ("sub-outline-size", json!(0)),
            ("sub-shadow-offset", json!(10)),
            ("sub-color", json!("#FFFFFF")),
            ("sub-border-color", json!("#80000000")),
            ("sub-back-color", json!("#00ff00aa")),
            ("sub-ass-override", json!("force")),
            ("sub-align-x", json!("center")),
            ("sub-align-y", json!("bottom")),
            ("sub-border-style", json!("opaque-box")),
            ("aid", json!(1)),
            ("sid", json!("no")),
            ("vid", json!("auto")),
            ("secondary-sid", json!("3")),
            ("sub-font", json!("Segoe UI")),
        ];
        for (name, value) in ok {
            assert_eq!(validate_property(name, &value), Ok(()), "{name} = {value}");
        }
    }

    #[test]
    fn rejects_unknown_properties_and_bad_values() {
        let bad = [
            // not settable at all
            ("stream-open-filename", json!("/etc/passwd")),
            ("glsl-shaders", json!("~/evil.glsl")),
            ("vf", json!("lavfi=movie=/etc/passwd")),
            ("script-opts", json!("x=y")),
            ("http-header-fields", json!("X: y")),
            ("user-agent", json!("x")),
            ("", json!(true)),
            // wrong type
            ("pause", json!("yes")),
            ("pause", json!(1)),
            ("fullscreen", json!("yes")),
            ("volume", json!("50")),
            ("aid", json!(true)),
            // out of range
            ("volume", json!(-1)),
            ("volume", json!(1000.5)),
            ("speed", json!(0.1)),
            ("speed", json!(8)),
            ("sub-delay", json!(601)),
            ("sub-scale", json!(5)),
            ("sub-font-size", json!(9)),
            // malformed colors
            ("sub-color", json!("white")),
            ("sub-color", json!("#FFF")),
            ("sub-color", json!("#GGGGGG")),
            ("sub-color", json!("FFFFFF")),
            // not in the choice
            ("sub-ass-override", json!("maybe")),
            ("sub-align-x", json!("middle")),
            // track ids
            ("aid", json!(0)),
            ("aid", json!(-2)),
            ("aid", json!(1.5)),
            ("sid", json!("1; quit")),
            ("sid", json!("")),
            ("sid", json!("0")),
            // fonts outside the list (a path would load an arbitrary file)
            ("sub-font", json!("/tmp/x.ttf")),
            ("sub-font", json!("Comic Sans MS")),
        ];
        for (name, value) in bad {
            assert!(
                validate_property(name, &value).is_err(),
                "{name} = {value} must be refused"
            );
        }
    }

    #[test]
    fn accepts_the_allowed_commands() {
        for args in [
            &["seek", "5", "relative"][..],
            &["seek", "-5", "relative"],
            &["seek", "0", "absolute"],
            &["seek", "42.5", "absolute+exact"],
            &["seek", "50", "absolute-percent+keyframes"],
            &["seek", "10"],
            &["cycle", "pause"],
            &["cycle", "sid", "down"],
            &["cycle", "mute", "up"],
            &["cycle", "fullscreen"],
            &["stop"],
            &["frame-step"],
            &["frame-back-step"],
            &["sub-remove", "3"],
            &["sub-reload", "12"],
        ] {
            assert_eq!(cmd(args), Ok(()), "{args:?}");
        }
    }

    #[test]
    fn rejects_dangerous_unknown_and_malformed_commands() {
        for args in [
            &[][..],
            &["loadfile", "file:///etc/passwd"],
            &["sub-add", "/home/u/.ssh/id_rsa"],
            &["run", "sh", "-c", "id"],
            &["subprocess", "sh"],
            &["af", "add", "lavfi=x"],
            &["vf", "add", "lavfi=x"],
            &["script-message", "x"],
            &["keybind", "q", "quit"],
            &["define-section", "x", "y"],
            &["quit"],
            &["set", "pause", "yes"],
            &["Seek", "5"],
            // seek arguments
            &["seek"],
            &["seek", "five"],
            &["seek", "nan"],
            &["seek", "inf"],
            &["seek", "1e400"],
            &["seek", "0x10"],
            &["seek", "5", "sideways"],
            &["seek", "5", "absolute+absolute"],
            &["seek", "5", "absolute", "exact"],
            // cycle may only target settable properties (it would bypass the table)
            &["cycle"],
            &["cycle", "glsl-shaders"],
            &["cycle", "stream-open-filename"],
            &["cycle", "ytdl"],
            &["cycle", "pause", "sideways"],
            // argument counts and track ids
            &["stop", "keep-playlist"],
            &["frame-step", "1"],
            &["sub-remove"],
            &["sub-remove", "0"],
            &["sub-remove", "-1"],
            &["sub-reload", "1", "2"],
            &["sub-reload", "x"],
        ] {
            assert!(cmd(args).is_err(), "{args:?} must be refused");
        }
    }

    #[test]
    fn every_cycle_target_is_settable_and_every_rule_is_reachable() {
        for (name, _) in SETTABLE {
            assert_eq!(cmd(&["cycle", name]), Ok(()), "{name}");
        }
        assert!(SETTABLE.iter().all(|(n, _)| !n.is_empty()));
    }

    #[test]
    fn reads_are_limited_to_diagnostics_and_observed_properties() {
        for name in [
            "video-codec",
            "hwdec-current",
            "frame-drop-count",
            "cache-speed",
            "mpv-version",
            "time-pos",
            "track-list",
            "demuxer-cache-time",
        ] {
            assert_eq!(validate_readable(name), Ok(()), "{name}");
        }
        for name in [
            "path",
            "stream-open-filename",
            "http-header-fields",
            "user-agent",
            "working-directory",
            "playlist",
            "",
        ] {
            assert!(validate_readable(name).is_err(), "{name} must be refused");
        }
    }

    /// Settings offers exactly the fonts the player allows (src/lib/subtitle-style.ts
    /// keeps a copy), so no choice there is refused here.
    #[test]
    fn the_settings_font_list_matches() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let source = std::fs::read_to_string(root.join("../src/lib/subtitle-style.ts")).unwrap();
        let list = source
            .split("export const SUBTITLE_FONTS = [")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("SUBTITLE_FONTS in subtitle-style.ts");
        let fonts: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
        assert_eq!(fonts, FONTS);
    }

    #[test]
    fn messages_are_sentences() {
        let e = validate_property("vf", &json!("x")).unwrap_err();
        assert_eq!(e, "The player property \"vf\" cannot be set from the UI.");
        let e = cmd(&["run", "x"]).unwrap_err();
        assert_eq!(e, "The player command \"run\" cannot be run from the UI.");
    }
}
