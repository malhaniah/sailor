/**
 * IPC surface of the embedded player (and the bits of the engine the player
 * page needs). Deliberately self-contained — no imports from lib/ipc.ts — so
 * the player can be wired into any navigation shell without coupling.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

export const playerEmbeddedAvailable = () => invoke<boolean>("player_embedded_available");
export const playerEmbeddedStart = (request: PlaybackRequest) =>
  invoke<void>("player_embedded_start", { request });
export const playerEmbeddedStop = () => invoke<void>("player_embedded_stop");
export const playerSetGeometry = (geometry: Geometry) =>
  invoke<void>("player_set_geometry", { geometry });
export const playerSetProperty = (name: string, value: boolean | number | string) =>
  invoke<void>("player_embedded_set_property", { name, value });
export const playerCommand = (...args: (string | number)[]) =>
  invoke<void>("player_embedded_command", { args: args.map(String) });

// external-mpv fallback (same backend command the v0.1 flow used)
export const playerExternalPlay = (request: PlaybackRequest) =>
  invoke<void>("player_play", { request });

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
export const settingsGet = () => invoke<{ deleteOnStop: boolean }>("settings_get");

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

export type PlayerEvent =
  | { type: "property"; name: string; value: unknown }
  | { type: "start-file" }
  | { type: "file-loaded" }
  | { type: "seek" }
  | { type: "playback-restart" }
  | { type: "end-file"; reason: "eof" | "stop" | "quit" | "error" | "redirect" | "unknown" }
  | { type: "error"; message: string }
  | { type: "shutdown" };

/** Subscribe to the embedded player's event stream (`player://event`). */
export function onPlayerEvent(handler: (event: PlayerEvent) => void): Promise<UnlistenFn> {
  return listen<PlayerEvent>("player://event", (e) => handler(e.payload));
}
