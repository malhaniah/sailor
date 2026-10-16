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
import type { PlayerBackend } from "./types";

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
 * `playerBackend` setting; Rust falls back from embedded to external mpv itself.
 */
export const playerStart = (request: PlaybackRequest, backend: PlayerBackend = "auto") =>
  invoke<StartResult>("player_start", { request, backend });
/** Load another file into the running player (same window, same backend). */
export const playerLoad = (request: PlaybackRequest) => invoke<void>("player_load", { request });
/** Stop whatever is playing, on any backend. */
/** Stop the player; with `only`, just a session on that backend (none = any backend). */
export const playerStop = (only?: ActiveBackend) =>
  invoke<void>("player_stop", only ? { only } : {});
export const playerSetGeometry = (geometry: Geometry) =>
  invoke<void>("player_set_geometry", { geometry });
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
  invoke<{ deleteOnStop: boolean; playerBackend: PlayerBackend }>("settings_get");

export function errorMessage(error: unknown): string {
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

// --- events -----------------------------------------------------------------------------

/** One entry of mpv's `track-list` (observed as JSON). */
export interface MpvTrack {
  id: number;
  type: "video" | "audio" | "sub";
  title?: string;
  lang?: string;
  codec?: string;
  default?: boolean;
  selected?: boolean;
  "demux-w"?: number;
  "demux-h"?: number;
  "demux-channel-count"?: number;
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
