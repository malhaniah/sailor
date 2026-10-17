/**
 * The next episode, for the Up Next card, the "Next Episode" pill and the n / p keys.
 *
 * - **Which.** The video after the current one in the title's order (lib/next-video.ts:
 *   Stremio's rules — released, no jump into the specials).
 * - **With what.** Near the end (the last 90 s, the credits, or when asked) the addons
 *   are asked for the next episode's streams (`streams_get`), and a stream from the same
 *   addon with the same `bingeGroup` continues this one: it loads in the same player
 *   (`loadNext` → `player_load`). Without one, playing it leaves the player and opens
 *   the next episode's stream picker, as Stremio does.
 * - **By itself** (`bingeWatching`): when the episode ends — unless the user closed the
 *   Up Next card for this episode (which also cancels a next episode still waiting for
 *   its streams). The card shows in the last `upNextSeconds` and counts down the time
 *   left. Playing the episode again (a new file) brings both back.
 *
 * Nothing here for another file opened in external mpv's window (`foreign`), nor for
 * live TV. The end-of-episode rule is Harbor's `use-auto-next-episode.ts` (MIT — see
 * THIRD-PARTY-NOTICES.md), reduced to the end of the file.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { bingeCandidate, type BingeCandidate, nextVideo, previousVideo } from "../../lib/next-video";
import {
  launchForVideo,
  leavePlayer,
  metaItemForLaunch,
  type PlayerLaunch,
  requestPicker,
} from "../../lib/playback-handoff";
import { streamsGet } from "../../lib/player-ipc";
import type { Video } from "../../lib/types";
import type { Phase } from "./mpv-state";
import { type PlaybackClock, useClock } from "./playback-clock";
import type { PlayerEventListener } from "./usePlaybackSession";

/** Ask for the next episode's streams this long before the end. */
const PREFETCH_SEC = 90;

export interface NextEpisode {
  video: Video;
  /** where its stream comes from: still asking, a stream that continues this one, or
   * none (its picker opens) */
  stream: { state: "unknown" } | { state: "checking" } | { state: "found"; candidate: BingeCandidate } | { state: "none" };
}

export interface NextEpisodeControls {
  next: NextEpisode | null;
  /** the Up Next card is up (the last seconds, or the episode ended) */
  upNext: boolean;
  /** seconds until the next episode plays by itself (null: it will not) */
  countdown: number | null;
  playNext: () => void;
  /** the episode before, if there is one (the p key) */
  playPrevious: (() => void) | null;
  /** close the Up Next card: no next episode by itself after this one */
  dismiss: () => void;
  /** ask for the next episode's streams now (the credits started) */
  prefetch: () => void;
}

export function useNextEpisode({
  launch,
  clock,
  phase,
  foreign,
  bingeWatching,
  upNextSeconds,
  loadNext,
  subscribeEvents,
}: {
  launch: PlayerLaunch;
  clock: PlaybackClock;
  phase: Phase;
  foreign: boolean;
  bingeWatching: boolean;
  upNextSeconds: number;
  loadNext: (next: PlayerLaunch) => void;
  subscribeEvents: (listener: PlayerEventListener) => () => void;
}): NextEpisodeControls {
  const meta = launch.meta;
  const current = launch.video;
  const video = useMemo(
    () => (meta && current && !launch.live ? nextVideo(meta.videos, current.id) : null),
    [meta, current, launch.live],
  );
  const previous = useMemo(
    () => (meta && current && !launch.live ? previousVideo(meta.videos, current.id) : null),
    [meta, current, launch.live],
  );

  // the next episode's stream, per launch (a new episode asks again)
  const [stream, setStream] = useState<NextEpisode["stream"]>({ state: "unknown" });
  const [dismissed, setDismissed] = useState(false);
  // play once the streams are known (Play now was pressed while still asking)
  const [wanted, setWanted] = useState(false);
  const asking = useRef<PlayerLaunch | null>(null);
  useEffect(() => {
    setStream({ state: "unknown" });
    setDismissed(false);
    setWanted(false);
    asking.current = null;
  }, [launch]);

  /** The candidate stream for `target` from the addons' answers (or the meta's own). */
  const findStream = useCallback(
    async (target: Video): Promise<BingeCandidate | null> => {
      const bingeGroup = launch.stream?.behaviorHints.bingeGroup ?? null;
      if (!meta || !bingeGroup) return null;
      const groups = launch.transportUrl ? await streamsGet(meta.type, target.id) : [];
      return bingeCandidate({
        bingeGroup,
        transportUrl: launch.transportUrl,
        addonName: launch.addonName,
        groups,
        video: target,
      });
    },
    [launch, meta],
  );

  const prefetch = useCallback(() => {
    if (!video || foreign || asking.current === launch) return;
    asking.current = launch;
    setStream({ state: "checking" });
    findStream(video)
      .then((candidate) => {
        if (asking.current !== launch) return;
        setStream(candidate ? { state: "found", candidate } : { state: "none" });
      })
      .catch(() => asking.current === launch && setStream({ state: "none" }));
  }, [video, foreign, launch, findStream]);

  // near the end: ask
  const nearEnd = useClock(clock, (s) => s.duration > 0 && s.duration - s.timePos <= PREFETCH_SEC);
  useEffect(() => {
    if (nearEnd && phase.kind === "playing") prefetch();
  }, [nearEnd, phase.kind, prefetch]);

  /** Play `target` with `candidate`, or open its picker. */
  const go = useCallback(
    (target: Video, candidate: BingeCandidate | null) => {
      const next = candidate ? launchForVideo(launch, { video: target, ...candidate }) : null;
      if (next) {
        loadNext(next);
      } else if (meta) {
        leavePlayer();
        requestPicker(metaItemForLaunch(meta), target);
      }
    },
    [launch, meta, loadNext],
  );

  const playNext = useCallback(() => {
    if (!video || foreign) return;
    if (stream.state === "found") go(video, stream.candidate);
    else if (stream.state === "none") go(video, null);
    else {
      setWanted(true);
      prefetch();
    }
  }, [video, foreign, stream, go, prefetch]);

  useEffect(() => {
    if (!wanted || !video) return;
    if (stream.state === "found") {
      setWanted(false);
      go(video, stream.candidate);
    } else if (stream.state === "none") {
      setWanted(false);
      go(video, null);
    }
  }, [wanted, video, stream, go]);

  // one previous-episode lookup at a time (a second p waits for the first)
  const goingBack = useRef(false);
  const playPrevious = useMemo(() => {
    if (!previous || foreign) return null;
    return () => {
      if (goingBack.current) return;
      goingBack.current = true;
      void findStream(previous)
        .catch(() => null)
        .then((candidate) => go(previous, candidate))
        .finally(() => {
          goingBack.current = false;
        });
    };
  }, [previous, foreign, findStream, go]);

  // the Up Next card and its countdown
  const ended = phase.kind === "ended";
  const remaining = useClock(clock, (s) =>
    s.duration > upNextSeconds * 2 && s.duration - s.timePos <= upNextSeconds
      ? Math.max(0, Math.ceil(s.duration - s.timePos))
      : null,
  );
  const upNext = !!video && !foreign && !dismissed && (ended || (phase.kind === "playing" && remaining !== null));
  const autoplay = bingeWatching && upNext;
  const countdown = autoplay ? (ended ? 0 : remaining) : null;

  // by itself, once the episode ended (once per file)
  const advanced = useRef(false);
  useEffect(() => {
    if (!autoplay || !ended || advanced.current) return;
    advanced.current = true;
    playNext();
  }, [autoplay, ended, playNext]);

  // a new file (Play again, or the next episode): the card and autoplay come back
  useEffect(
    () =>
      subscribeEvents((event) => {
        if (event.type !== "start-file") return;
        advanced.current = false;
        setDismissed(false);
      }),
    [subscribeEvents],
  );

  return {
    next: video && !foreign ? { video, stream } : null,
    upNext,
    countdown,
    playNext,
    playPrevious,
    dismiss: useCallback(() => {
      setDismissed(true);
      setWanted(false);
    }, []),
    prefetch,
  };
}
