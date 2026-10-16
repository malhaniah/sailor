//! Preferred languages as mpv's `alang`/`slang` lists.
//!
//! Settings store languages as ISO 639-1 codes (or BCP-47 tags such as `pt-BR`), the
//! UI's choice. Tracks in files carry either ISO 639-1 or ISO 639-2 codes (`en`,
//! `eng`; `fr`, `fre`, `fra`), and mpv before 0.37 compares them literally, so each
//! preferred language goes to mpv in every form. The 639-1 → 639-2 table is
//! `src/lib/languages.json`, which the UI reads too (one table, no drift).

use std::collections::HashMap;
use std::sync::LazyLock;

/// ISO 639-1 → its ISO 639-2 codes (bibliographic first, then terminological).
static ISO_639_2: LazyLock<HashMap<String, Vec<String>>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../src/lib/languages.json"))
        .expect("src/lib/languages.json is a map of codes (tests check it)")
});

/// Whether `code`'s primary language is in the table, as an ISO 639-1 or 639-2 code.
pub(crate) fn is_known(code: &str) -> bool {
    let primary = code
        .split('-')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    ISO_639_2.contains_key(&primary) || ISO_639_2.values().flatten().any(|c| *c == primary)
}

/// The id prefixes of anime databases (as in `zeiler_addon::find_meta`).
const ANIME_ID_PREFIXES: [&str; 4] = ["kitsu:", "mal:", "anilist:", "anidb:"];

/// Whether a title is anime, by its meta id or type.
pub(crate) fn is_anime(meta_id: &str, meta_type: &str) -> bool {
    meta_type == "anime" || ANIME_ID_PREFIXES.iter().any(|p| meta_id.starts_with(p))
}

/// The mpv list (comma-separated) for `langs`: each language as stored, then its
/// primary language, then that language's ISO 639-2 codes, without repeats.
pub(crate) fn mpv_language_list(langs: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut push = |code: String| {
        if !code.is_empty() && !out.iter().any(|c| c.eq_ignore_ascii_case(&code)) {
            out.push(code);
        }
    };
    for lang in langs {
        // never a list separator or anything else mpv would read as syntax
        if !crate::settings_groups::is_language_code(lang) {
            continue;
        }
        push(lang.clone());
        let primary = lang
            .split('-')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        push(primary.clone());
        for code in ISO_639_2.get(&primary).into_iter().flatten() {
            push(code.clone());
        }
    }
    out.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_table_is_well_formed() {
        assert!(ISO_639_2.len() > 60);
        for (one, twos) in ISO_639_2.iter() {
            assert!(
                one.len() == 2 && one.bytes().all(|b| b.is_ascii_lowercase()),
                "{one}"
            );
            assert!(matches!(twos.len(), 1 | 2), "{one}: {twos:?}");
            assert!(
                twos.iter()
                    .all(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_lowercase())),
                "{one}: {twos:?}"
            );
        }
        assert_eq!(ISO_639_2["fr"], ["fre", "fra"]);
    }

    #[test]
    fn languages_reach_mpv_in_every_form() {
        let langs =
            |l: &[&str]| mpv_language_list(&l.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(langs(&["ar", "en"]), "ar,ara,en,eng");
        assert_eq!(langs(&["de"]), "de,ger,deu");
        assert_eq!(langs(&["pt-BR", "pt"]), "pt-BR,pt,por");
        assert_eq!(langs(&["fil"]), "fil", "a 639-2 code with no 639-1 form");
        assert_eq!(langs(&["en", "eng", "EN"]), "en,eng");
        assert_eq!(
            langs(&["en,fr", "xx yy", "ja"]),
            "ja,jpn",
            "malformed entries are skipped"
        );
        assert_eq!(langs(&[]), "");
    }

    #[test]
    fn anime_is_known_by_id_or_type() {
        assert!(is_anime("kitsu:1", "series"));
        assert!(is_anime("mal:5114", "movie"));
        assert!(is_anime("tt0213338", "anime"));
        assert!(!is_anime("tt0108778", "series"));
    }
}
