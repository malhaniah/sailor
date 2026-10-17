//! TheIntroDB (IMDb and TMDB ids). Ported from Harbor's
//! `src/lib/skip-intro/theintrodb.ts` (MIT) — see THIRD-PARTY-NOTICES.md.
//!
//! `GET {introdb}media?imdb_id=tt…|tmdb_id=N[&season=S&episode=E]` → `{tmdb_id, type,
//! intro: [{start_ms, end_ms}], recap: [...], credits: [...], preview: [...]}`, each
//! list optional; a `null` start is the beginning and a `null` end the end of the video.
//! Credits and the preview after them are both an outro. 404 when it knows no such
//! title. Read leniently: a missing or broken list is empty, a broken entry is skipped,
//! and a number may come as a string.

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
/// at `season`/`episode` for an episode.
pub(crate) async fn segments(
    client: &SkipClient,
    id: (&str, String),
    season: Option<u32>,
    episode: Option<u32>,
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
        ranges.into_iter().map(move |r| RawSegment {
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
            r#"{"tmdb_id":1396,"type":"tv","season":1,"episode":1,
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
            r#"{"tmdb_id":278,"type":"movie","credits":[{"start_ms":8300000,"end_ms":null}]}"#,
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
}
