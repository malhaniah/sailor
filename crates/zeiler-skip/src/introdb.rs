//! TheIntroDB (IMDb and TMDB ids). Ported from Harbor's
//! `src/lib/skip-intro/theintrodb.ts` (MIT) — see THIRD-PARTY-NOTICES.md.
//!
//! `GET {introdb}media?imdb_id=tt…|tmdb_id=N[&season=S&episode=E][&duration_ms=L]` →
//! `{tvdb_id, type, intro: [{start_ms, end_ms}], recap: [...], credits: [...],
//! preview: [...]}`, each list optional; a `null` start is the beginning and a `null` end
//! the end of the video. Credits and the preview after them are both an outro; one
//! starting at 0 is the service's "none" (an intro or recap ending at 0 is too, and comes
//! out empty). 404 when it knows no such title. Season and episode are in the id's own
//! provider's numbering (IMDb's, TMDB's), which the service converts itself, so no
//! `mapping` is sent. `duration_ms`, the video's length, picks the release version
//! (theatrical, extended cut…) closest to it within 5 minutes, else the most submitted
//! one, as no `duration_ms` does; 0 asks for the version of unknown length, so an unknown
//! length sends none. Read leniently: a missing or broken list is empty (so is the
//! per-episode count list it may answer instead), a broken entry is skipped, and a number
//! may come as a string.

use serde::Deserialize;
use serde_with::{DefaultOnError, DisplayFromStr, PickFirst, VecSkipError, serde_as};

use crate::{FetchError, RawSegment, SegmentKind, SegmentSource, SkipClient};

#[serde_as]
#[derive(Debug, Default, Deserialize)]
struct Media {
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError<VecSkipError<_>>")]
    intro: Vec<Range>,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError<VecSkipError<_>>")]
    recap: Vec<Range>,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError<VecSkipError<_>>")]
    credits: Vec<Range>,
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError<VecSkipError<_>>")]
    preview: Vec<Range>,
}

/// An entry whose start or end is neither a number, a numeric string nor null is
/// skipped (null is the start or end of the video).
#[serde_as]
#[derive(Debug, Deserialize)]
struct Range {
    #[serde(default)]
    #[serde_as(deserialize_as = "Option<PickFirst<(_, DisplayFromStr)>>")]
    start_ms: Option<f64>,
    #[serde(default)]
    #[serde_as(deserialize_as = "Option<PickFirst<(_, DisplayFromStr)>>")]
    end_ms: Option<f64>,
}

/// TheIntroDB's segments for the title `id` (`(param, value)`: `imdb_id` or `tmdb_id`),
/// at `season`/`episode` for an episode, in the release version `duration_sec` long (0:
/// not known).
pub(crate) async fn segments(
    client: &SkipClient,
    id: (&str, String),
    season: Option<u32>,
    episode: Option<u32>,
    duration_sec: f64,
) -> Result<Vec<RawSegment>, FetchError> {
    let mut url = client
        .endpoints()
        .introdb
        .join("media")
        .expect("a relative path");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair(id.0, &id.1);
        if let (Some(season), Some(episode)) = (season, episode) {
            query.append_pair("season", &season.to_string());
            query.append_pair("episode", &episode.to_string());
        }
        // 0 would ask for the version of unknown length, not for any version
        let duration_ms = if duration_sec.is_finite() {
            (duration_sec * 1000.0).round()
        } else {
            0.0
        };
        if duration_ms > 0.0 {
            query.append_pair("duration_ms", &format!("{duration_ms:.0}"));
        }
    }
    Ok(client
        .get_json::<Media>(url)
        .await?
        .map(parse_media)
        .unwrap_or_default())
}

fn parse_media(media: Media) -> Vec<RawSegment> {
    let seconds = |ms: Option<f64>| ms.map(|ms| ms / 1000.0);
    [
        (SegmentKind::Intro, media.intro),
        (SegmentKind::Recap, media.recap),
        (SegmentKind::Outro, media.credits),
        (SegmentKind::Outro, media.preview),
    ]
    .into_iter()
    .flat_map(|(kind, ranges)| {
        ranges
            .into_iter()
            // credits or a preview starting at 0 is the service's "none", not the whole video
            .filter(move |r| kind != SegmentKind::Outro || r.start_ms != Some(0.0))
            .map(move |r| RawSegment {
                kind,
                start: seconds(r.start_ms),
                end: seconds(r.end_ms),
                source: SegmentSource::IntroDb,
            })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from `api.theintrodb.org` (Breaking Bad S01E01; a movie has no season).
    #[test]
    fn credits_and_previews_are_outros_and_open_ends_stay_open() {
        let media: Media = serde_json::from_str(
            r#"{"tvdb_id":81189,"type":"tv","season":1,"episode":1,
                "intro":[{"start_ms":228664,"end_ms":246143}],
                "credits":[{"start_ms":3431000,"end_ms":null}],
                "recap":[{"start_ms":null,"end_ms":30000}],
                "preview":[{"start_ms":3500000}]}"#,
        )
        .unwrap();
        let segments = parse_media(media);
        let got: Vec<_> = segments.iter().map(|s| (s.kind, s.start, s.end)).collect();
        assert_eq!(
            got,
            [
                (SegmentKind::Intro, Some(228.664), Some(246.143)),
                (SegmentKind::Recap, None, Some(30.0)),
                (SegmentKind::Outro, Some(3431.0), None),
                (SegmentKind::Outro, Some(3500.0), None),
            ]
        );
        let movie: Media = serde_json::from_str(
            r#"{"tvdb_id":190,"type":"movie","credits":[{"start_ms":8300000,"end_ms":null}]}"#,
        )
        .unwrap();
        assert_eq!(parse_media(movie).len(), 1);
    }

    #[test]
    fn nulls_strings_and_broken_entries_cost_only_themselves() {
        let media: Media = serde_json::from_str(
            r#"{"intro": null, "recap": "soon",
                "credits": [{"start_ms": "3431000", "end_ms": null}, {"start_ms": {}}, 7],
                "preview": [{"start_ms": 3500000, "end_ms": 3520000, "extra": true}]}"#,
        )
        .unwrap();
        let got: Vec<_> = parse_media(media)
            .into_iter()
            .map(|s| (s.kind, s.start, s.end))
            .collect();
        assert_eq!(
            got,
            [
                (SegmentKind::Outro, Some(3431.0), None),
                (SegmentKind::Outro, Some(3500.0), Some(3520.0)),
            ]
        );
    }

    /// The OpenAPI spec: credits and previews "may" start at 0 for "no segment".
    #[test]
    fn credits_starting_at_zero_are_no_credits() {
        let media: Media = serde_json::from_str(
            r#"{"tvdb_id":81189,"type":"tv","season":1,"episode":1,
                "recap":[{"start_ms":0,"end_ms":30000}],
                "credits":[{"start_ms":0,"end_ms":null},{"start_ms":3431000,"end_ms":null}],
                "preview":[{"start_ms":0,"end_ms":0}]}"#,
        )
        .unwrap();
        let got: Vec<_> = parse_media(media)
            .into_iter()
            .map(|s| (s.kind, s.start, s.end))
            .collect();
        assert_eq!(
            got,
            [
                (SegmentKind::Recap, Some(0.0), Some(30.0)),
                (SegmentKind::Outro, Some(3431.0), None),
            ],
            "a recap may start at 0; credits may not"
        );
    }

    /// v4's other answer for a title with no submissions (the OpenAPI spec's example).
    #[test]
    fn an_episode_count_list_has_no_segments() {
        let media: Media = serde_json::from_str(
            r#"{"tvdb_id":71663,"type":"tv","episodes":[{"season":1,"episode":1,
                "total_submission_count":4,"intro_accepted_count":4}]}"#,
        )
        .unwrap();
        assert!(parse_media(media).is_empty());
    }
}
