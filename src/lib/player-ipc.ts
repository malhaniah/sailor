/**
 * IPC surface of the player (and the bits of the engine the player page needs).
 * Deliberately self-contained — no imports from lib/ipc.ts — so the player can be
 * wired into any navigation shell without coupling.
 *
 * One backend-neutral player (src-tauri/src/commands/mpv/): `playerStart` picks
 * embedded libmpv or external mpv (remote-controlled over JSON IPC) in Rust and says
 * which it used; every other call acts on whichever is running, and both backends
 * emit the same `player://event` stream. Property writes, reads and commands are
 * validated against allow-lists in Rust (commands/mpv/validate.rs). The pre-M2 command
 * names (`player_play`, `player_embedded_*`, `player_status`) still exist in Rust for one
 * release (commands/mpv/aliases.rs) but nothing in the UI calls them any more.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PlaybackContext } from "./library-ipc";
import type {
  AddonStreams,
  AddonSubtitles,
  PlayerBackend,
  Settings,
  SettingsPatch,
  SubtitleStyle,
} from "./types";

export type { PlayerBackend };

// --- requests ---------------------------------------------------------------------------

export interface PlaybackRequest {
  url: string;
  title?: string | null;
  headers?: [string, string][];
  userAgent?: string | null;
  referrer?: string | null;
  startAtSec?: number | null;
}

/** Physical pixels, relative to the window's client area. */
export interface Geometry {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The backend a session actually runs on. */
export type ActiveBackend = "embedded" | "external";

/** What `playerStart` did. */
export interface StartResult {
  backend: ActiveBackend;
  /** Set when built-in playback was wanted ("auto") but failed to start: why. */
  fallbackReason?: string;
  /**
   * False only for the last-resort external launch without JSON IPC: mpv plays, but
   * Zeiler cannot control it or see its state (no events arrive).
   */
  remoteControl: boolean;
}

export interface PlayerBackends {
  embedded: { available: boolean; reason?: string; libmpvVersion?: string };
  external: { available: boolean; path?: string | null };
}

/** What each backend can do on this machine and build. */
export const playerBackends = () => invoke<PlayerBackends>("player_backends");
/**
 * Start (or replace) playback. `backend` defaults to "auto", which follows the
 * `playerBackend` setting; Rust falls back from embedded to external mpv itself. With a
 * `context` (the title and video), Rust saves the playback's progress to the library.
 */
export const playerStart = (
  request: PlaybackRequest,
  backend: PlayerBackend = "auto",
  context: PlaybackContext | null = null,
) => invoke<StartResult>("player_start", { request, backend, context });
/** Load another file into the running player (same window, same backend); `context`
 * tracks the new file's progress (the replaced file's is saved first). */
export const playerLoad = (request: PlaybackRequest, context: PlaybackContext | null = null) =>
  invoke<void>("player_load", { request, context });
/** Stop the player; with `only`, just a session on that backend (none = any backend). */
export const playerStop = (only?: ActiveBackend) =>
  invoke<void>("player_stop", only ? { only } : {});
export const playerSetGeometry = (geometry: Geometry) =>
  invoke<void>("player_set_geometry", { geometry });
/**
 * Re-assert the built-in video surface after a window change (Windows: transparent
 * webview, video window pinned under it, keyboard focus). A no-op when no built-in
 * video plays.
 */
export const playerSurfaceRefresh = () => invoke<void>("player_surface_refresh");
/** Set an allow-listed property (Rust validates name and value). */
export const playerSetProperty = (name: string, value: boolean | number | string) =>
  invoke<void>("player_set_property", { name, value });
/** Run an allow-listed command (Rust validates every argument). */
export const playerCommand = (...args: (string | number)[]) =>
  invoke<void>("player_command", { args: args.map(String) });
/** Read an allow-listed property: diagnostics (codecs, fps, drops…) or an observed one. */
export const playerGetProperty = (name: string) =>
  invoke<unknown>("player_get_property", { name });

// engine commands the player page uses to resolve/clean up torrent streams
export interface ResolvedStream {
  streamUrl: string;
  infoHash: string;
  fileName?: string | null;
  fileLength?: number | null;
}
export const engineResolve = (request: {
  infoHash: string;
  fileIdx?: number | null;
  sources?: string[];
  filename?: string | null;
  fileMustInclude?: string[];
}) => invoke<ResolvedStream>("engine_resolve", { request });
export const engineStats = (infoHash: string) =>
  invoke<Record<string, unknown>>("engine_stats", { infoHash });
export const engineRemove = (infoHash: string, deleteFiles: boolean) =>
  invoke<void>("engine_remove", { infoHash, deleteFiles });
export const settingsGet = () =>
  invoke<
    Pick<
      Settings,
      | "deleteOnStop"
      | "playerBackend"
      | "subtitles"
      | "audio"
      | "skip"
      | "bingeWatching"
      | "upNextSeconds"
    >
  >("settings_get");
/** Merges these keys into the settings (a group field by field), as `settings_patch`. */
export const settingsPatch = (patch: SettingsPatch) =>
  invoke<Settings>("settings_patch", { patch });

// --- subtitles (src-tauri/src/commands/mpv/subtitles.rs) ----------------------------------

/**
 * Every subtitle addon's subtitles for a video (`subtitles_get`). `videoHash`,
 * `videoSize` and `filename` come from the stream's hints and let addons match the
 * exact release; Rust leaves out any that is malformed.
 */
export const subtitlesGet = (query: {
  type: string;
  id: string;
  videoHash?: string | null;
  videoSize?: number | null;
  filename?: string | null;
}) => invoke<AddonSubtitles[]>("subtitles_get", query);

/** What an add did: `file` names the new track (its `external-filename` ends with it). */
export interface AddedSubtitle {
  file: string;
  format: "srt" | "vtt" | "ass" | "sub";
  /** the encoding the file was read as, e.g. "UTF-8", "windows-1256" */
  encoding: string;
}

/** Download, normalize (gzip, encoding) and add a subtitle from an http(s) link. */
export const playerSubtitleAdd = (request: {
  url: string;
  lang?: string | null;
  label?: string | null;
  select?: boolean;
  /** an encoding label that replaces detection, e.g. "windows-1256" */
  encoding?: string | null;
}) => invoke<AddedSubtitle>("player_subtitle_add", { request });
/** Add an already added subtitle again, read as `encoding` (null: detect), selected. */
export const playerSubtitleReencode = (file: string, encoding: string | null) =>
  invoke<AddedSubtitle>("player_subtitle_reencode", { file, encoding });
/** Open the OS file dialog (from Rust) and add the picked file; null when cancelled. */
export const playerSubtitlePickLocal = () =>
  invoke<AddedSubtitle | null>("player_subtitle_pick_local");
/** Apply subtitle style fields live (save them with `settingsPatch`); resolves to the mpv
 * properties the player refused (an older external mpv lacks some). */
export const playerSetSubStyle = (style: Partial<SubtitleStyle>) =>
  invoke<string[]>("player_set_sub_style", { style });

/** The choices remembered for one title, so the next episode starts the same way. */
export interface TitlePrefs {
  metaId: string;
  /** a language code, or "off" */
  subLang: string | null;
  /** where that subtitle came from */
  subSource: "embedded" | "stream" | "addon" | null;
  audioLang: string | null;
  subDelaySec: number | null;
  rate: number | null;
}
export const playerPrefsGet = (metaId: string) =>
  invoke<TitlePrefs | null>("player_prefs_get", { metaId });
/** Merge choices for a title (null forgets one). */
export const playerPrefsSet = (
  metaId: string,
  patch: Partial<Omit<TitlePrefs, "metaId">>,
) => invoke<TitlePrefs>("player_prefs_set", { metaId, patch });

// --- skip and next episode (src-tauri/src/commands/skip.rs, commands/mpv/osd.rs) -----------

/** A skip segment from AniSkip or TheIntroDB (Rust `zeiler_skip::Segment`). */
export interface OnlineSegment {
  kind: "intro" | "recap" | "outro";
  startSec: number;
  endSec: number;
  source: "aniskip" | "introdb";
}

/** Where a video's intro, recap and credits are, from AniSkip (anime) or TheIntroDB
 * (IMDb/TMDB titles); empty when online lookups are off or nothing is known. */
export const skipSegmentsGet = (query: { metaId: string; videoId: string; durationSec: number }) =>
  invoke<OnlineSegment[]>("skip_segments_get", { query });
/** Offer a skip in external mpv's own window (OSD text, Enter sends `zeiler-skip` back
 * as a client-message); null takes it back. A no-op for built-in playback. */
export const playerOsdPrompt = (kind: "intro" | "recap" | "outro" | "next" | null) =>
  invoke<void>("player_osd_prompt", { kind });
/** Every addon's streams for a video (the next episode's, to binge with). */
export const streamsGet = (type: string, id: string) =>
  invoke<AddonStreams[]>("streams_get", { type, id });

/** The OpenSubtitles hash and size of the file playing (null: unknown), for subtitle
 * addons when the stream names none. Rust reads the stream itself (two range requests). */
export const playerVideoHash = () =>
  invoke<{ hash: string; size: number } | null>("player_video_hash");
/** Even out the volume (a fixed, labelled audio filter) or stop; save it in
 * `audio.normalize` with `settingsPatch`. */
export const playerSetAudioNormalize = (on: boolean) =>
  invoke<void>("player_set_audio_normalize", { on });

/** How a subtitle file dropped on the window went (Rust emits it while a player runs). */
export type SubtitleDropped =
  | { name: string; added: AddedSubtitle }
  | { name: string; error: string };
export function onSubtitleDropped(
  handler: (event: SubtitleDropped) => void,
): Promise<UnlistenFn> {
  return listen<SubtitleDropped>("subtitles://dropped", (e) => handler(e.payload));
}

export function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

// --- events -----------------------------------------------------------------------------

/** One entry of mpv's `chapter-list` (observed as JSON). */
export interface MpvChapter {
  time: number;
  title?: string;
}

/** One entry of mpv's `track-list` (observed as JSON). */
export interface MpvTrack {
  id: number;
  type: "video" | "audio" | "sub";
  title?: string;
  lang?: string;
  codec?: string;
  default?: boolean;
  forced?: boolean;
  "hearing-impaired"?: boolean;
  /** added from a file (sub-add), not part of the stream */
  external?: boolean;
  "external-filename"?: string;
  /** mpv sets it for the `sid` track and the `secondary-sid` track alike */
  selected?: boolean;
  /** 0: the main selection (`sid`), 1: the second (`secondary-sid`) */
  "main-selection"?: number;
  "demux-w"?: number;
  "demux-h"?: number;
  /** an HLS variant's bitrate, bits/s */
  "hls-bitrate"?: number;
  "demux-bitrate"?: number;
  /** cover art or a still image, not video */
  albumart?: boolean;
  image?: boolean;
  "demux-channel-count"?: number;
  /** mpv's channel layout name, e.g. "5.1(side)" */
  "demux-channels"?: string;
}

/**
 * The player's event stream (Rust `zeiler_player::PlayerEvent`), identical for the
 * embedded and the external backend. Observed properties: time-pos (throttled to
 * 200 ms), duration, pause, paused-for-cache, cache-buffering-state, eof-reached,
 * seeking, volume, mute, media-title, track-list, chapter-list, aid, sid, vid,
 * secondary-sid, sub-delay, audio-delay, speed, chapter, dwidth, dheight,
 * demuxer-cache-time. Track ids arrive in mpv's string form ("1", "no", "auto");
 * a value is `null` while the property is unavailable.
 */
export type PlayerEvent =
  | { type: "property"; name: string; value: unknown }
  | { type: "start-file" }
  | { type: "file-loaded" }
  | { type: "seek" }
  | { type: "playback-restart" }
  | {
      type: "end-file";
      reason: "eof" | "stop" | "quit" | "error" | "redirect" | "unknown";
      /** mpv's error text (external backend), e.g. "loading failed" */
      fileError?: string;
    }
  | { type: "error"; message: string }
  /** a `script-message` from the user's mpv bindings (external backend) */
  | { type: "client-message"; args: string[] }
  /** the core is gone (stopped, crashed, or the external window was closed) */
  | { type: "shutdown" };

/** Subscribe to the player's event stream (`player://event`). */
export function onPlayerEvent(handler: (event: PlayerEvent) => void): Promise<UnlistenFn> {
  return listen<PlayerEvent>("player://event", (e) => handler(e.payload));
}
