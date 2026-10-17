//! The adult-content filter: the one place Zeiler decides whether a piece of addon text
//! (a genre option, a catalog name, a title's genres, an addon's name) is adult, so the
//! catalogs and the marketplace hide the same things while `showAdult` is off.
//!
//! The term lists are adapted from Harbor's `src/lib/addons-store/adult-filter.ts`
//! (github.com/harborstremio/harbor), Copyright (c) 2026 Harbor, MIT License — see
//! THIRD-PARTY-NOTICES.md. Zeiler keeps the terms that are unambiguous in titles of
//! catalogs and genres, and drops the ones that are everyday words (`balls`, `stroke`,
//! `explicit`, …) or hide inside them (`scat` in "scatter").
//!
//! It matches text, not meaning: an addon flagged `behaviorHints.adult` is gated by the
//! flag, and anything this misses still needs that flag.

use crate::manifest::Manifest;
use crate::resource::MetaPreview;

/// Adult however they are embedded in a word: matched inside each word with everything
/// but letters removed, so `Hentai-Club` and `hentaiTV` both match (but never across
/// two words: "Top Ornaments" is not "porn").
const SUBSTRING_TERMS: &[&str] = &[
    "porn",
    "hentai",
    "ahegao",
    "doujinshi",
    "futanari",
    "xhamster",
    "xnxx",
    "xvideos",
    "redtube",
    "youporn",
    "spankbang",
    "brazzers",
    "onlyfans",
    "fansly",
    "chaturbate",
    "stripchat",
    "bongacams",
    "livejasmin",
    "camsoda",
    "rule34",
    "blowjob",
    "handjob",
    "cumshot",
    "creampie",
    "gangbang",
    "deepthroat",
    "softcore",
    "striptease",
];

/// Adult only as a whole word (between non-alphanumeric characters), since they also
/// occur inside innocent words or names.
const WORD_TERMS: &[&str] = &[
    "xxx", "nsfw", "18+", "sex", "erotic", "erotica", "eroge", "nude", "nudes", "nudity", "naked",
    "smut", "lewd", "milf", "jav", "fetish", "bdsm", "adult", "adults",
];

/// Tags that rate a genre or a catalog but are worn by plenty of mainstream titles:
/// they are hidden as genre options and catalog names, never used to hide a title.
const SOFT_TERMS: &[&str] = &["ecchi", "yaoi", "yuri", "sexy"];

/// "Adult" that is not a rating: a reading age, an animation genre, a channel.
const ADULT_EXCEPTIONS: &[(&str, &str)] = &[
    ("young", "adult"),
    ("young", "adults"),
    ("adult", "animation"),
    ("adult", "swim"),
    ("adult", "cast"),
];

/// Whether `text` reads as adult content.
pub fn is_adult_text(text: &str) -> bool {
    matches(text, true)
}

fn matches(text: &str, soft: bool) -> bool {
    let lower = text.to_lowercase();
    for word in lower.split_whitespace() {
        let letters: String = word.chars().filter(|c| c.is_alphabetic()).collect();
        if SUBSTRING_TERMS.iter().any(|term| letters.contains(term)) {
            return true;
        }
    }
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '+'))
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().enumerate().any(|(i, word)| {
        let excepted = ADULT_EXCEPTIONS.iter().any(|(a, b)| {
            (word == a && words.get(i + 1) == Some(b)) || (i > 0 && words[i - 1] == *a && word == b)
        });
        (WORD_TERMS.contains(word) || (soft && SOFT_TERMS.contains(word))) && !excepted
    })
}

/// Whether a catalog item is adult by its genres. Its title alone isn't enough (that
/// would hide "Yuri!!! on Ice"), and the soft tags ("Ecchi") aren't either.
pub fn is_adult_meta(meta: &MetaPreview) -> bool {
    meta.genres.iter().any(|genre| matches(genre, false))
}

/// Whether an addon is adult: flagged so in `behaviorHints`, or its id or name says so.
pub fn is_adult_manifest(manifest: &Manifest) -> bool {
    manifest.behavior_hints.adult || is_adult_text(&manifest.id) || is_adult_text(&manifest.name)
}

/// This manifest as shown while adult content is hidden: adult options are taken out of
/// every catalog's extras, and a catalog whose name is adult is dropped. The addon's own
/// `behaviorHints.adult` is left for the caller, which decides what a flagged addon
/// shows.
pub fn without_adult_options(mut manifest: Manifest) -> Manifest {
    // by what the UI shows: the name, else the id
    manifest
        .catalogs
        .retain(|catalog| !is_adult_text(catalog.display_name()));
    for catalog in &mut manifest.catalogs {
        catalog.retain_options(|option| !is_adult_text(option));
    }
    manifest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitsu_adult_genres_are_caught_and_ordinary_ones_are_not() {
        for adult in [
            "Hentai",
            "Ecchi",
            "Yaoi",
            "Yuri",
            "Doujinshi",
            "XXX",
            "18+",
            "Erotic",
        ] {
            assert!(is_adult_text(adult), "{adult}");
        }
        for fine in [
            "Action",
            "Shoujo Ai",
            "Shounen Ai",
            "Mature",
            "Gore",
            "Drama",
            "Sci-Fi",
            "Music",
            "Slice of Life",
            "Kids",
            "Young Adult",
            "Adult Animation",
            "Adult Swim",
            "Top Ornaments",
            "Banana Cream Pie",
            "Chen Tai",
            "Vecchi amici",
            "Scatter",
            "Essex",
            "Sussex Stories",
            "Japan",
            "2026",
        ] {
            assert!(!is_adult_text(fine), "{fine}");
        }
    }

    #[test]
    fn soft_tags_hide_options_but_not_titles() {
        let meta = |genres: &[&str]| -> MetaPreview {
            serde_json::from_value(serde_json::json!({
                "id": "kitsu:1", "type": "anime", "name": "x", "genres": genres
            }))
            .unwrap()
        };
        assert!(is_adult_text("Ecchi"));
        assert!(!is_adult_meta(&meta(&["Comedy", "Ecchi"])));
        assert!(!is_adult_meta(&meta(&["Yuri"])));
        assert!(is_adult_meta(&meta(&["Comedy", "Hentai"])));
        assert!(!is_adult_meta(&meta(&["Adult Cast", "Drama"])));
    }

    #[test]
    fn embedded_substrings_and_separated_words_both_match() {
        assert!(is_adult_text("Hentai-Club"));
        assert!(is_adult_text("bestHENTAItv"));
        assert!(is_adult_text("Adult Movies"));
        assert!(is_adult_text("Top NSFW"));
        assert!(!is_adult_text("Adulthood"), "a whole word only");
    }

    #[test]
    fn the_snapshot_kitsu_genres_lose_only_their_adult_options() {
        let manifest: Manifest = serde_json::from_str(include_str!(
            "../../../src-tauri/builtin/kitsu-manifest.json"
        ))
        .unwrap();
        let before = manifest.catalogs.len();
        let gated = without_adult_options(manifest.clone());
        assert_eq!(gated.catalogs.len(), before);
        for (catalog, original) in gated.catalogs.iter().zip(&manifest.catalogs) {
            let options = |c: &crate::ManifestCatalog| -> Vec<String> {
                c.extra_props()
                    .into_iter()
                    .find(|p| p.name == "genre")
                    .map(|p| p.options)
                    .unwrap_or_default()
            };
            let kept = options(catalog);
            assert!(
                !kept
                    .iter()
                    .any(|g| ["Hentai", "Ecchi", "Yaoi", "Yuri"].contains(&g.as_str()))
            );
            let dropped: Vec<_> = options(original)
                .into_iter()
                .filter(|g| !kept.contains(g))
                .collect();
            assert!(dropped.len() <= 6, "{}: {dropped:?}", catalog.id);
            if !kept.is_empty() {
                assert!(kept.contains(&"Action".to_string()));
            }
        }
    }

    #[test]
    fn an_adult_catalog_name_drops_the_catalog() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"id":"x","name":"X","types":["movie"],"resources":["catalog"],
                "catalogs":[{"type":"movie","id":"a","name":"Popular"},
                            {"type":"movie","id":"b","name":"XXX Picks"},
                            {"type":"movie","id":"hentai-top"}]}"#,
        )
        .unwrap();
        let gated = without_adult_options(manifest);
        let ids: Vec<_> = gated.catalogs.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["a"]);
    }
}
