/**
 * The next (and previous) episode of a series, and the stream to binge it with.
 *
 * - **Order.** Stremio's: `meta.videos` as stremio-core sorts them (episodes by season,
 *   the specials of season 0 last, then episode; others by release date), and the next
 *   one only if it is released and does not cross into the specials from another
 *   season. The same rules as Rust's `zeiler_account::next_video_id`, which marks the
 *   next episode in Continue Watching.
 * - **Binge.** Stremio's `behaviorHints.bingeGroup`: a stream for the next episode from
 *   the same addon with the same, non-empty bingeGroup is the same source (same
 *   release group, quality, debrid…), so it plays without asking. Without one the next
 *   episode's stream picker opens instead.
 *
 * The order and next-video rules are stremio-core's (`types/resource/meta_item.rs`,
 * MIT, Copyright (c) 2019 SmartCode OOD — see THIRD-PARTY-NOTICES.md).
 */
import type { AddonStreams, Stream, Video } from "./types";

/** stremio-core's series info: both season and episode, or neither. */
function seriesInfo(video: Video): [number, number] | null {
  return video.season != null && video.episode != null ? [video.season, video.episode] : null;
}

function releasedAt(video: Video): number | null {
  if (!video.released) return null;
  const at = Date.parse(video.released);
  return Number.isFinite(at) ? at : null;
}

/** The videos in stremio-core's order (`MetaItem.videos`): later duplicates of an id
 * dropped; episodes first, by season (specials last) then episode; the rest by release
 * date, oldest first in a series and newest first otherwise, undated last. Stable. */
export function metaOrder(videos: Video[]): Video[] {
  const seen = new Set<string>();
  const unique = videos.filter((v) => (seen.has(v.id) ? false : (seen.add(v.id), true)));
  const isSeries = unique.some((v) => seriesInfo(v) !== null);
  const season = (s: number) => (s === 0 ? Number.MAX_SAFE_INTEGER : s);
  return unique
    .map((video, index) => ({ video, index }))
    .sort((a, b) => {
      const sa = seriesInfo(a.video);
      const sb = seriesInfo(b.video);
      let order = 0;
      if (sa && sb) order = season(sa[0]) - season(sb[0]) || sa[1] - sb[1];
      else if (sa) order = -1;
      else if (sb) order = 1;
      else {
        const ra = releasedAt(a.video);
        const rb = releasedAt(b.video);
        if (ra !== null && rb !== null) order = isSeries ? ra - rb : rb - ra;
        else if (ra !== null) order = -1;
        else if (rb !== null) order = 1;
      }
      return order || a.index - b.index;
    })
    .map((x) => x.video);
}

const seasonOf = (video: Video) => seriesInfo(video)?.[0] ?? 0;

/** The video after `currentId`: released by `now`, and not a special (season 0) after
 * an episode of another season (stremio-core `MetaItem::next_video`). */
export function nextVideo(videos: Video[], currentId: string, now = Date.now()): Video | null {
  const order = metaOrder(videos);
  const at = order.findIndex((v) => v.id === currentId);
  if (at < 0) return null;
  const current = order[at];
  const next = order[at + 1];
  if (!next) return null;
  const released = releasedAt(next);
  if (released !== null && released > now) return null;
  if (seasonOf(next) === 0 && seasonOf(current) !== 0) return null;
  return next;
}

/** The video before `currentId`, by the same rule the other way round: no special
 * before an episode of a regular season. */
export function previousVideo(videos: Video[], currentId: string): Video | null {
  const order = metaOrder(videos);
  const at = order.findIndex((v) => v.id === currentId);
  if (at <= 0) return null;
  const current = order[at];
  const previous = order[at - 1];
  if (seasonOf(previous) === 0 && seasonOf(current) !== 0) return null;
  return previous;
}

/** A stream to binge with, and where it came from. */
export interface BingeCandidate {
  stream: Stream;
  /** null for a stream that came with the meta itself */
  transportUrl: string | null;
  addonName: string | null;
}

/**
 * The stream for the next video that continues this one: from the same addon
 * (`transportUrl`; null = the streams that came with the meta, in `video.streams`) and
 * with the same non-empty bingeGroup. Only playable streams (a URL or a torrent).
 */
export function bingeCandidate({
  bingeGroup,
  transportUrl,
  addonName,
  groups,
  video,
}: {
  bingeGroup: string | null;
  transportUrl: string | null;
  addonName: string | null;
  /** the addons' answers for the next video (`streams_get`) */
  groups: AddonStreams[];
  video: Video;
}): BingeCandidate | null {
  if (!bingeGroup) return null;
  const matches = (s: Stream) => s.behaviorHints.bingeGroup === bingeGroup && !!(s.url || s.infoHash);
  if (transportUrl === null) {
    const stream = video.streams.find(matches);
    return stream ? { stream, transportUrl: null, addonName } : null;
  }
  const group = groups.find((g) => g.transportUrl === transportUrl);
  const stream = group?.streams.find(matches);
  return stream && group ? { stream, transportUrl, addonName: group.addonName } : null;
}
