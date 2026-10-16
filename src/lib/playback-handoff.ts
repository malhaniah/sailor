/**
 * Playback handoff — the one way to start playback in Zeiler.
 *
 * Every playback starts on the Player screen (`#/player`). The caller describes what to
 * play as a `PlayerLaunch` and calls `openPlayer(launch)`: the launch is stashed in memory
 * under a random one-shot id and the app navigates to `#/player?h=<id>`. The Player
 * screen `takeLaunch(id)`s it exactly once.
 *
 * Stream URLs, request headers and addon transport URLs (which may embed credentials,
 * e.g. config-in-URL addons or IPTV logins) therefore never reach `location.hash`, the
 * history stack or a log line: the hash carries only the id. A reload, or a stale
 * history entry, finds no launch and the Player shows "nothing to play". (The dev-only
 * VITE_ZEILER_AUTOPLAY hook still accepts the legacy `url=`/`infoHash=` query params; see
 * `src/pages/player/launch.ts`.)
 *
 * This module also owns `playerActive()`: the single signal that the Player screen is up
 * and the app shell is mounted but hidden underneath it (see `src/main.tsx`). The shell's
 * global key listeners read it so they stay quiet while the player has the keyboard.
 */
import { type PlaybackContext, videoRefs } from "./library-ipc";
import type { PlaybackRequest } from "./player-ipc";
import type { MetaItem, PlayerBackend, PosterShape, Stream, Subtitle, Video } from "./types";

// --- the launch ---------------------------------------------------------------------------

/** A direct http(s) stream. Headers come from `behaviorHints.proxyHeaders.request`. */
export interface UrlSource {
  kind: "url";
  url: string;
  headers: [string, string][];
  userAgent: string | null;
  referrer: string | null;
}

/** A torrent stream; the Player resolves it through the engine (`engine_resolve`). */
export interface TorrentSource {
  kind: "torrent";
  infoHash: string;
  fileIdx: number | null;
  /** tracker / DHT sources from the stream's `sources` (`announce`) */
  sources: string[];
  fileMustInclude: string[];
  filename: string | null;
}

export type LaunchSource = UrlSource | TorrentSource;

export interface LaunchMeta {
  type: string;
  id: string;
  name: string;
  poster: string | null;
  posterShape: PosterShape;
  /** wide backdrop art; the external-mpv screen fills the window with it */
  background: string | null;
  /** the series' episode list, for next-episode and binge lookups */
  videos: Video[];
  /** all of the meta's hints (the library keeps them, as Stremio does) */
  behaviorHints: {
    /** the video a movie's streams are asked for when it has no `video` */
    defaultVideoId: string | null;
    hasScheduledVideos: boolean;
    [key: string]: unknown;
  };
}

export interface LaunchVideo {
  id: string;
  season: number | null;
  episode: number | null;
  title: string | null;
  thumbnail: string | null;
}

export interface LaunchStream {
  name: string | null;
  behaviorHints: {
    bingeGroup: string | null;
    filename: string | null;
    videoHash: string | null;
    videoSize: number | null;
  };
  subtitles: Subtitle[];
}

export interface PlayerLaunch {
  source: LaunchSource;
  /** media title handed to the player (window title / OSD) */
  title: string | null;
  startAtSec: number | null;
  meta: LaunchMeta | null;
  video: LaunchVideo | null;
  stream: LaunchStream | null;
  /** the addon that served the stream; null for streams embedded in the meta */
  transportUrl: string | null;
  addonName: string | null;
  /** live TV / channel: no duration, no seeking, no progress */
  live: boolean;
  /**
   * Which player to use for this launch: "auto" follows the `playerBackend` setting
   * (Rust decides, falling back from built-in to mpv); the picker's "Open in external
   * mpv" forces "external".
   */
  backend: PlayerBackend;
}

/**
 * Builds the launch for one picker row. Returns null for streams the Player cannot play
 * (YouTube, external links, unsupported sources), which keep their own paths.
 */
export function launchForStream({
  meta,
  video,
  stream,
  transportUrl,
  addonName,
}: {
  meta: MetaItem;
  video: Video | null;
  stream: Stream;
  transportUrl: string | null;
  addonName: string | null;
}): PlayerLaunch | null {
  const hints = stream.behaviorHints;
  let source: LaunchSource;
  if (stream.url) {
    const headerEntries = Object.entries(hints.proxyHeaders?.request ?? {});
    const uaEntry = headerEntries.find(([name]) => /^user-agent$/i.test(name));
    const referrerEntry = headerEntries.find(([name]) => /^referer$/i.test(name));
    source = {
      kind: "url",
      url: stream.url,
      headers: headerEntries.filter((e) => e !== uaEntry && e !== referrerEntry),
      userAgent: uaEntry?.[1] ?? null,
      referrer: referrerEntry?.[1] ?? null,
    };
  } else if (stream.infoHash) {
    source = {
      kind: "torrent",
      infoHash: stream.infoHash,
      fileIdx: stream.fileIdx ?? null,
      sources: stream.announce ?? [],
      fileMustInclude: stream.fileMustInclude ?? [],
      filename: hints.filename ?? null,
    };
  } else {
    return null;
  }

  const title =
    hints.filename ??
    (video
      ? `${meta.name} · S${video.season ?? "?"}E${video.episode ?? "?"}`
      : (stream.name ?? meta.name));

  return {
    source,
    title,
    startAtSec: null,
    meta: {
      type: meta.type,
      id: meta.id,
      name: meta.name,
      poster: meta.poster ?? null,
      posterShape: meta.posterShape,
      background: meta.background ?? null,
      videos: meta.videos,
      behaviorHints: {
        ...meta.behaviorHints,
        defaultVideoId: meta.behaviorHints.defaultVideoId ?? null,
      },
    },
    video: video
      ? {
          id: video.id,
          season: video.season ?? null,
          episode: video.episode ?? null,
          title: video.title ?? null,
          thumbnail: video.thumbnail ?? null,
        }
      : null,
    stream: {
      name: stream.name ?? null,
      behaviorHints: {
        bingeGroup: hints.bingeGroup ?? null,
        filename: hints.filename ?? null,
        videoHash: hints.videoHash ?? null,
        videoSize: hints.videoSize ?? null,
      },
      subtitles: stream.subtitles,
    },
    transportUrl,
    addonName,
    live: meta.behaviorHints.isLive || meta.type === "tv",
    backend: "auto",
  };
}

/**
 * What `player_start` needs to save this launch's progress to the library (Rust tracks
 * it from the player's events). Null for live TV, which is never tracked, and for
 * launches without a meta (the dev-only legacy routes).
 */
export function playbackContext(launch: PlayerLaunch): PlaybackContext | null {
  const { meta } = launch;
  if (!meta || launch.live) return null;
  return {
    meta: {
      id: meta.id,
      type: meta.type,
      name: meta.name,
      poster: meta.poster,
      posterShape: meta.posterShape,
      behaviorHints: { ...meta.behaviorHints, isLive: false },
    },
    // the id the picker asked the addons for (PlayPicker)
    videoId: launch.video?.id ?? meta.behaviorHints.defaultVideoId ?? meta.id,
    videos: videoRefs(meta.videos),
  };
}

/** The player request for a direct stream (torrents get their URL from the engine). */
export function urlRequest(source: UrlSource, launch: PlayerLaunch): PlaybackRequest {
  return {
    url: source.url,
    title: launch.title,
    headers: source.headers,
    userAgent: source.userAgent,
    referrer: source.referrer,
    startAtSec: launch.startAtSec,
  };
}

// --- one-shot stash -----------------------------------------------------------------------

/** An untaken launch is dropped after this long (the user never reached the player). */
const TTL_MS = 10 * 60 * 1000;
const MAX_PENDING = 8;

const pending = new Map<string, { launch: PlayerLaunch; at: number }>();

function randomId(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Keeps `launch` in memory and returns the random one-shot id that names it. */
export function stashLaunch(launch: PlayerLaunch): string {
  const now = Date.now();
  for (const [id, entry] of pending) {
    if (now - entry.at > TTL_MS) pending.delete(id);
  }
  // Map iteration is insertion order: drop the oldest beyond the cap
  while (pending.size >= MAX_PENDING) {
    const oldest = pending.keys().next().value;
    if (oldest === undefined) break;
    pending.delete(oldest);
  }
  const id = randomId();
  pending.set(id, { launch, at: now });
  return id;
}

/** Returns the launch named by `id` once; every later call (or an expired id) gets null. */
export function takeLaunch(id: string): PlayerLaunch | null {
  const entry = pending.get(id);
  pending.delete(id);
  if (!entry || Date.now() - entry.at > TTL_MS) return null;
  return entry.launch;
}

// --- the player route ---------------------------------------------------------------------

const PLAYER_ROUTE = "#/player";

export function isPlayerHash(hash: string): boolean {
  return hash.startsWith(PLAYER_ROUTE);
}

/** Stashes `launch` and navigates to the Player screen (a new history entry, so Back
 * returns to wherever the user was). Returns the launch id, which names the launch in
 * `subscribeLaunchFailures`. */
export function openPlayer(launch: PlayerLaunch): string {
  const id = stashLaunch(launch);
  window.location.hash = `${PLAYER_ROUTE}?h=${id}`;
  return id;
}

// --- playback outcome ---------------------------------------------------------------------

const failureListeners = new Set<(id: string) => void>();

/** The Player calls this when the launch named by `id` failed to play, so the screen that
 * opened it (still mounted under the player) can mark the stream it came from. */
export function reportLaunchFailed(id: string): void {
  for (const listener of [...failureListeners]) listener(id);
}

/** Subscribes to launch failures (by launch id); returns the unsubscribe function. */
export function subscribeLaunchFailures(listener: (id: string) => void): () => void {
  failureListeners.add(listener);
  return () => {
    failureListeners.delete(listener);
  };
}

/** Leaves the Player screen: back to the previous history entry, or to the app's start
 * when the player is the first entry (e.g. it was reloaded in a fresh window). */
export function leavePlayer(): void {
  if (window.history.length > 1) window.history.back();
  else window.location.hash = "#/";
}

const routeListeners = new Set<() => void>();

// One window listener calls every subscriber synchronously, so all of them observe the
// route change before React commits the resulting render (see the scroll snapshot in
// src/main.tsx).
window.addEventListener("hashchange", () => {
  for (const listener of [...routeListeners]) listener();
});

/** Subscribes to route (hash) changes; returns the unsubscribe function. */
export function subscribeRoute(listener: () => void): () => void {
  routeListeners.add(listener);
  return () => {
    routeListeners.delete(listener);
  };
}

/**
 * True while the Player screen is up. The app shell stays mounted (hidden and `inert`)
 * underneath it, so its window-level key handlers must check this and do nothing.
 */
export function playerActive(): boolean {
  return isPlayerHash(window.location.hash);
}
