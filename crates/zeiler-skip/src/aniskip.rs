//! AniSkip (anime, by MyAnimeList id) and the Kitsu → MyAnimeList id mapping it needs.
//! Ported from Harbor's `src/lib/skip-intro/aniskip.ts` (MIT) — see
//! THIRD-PARTY-NOTICES.md.
//!
//! - `GET {kitsu}anime/{kitsuId}/mappings?filter[externalSite]=myanimelist/anime` →
//!   `{data: [{attributes: {externalSite, externalId}}]}`.
//! - `GET {aniskip}skip-times/{malId}/{episode}?types=op&types=ed&types=mixed-op&
//!   types=mixed-ed&types=recap&episodeLength={seconds}` → `{found, results:
//!   [{interval: {startTime, endTime}, skipType}]}`; 404 with `found: false` when there
//!   are none. `episodeLength` is required (AniSkip prefers submissions for a cut of
//!   about that length).
//!
//! Both are read leniently: a broken list is empty, a broken entry is skipped, and a
//! number may come as a string.

use serde::Deserialize;
use serde_with::{DefaultOnError, DisplayFromStr, PickFirst, VecSkipError, serde_as};

use crate::{FetchError, RawSegment, SegmentKind, SegmentSource, SkipClient};

const MAL_SITE: &str = "myanimelist/anime";

#[serde_as]
#[derive(Debug, Deserialize)]
struct Mappings {
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError<VecSkipError<_>>")]
    data: Vec<Mapping>,
}

#[derive(Debug, Deserialize)]
struct Mapping {
    attributes: MappingAttributes,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MappingAttributes {
    #[serde(default)]
    external_site: Option<String>,
    #[serde(default)]
    external_id: Option<serde_json::Value>,
}

/// The MyAnimeList id Kitsu maps `kitsu` to (`None`: Kitsu knows none).
pub(crate) async fn mal_id(client: &SkipClient, kitsu: u64) -> Result<Option<u64>, FetchError> {
    let mut url = client
        .endpoints()
        .kitsu
        .join(&format!("anime/{kitsu}/mappings"))
        .expect("a relative path");
    url.query_pairs_mut()
        .append_pair("filter[externalSite]", MAL_SITE);
    let Some(mappings) = client.get_json::<Mappings>(url).await? else {
        return Ok(None);
    };
    Ok(parse_mal_id(&mappings))
}

fn parse_mal_id(mappings: &Mappings) -> Option<u64> {
    mappings.data.iter().find_map(|m| {
        if m.attributes.external_site.as_deref() != Some(MAL_SITE) {
            return None;
        }
        match m.attributes.external_id.as_ref()? {
            serde_json::Value::String(s) => s.trim().parse().ok(),
            serde_json::Value::Number(n) => n.as_u64(),
            _ => None,
        }
    })
}

/// `found: false` always comes with no results, so only the results are read.
#[serde_as]
#[derive(Debug, Deserialize)]
struct SkipTimes {
    #[serde(default)]
    #[serde_as(deserialize_as = "DefaultOnError<VecSkipError<_>>")]
    results: Vec<SkipResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkipResult {
    interval: Interval,
    skip_type: String,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Interval {
    #[serde_as(deserialize_as = "PickFirst<(_, DisplayFromStr)>")]
    start_time: f64,
    #[serde_as(deserialize_as = "PickFirst<(_, DisplayFromStr)>")]
    end_time: f64,
}

/// AniSkip's segments for episode `episode` of MyAnimeList title `mal`.
pub(crate) async fn segments(
    client: &SkipClient,
    mal: u64,
    episode: u32,
    duration_sec: f64,
) -> Result<Vec<RawSegment>, FetchError> {
    let mut url = client
        .endpoints()
        .aniskip
        .join(&format!("skip-times/{mal}/{episode}"))
        .expect("a relative path");
    {
        let mut query = url.query_pairs_mut();
        for kind in ["op", "ed", "mixed-op", "mixed-ed", "recap"] {
            query.append_pair("types", kind);
        }
        let length = if duration_sec.is_finite() && duration_sec > 0.0 {
            duration_sec
        } else {
            0.0
        };
        query.append_pair("episodeLength", &format!("{length:.3}"));
    }
    Ok(client
        .get_json::<SkipTimes>(url)
        .await?
        .map(parse_segments)
        .unwrap_or_default())
}

fn parse_segments(times: SkipTimes) -> Vec<RawSegment> {
    times
        .results
        .into_iter()
        .filter_map(|r| {
            let kind = match r.skip_type.as_str() {
                "op" | "mixed-op" => SegmentKind::Intro,
                "ed" | "mixed-ed" => SegmentKind::Outro,
                "recap" => SegmentKind::Recap,
                _ => return None,
            };
            Some(RawSegment {
                kind,
                start: Some(r.interval.start_time),
                end: Some(r.interval.end_time),
                source: SegmentSource::AniSkip,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from `kitsu.app` (Cowboy Bebop, Kitsu 1 ↔ MyAnimeList 1) and trimmed.
    #[test]
    fn kitsu_maps_to_the_myanimelist_id() {
        let mappings: Mappings = serde_json::from_str(
            r#"{"data":[
                {"id":"1","type":"mappings","attributes":{"externalSite":"anidb","externalId":"23"}},
                {"id":"1175","type":"mappings","attributes":{"createdAt":null,
                 "externalSite":"myanimelist/anime","externalId":"21"}}
            ],"meta":{"count":2}}"#,
        )
        .unwrap();
        assert_eq!(parse_mal_id(&mappings), Some(21));
        let numeric: Mappings = serde_json::from_str(
            r#"{"data":[{"attributes":{"externalSite":"myanimelist/anime","externalId":5}}]}"#,
        )
        .unwrap();
        assert_eq!(parse_mal_id(&numeric), Some(5));
        let none: Mappings = serde_json::from_str(r#"{"data":[]}"#).unwrap();
        assert_eq!(parse_mal_id(&none), None);
        let junk: Mappings = serde_json::from_str(
            r#"{"data":[{"attributes":{"externalSite":"myanimelist/anime","externalId":"x"}}]}"#,
        )
        .unwrap();
        assert_eq!(parse_mal_id(&junk), None);
    }

    /// Recorded from `api.aniskip.com` (One Piece, MyAnimeList 21, episode 1000).
    #[test]
    fn skip_types_map_to_kinds() {
        let times: SkipTimes = serde_json::from_str(
            r#"{"found":true,"results":[
                {"interval":{"startTime":13.891,"endTime":123.834},"skipType":"op",
                 "skipId":"5bcf9262-b16f-40ad-9d12-8168e4ecad2d","episodeLength":1430.721},
                {"interval":{"startTime":1300,"endTime":1390.5},"skipType":"mixed-ed"},
                {"interval":{"startTime":0,"endTime":40},"skipType":"recap"},
                {"interval":{"startTime":5,"endTime":9},"skipType":"something-new"}
            ],"message":"Successfully found skip times","statusCode":200}"#,
        )
        .unwrap();
        let kinds: Vec<_> = parse_segments(times)
            .into_iter()
            .map(|s| (s.kind, s.start, s.end))
            .collect();
        assert_eq!(
            kinds,
            [
                (SegmentKind::Intro, Some(13.891), Some(123.834)),
                (SegmentKind::Outro, Some(1300.0), Some(1390.5)),
                (SegmentKind::Recap, Some(0.0), Some(40.0)),
            ]
        );
        let none: SkipTimes = serde_json::from_str(
            r#"{"found":false,"results":[],"message":"No skip times found","statusCode":404}"#,
        )
        .unwrap();
        assert!(parse_segments(none).is_empty());
    }

    #[test]
    fn nulls_strings_and_broken_entries_cost_only_themselves() {
        let times: SkipTimes = serde_json::from_str(
            r#"{"found": null, "results": [
                {"interval": {"startTime": null, "endTime": 9}, "skipType": "op"},
                {"interval": {"startTime": "13.5", "endTime": "90"}, "skipType": "op"},
                {"skipType": "ed"},
                "junk"
            ]}"#,
        )
        .unwrap();
        let got: Vec<_> = parse_segments(times)
            .into_iter()
            .map(|s| (s.kind, s.start, s.end))
            .collect();
        assert_eq!(got, [(SegmentKind::Intro, Some(13.5), Some(90.0))]);
        let broken: SkipTimes = serde_json::from_str(r#"{"results": null}"#).unwrap();
        assert!(parse_segments(broken).is_empty());
        let mappings: Mappings = serde_json::from_str(
            r#"{"data": [5, {"attributes": {"externalSite": "myanimelist/anime", "externalId": "21"}}]}"#,
        )
        .unwrap();
        assert_eq!(parse_mal_id(&mappings), Some(21));
    }
}
