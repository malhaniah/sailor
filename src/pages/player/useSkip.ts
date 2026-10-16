/**
 * Skipping intros, recaps and credits, per file: where they are, the one playing now,
 * the Skip action, and skipping by itself with an Undo.
 *
 * - **Where.** Once per file, when its duration is first known, Rust asks AniSkip or
 *   TheIntroDB (`skip_segments_get`, if `skip.onlineLookup`); without an answer the
 *   file's chapter names count (lib/skip.ts). With a next episode and no credits found,
 *   the last 90 s are its credits. Another file opened in external mpv's window gets
 *   only its own chapters.
 * - **Skip.** Seeks to the segment's end; credits with a next episode play it instead.
 * - **By itself** (`skip.autoSkip*`, off by default): once per segment per file, only
 *   when playback enters the segment by playing on (never after a seek into it), and
 *   only for segments from AniSkip, TheIntroDB or a chapter named exactly for it —
 *   never the synthetic credits. An online answer arrives after playback started, so a
 *   segment it brings counts as entered if playback is in its first seconds and the
 *   user has not seeked in this file. A toast offers Undo for 5 s, which seeks back and
 *   leaves that segment alone. Credits skipped by themselves play the next episode when
 *   there is one (as the "Next Episode" pill would).
 * - **External mpv.** While a skip is offered, mpv's own window shows it too, and Enter
 *   there skips (`player_osd_prompt`, answered as a `client-message`).
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PlayerLaunch } from "../../lib/playback-handoff";
import { playerCommand, playerOsdPrompt, skipSegmentsGet } from "../../lib/player-ipc";
import {
  activeSegmentIndex,
  chapterSegments,
  enteredByPlaying,
  LATE_ENTRY_SEC,
  mayAutoSkip,
  type SegmentKind,
  type SkipSegment,
  withSyntheticOutro,
} from "../../lib/skip";
import type { SkipSettings } from "../../lib/types";
import type { Backend, MpvState } from "./mpv-state";
import { type PlaybackClock, useClock } from "./playback-clock";
import type { PlayerEventListener } from "./usePlaybackSession";

/** How long the "Skipped intro · Undo" toast stays. */
const UNDO_MS = 5000;

// External mpv's prompt commands go one after another, in the order asked: an offer
// taken back and the next one made in the same moment must not land the wrong way round.
let promptQueue: Promise<unknown> = Promise.resolve();
function sendPrompt(kind: Parameters<typeof playerOsdPrompt>[0]) {
  promptQueue = promptQueue.then(() => playerOsdPrompt(kind)).catch(() => undefined);
}

export interface SkipOffer {
  segment: SkipSegment;
  /** credits that lead to the next episode: the pill says "Next Episode" */
  next: boolean;
}

export interface AutoSkipped {
  kind: SegmentKind;
  undo: () => void;
}

export interface SkipControls {
  /** the segment playing now, when the Skip button may show */
  offer: SkipOffer | null;
  skip: () => void;
  /** a skip that just happened by itself */
  skipped: AutoSkipped | null;
  dismissSkipped: () => void;
  /** the credits are playing (the next episode's streams are asked for then) */
  inOutro: boolean;
}

export function useSkip({
  launch,
  clock,
  mpv,
  backend,
  playing,
  foreign,
  settings,
  hasNext,
  onNext,
  subscribeEvents,
}: {
  launch: PlayerLaunch;
  clock: PlaybackClock;
  mpv: MpvState;
  backend: Backend;
  /** a file is playing (not loading, ended, gone) */
  playing: boolean;
  foreign: boolean;
  settings: SkipSettings | null;
  hasNext: boolean;
  onNext: () => void;
  subscribeEvents: (listener: PlayerEventListener) => () => void;
}): SkipControls {
  // the file the segments belong to (start-file moves it on)
  const [file, setFile] = useState(0);
  const [online, setOnline] = useState<{ file: number; segments: SkipSegment[] } | null>(null);
  useEffect(
    () =>
      subscribeEvents((event) => {
        if (event.type === "start-file") setFile((n) => n + 1);
      }),
    [subscribeEvents],
  );

  const known = useClock(clock, (s) => (s.duration > 0 ? s.duration : 0));
  const durationKnown = known > 0;
  const lookupOn = settings?.onlineLookup ?? false;
  useEffect(() => {
    const meta = launch.meta;
    if (!durationKnown || !meta || foreign || launch.live || !settings) return;
    if (online?.file === file) return;
    const forFile = file;
    setOnline({ file: forFile, segments: [] });
    if (!lookupOn) return;
    const videoId = launch.video?.id ?? meta.behaviorHints.defaultVideoId ?? meta.id;
    skipSegmentsGet({ metaId: meta.id, videoId, durationSec: clock.get().duration })
      .then((found) =>
        setOnline((o) => (o?.file === forFile ? { file: forFile, segments: found } : o)),
      )
      .catch(() => undefined);
    // the duration only gates the lookup: one per file
  }, [durationKnown, file, online, launch, foreign, settings, lookupOn, clock]);

  const segments = useMemo(() => {
    const fromOnline = !foreign && online?.file === file ? online.segments : [];
    const found: SkipSegment[] = fromOnline.length > 0 ? fromOnline : chapterSegments(mpv.chapters, known);
    return withSyntheticOutro(found, known, hasNext && !foreign);
  }, [foreign, online, file, mpv.chapters, known, hasNext]);

  const index = useClock(clock, (s) => activeSegmentIndex(segments, s.timePos));
  const active = playing && index >= 0 ? segments[index] : null;
  const offer: SkipOffer | null =
    active && (settings?.showButton ?? true)
      ? { segment: active, next: active.kind === "outro" && hasNext }
      : null;

  const seekTo = (sec: number) => void playerCommand("seek", sec, "absolute").catch(() => undefined);

  const latest = useRef({ active, hasNext, onNext });
  latest.current = { active, hasNext, onNext };
  const skip = useCallback(() => {
    const { active: segment, hasNext: next, onNext: playNext } = latest.current;
    if (!segment) return;
    if (segment.kind === "outro" && next) playNext();
    else seekTo(segment.endSec);
  }, []);

  // --- skipping by itself ------------------------------------------------------------

  const [skipped, setSkipped] = useState<AutoSkipped | null>(null);
  const handled = useRef(new Set<string>());
  useEffect(() => {
    handled.current = new Set();
    setSkipped(null);
  }, [file]);

  const auto = useRef({ segments, settings, playing, hasNext, onNext });
  auto.current = { segments, settings, playing, hasNext, onNext };
  useEffect(() => {
    let previous: number | null = null;
    let seeked = false;
    // per file: a seek happened (by the user, or a skip), and the segments seen so far
    let seekedInFile = false;
    let seen = new Set<string>();
    const offEvents = subscribeEvents((event) => {
      if (event.type === "start-file") {
        previous = null;
        seekedInFile = false;
        seen = new Set();
      }
      // a seek's landing is not playing on: the next position starts afresh
      if (event.type === "seek") {
        seeked = true;
        seekedInFile = true;
      }
    });
    const offClock = clock.subscribe(() => {
      const { timePos, pause } = clock.get();
      const before = previous;
      previous = timePos;
      if (seeked) {
        seeked = false;
        return;
      }
      const now = auto.current;
      if (!now.playing || pause || !now.settings) return;
      for (const segment of now.segments) {
        const key = `${segment.kind}@${segment.startSec}`;
        const fresh = !seen.has(key);
        seen.add(key);
        if (handled.current.has(key)) continue;
        // (a segment that only just became known, while playback is in its first
        // seconds and nothing was seeked yet, was entered by playing on too)
        const late =
          fresh &&
          !seekedInFile &&
          timePos >= segment.startSec &&
          timePos < segment.startSec + LATE_ENTRY_SEC;
        if (!late && !enteredByPlaying(segment, before, timePos)) continue;
        handled.current.add(key);
        const wanted =
          segment.kind === "intro"
            ? now.settings.autoSkipIntro
            : segment.kind === "recap"
              ? now.settings.autoSkipRecap
              : now.settings.autoSkipCredits;
        if (!wanted || !mayAutoSkip(segment)) continue;
        if (segment.kind === "outro" && now.hasNext) {
          now.onNext();
          return;
        }
        seekTo(segment.endSec);
        setSkipped({ kind: segment.kind, undo: () => seekTo(segment.startSec) });
        return;
      }
    });
    return () => {
      offEvents();
      offClock();
    };
  }, [clock, subscribeEvents]);

  useEffect(() => {
    if (!skipped) return;
    const timer = window.setTimeout(() => setSkipped(null), UNDO_MS);
    return () => window.clearTimeout(timer);
  }, [skipped]);

  // --- external mpv: the prompt in its own window -------------------------------------

  const external = backend === "external";
  const promptKind = offer ? (offer.next ? "next" : offer.segment.kind) : null;
  const promptKey = offer ? `${file}|${promptKind}|${offer.segment.startSec}` : null;
  useEffect(() => {
    if (!external || !promptKey || !promptKind) return;
    sendPrompt(promptKind);
    return () => sendPrompt(null);
  }, [external, promptKey, promptKind]);
  useEffect(() => {
    if (!external) return;
    return subscribeEvents((event) => {
      if (event.type === "client-message" && event.args[0] === "zeiler-skip") skip();
    });
  }, [external, subscribeEvents, skip]);

  return {
    offer,
    skip,
    skipped,
    dismissSkipped: useCallback(() => setSkipped(null), []),
    inOutro: active?.kind === "outro",
  };
}
