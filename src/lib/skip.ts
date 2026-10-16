/**
 * Skip segments for the player: where the intro, recap and credits are, and which one
 * the position is in.
 *
 * Sources, first non-empty wins (they are not merged):
 * 1. AniSkip or TheIntroDB, asked in Rust (`skip_segments_get`, crates/zeiler-skip);
 * 2. the file's own chapter names (`chapterSegments`).
 * When a next episode exists and no source found credits, the last 90 seconds count
 * as credits (`withSyntheticOutro`), so "Next Episode" shows there.
 *
 * Adapted from Harbor's `src/lib/skip-intro/{chapters,index}.ts` (MIT) — see
 * THIRD-PARTY-NOTICES.md.
 */
import type { MpvChapter } from "./player-ipc";

export type SegmentKind = "intro" | "recap" | "outro";
export type SegmentSource = "aniskip" | "introdb" | "chapter" | "synthetic";

export interface SkipSegment {
  kind: SegmentKind;
  startSec: number;
  endSec: number;
  source: SegmentSource;
}

/** The end of a segment is this much earlier for the pill: it does not flash up for the
 * last moment of a segment, and a skip that lands right at the end leaves it. */
const END_GUARD_SEC = 0.75;
/** A chapter that is the last one, in a file of unknown length, is taken to be this long. */
const LAST_CHAPTER_SEC = 90;
/** The synthetic credits: the last this-many seconds, when a next episode exists. */
export const SYNTHETIC_OUTRO_SEC = 90;

// Harbor's chapter-title classifier: recap first, then intro (so "Opening Credits" is
// the intro), then outro.
const RECAP = /\b(recap|previously)\b/i;
const INTRO = /\b(opening|op|intro|theme song)\b/i;
const OUTRO = /\b(ending|ed|outro|credits)\b/i;
/** Chapter titles that name nothing but the segment: confident enough to skip by itself. */
const EXACT =
  /^\s*(recap|previously(\s+on\b.*)?|intro|opening|op|opening credits|ending|ed|outro|credits|end credits|closing credits)\s*$/i;

/** What a chapter title says it is, if anything. */
export function classifyChapter(title: string): SegmentKind | null {
  if (RECAP.test(title)) return "recap";
  if (INTRO.test(title)) return "intro";
  if (OUTRO.test(title)) return "outro";
  return null;
}

/** Whether a chapter title is exactly a segment's name ("Intro", "Opening", "Credits"…). */
export function isExactChapter(title: string): boolean {
  return EXACT.test(title);
}

export interface ChapterSegment extends SkipSegment {
  source: "chapter";
  /** the title is exactly the segment's name, not just mentions it */
  exact: boolean;
}

/** Segments from mpv's chapter list: a chapter whose title names a segment runs to the
 * next chapter (or the end of the file). */
export function chapterSegments(chapters: MpvChapter[], durationSec: number): ChapterSegment[] {
  const sorted = chapters
    .filter((c) => Number.isFinite(c.time))
    .slice()
    .sort((a, b) => a.time - b.time);
  const out: ChapterSegment[] = [];
  sorted.forEach((chapter, i) => {
    const title = chapter.title?.trim() ?? "";
    const kind = classifyChapter(title);
    if (!kind) return;
    const next = sorted[i + 1]?.time;
    const end = next ?? (durationSec > 0 ? durationSec : chapter.time + LAST_CHAPTER_SEC);
    if (end <= chapter.time) return;
    out.push({
      kind,
      startSec: Math.max(0, chapter.time),
      endSec: end,
      source: "chapter",
      exact: isExactChapter(title),
    });
  });
  return out;
}

/** Credits for the last 90 s when a next episode exists and nothing else found credits
 * (never in a video too short for it to mean anything). */
export function withSyntheticOutro(
  segments: SkipSegment[],
  durationSec: number,
  hasNext: boolean,
): SkipSegment[] {
  if (!hasNext || durationSec < SYNTHETIC_OUTRO_SEC * 3) return segments;
  if (segments.some((s) => s.kind === "outro")) return segments;
  return [
    ...segments,
    {
      kind: "outro",
      startSec: durationSec - SYNTHETIC_OUTRO_SEC,
      endSec: durationSec,
      source: "synthetic",
    },
  ];
}

/** The segment the position is in (the first, if they overlap). */
export function activeSegmentIndex(segments: SkipSegment[], positionSec: number): number {
  return segments.findIndex(
    (s) => s.startSec <= positionSec && positionSec < s.endSec - END_GUARD_SEC,
  );
}

/** Whether a segment may be skipped without asking: never the synthetic credits, and a
 * chapter only when its title is exactly the segment's name. */
export function mayAutoSkip(segment: SkipSegment): boolean {
  if (segment.source === "synthetic") return false;
  if (segment.source === "chapter") return (segment as ChapterSegment).exact === true;
  return true;
}

/** How long after a segment starts playback may still enter it for an automatic skip,
 * and the biggest jump between two positions that still counts as playing on. */
export const AUTO_SKIP_WINDOW_SEC = 2;
/** A segment that becomes known late (an online answer) still counts as entered while
 * playback is this far into it, if nothing was seeked in the file yet. */
export const LATE_ENTRY_SEC = 10;
export const CONTINUOUS_STEP_SEC = 1.5;

/**
 * Whether playback just entered `segment` by playing on (not by a seek): the position
 * is in its first two seconds, and the one before was before its start, at most a step
 * away. `previous` is null for the first position of a file (a recap at 0:00 counts).
 */
export function enteredByPlaying(
  segment: SkipSegment,
  previous: number | null,
  position: number,
): boolean {
  if (position < segment.startSec || position >= segment.startSec + AUTO_SKIP_WINDOW_SEC) {
    return false;
  }
  if (previous === null) return true;
  return previous < segment.startSec && position - previous <= CONTINUOUS_STEP_SEC;
}

export function segmentLabel(kind: SegmentKind): string {
  return kind === "intro" ? "intro" : kind === "recap" ? "recap" : "credits";
}
