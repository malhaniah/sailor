/**
 * The player page's slower mpv state (tracks, volume, title…) and its phase. The fast
 * properties — position, duration, pause, buffering — live in the playback clock
 * (`playback-clock.ts`) instead.
 */
import type { MpvTrack, PlayerEvent } from "../../lib/player-ipc";

export interface MpvState {
  eofReached: boolean;
  volume: number;
  mute: boolean;
  mediaTitle: string | null;
  tracks: MpvTrack[];
  aid: string;
  sid: string;
  vid: string;
}

export const INITIAL_MPV: MpvState = {
  eofReached: false,
  volume: 100,
  mute: false,
  mediaTitle: null,
  tracks: [],
  aid: "auto",
  sid: "auto",
  vid: "auto",
};

export function mpvReducer(state: MpvState, event: PlayerEvent): MpvState {
  if (event.type === "start-file") {
    return { ...state, eofReached: false, tracks: [] };
  }
  if (event.type !== "property") return state;
  const v = event.value;
  switch (event.name) {
    case "eof-reached":
      return typeof v === "boolean" ? { ...state, eofReached: v } : state;
    case "volume":
      return typeof v === "number" ? { ...state, volume: v } : state;
    case "mute":
      return typeof v === "boolean" ? { ...state, mute: v } : state;
    case "media-title":
      return typeof v === "string" ? { ...state, mediaTitle: v } : state;
    case "track-list":
      return Array.isArray(v) ? { ...state, tracks: v as MpvTrack[] } : state;
    case "aid":
      return { ...state, aid: String(v) };
    case "sid":
      return { ...state, sid: String(v) };
    case "vid":
      return { ...state, vid: String(v) };
    default:
      return state;
  }
}

/** Which backend this session plays on; `unknown` until the availability check answers. */
export type Backend = "unknown" | "embedded" | "external";

export type Phase =
  | { kind: "loading"; label: string }
  | { kind: "playing" }
  /** handed to an external mpv window (non-embedded builds) */
  | { kind: "external" }
  | { kind: "ended" }
  /** `streamUrl`: offered for copying when the external player could not be launched */
  | { kind: "error"; message: string; streamUrl?: string };

export function formatTime(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(sec).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}
