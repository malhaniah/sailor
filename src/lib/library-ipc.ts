/**
 * IPC surface of the local library (src-tauri/src/commands/library.rs). Self-contained,
 * like player-ipc.ts: its own types, one-line wrappers, and the change event.
 *
 * The library lives in Rust (`library.json`, Stremio-compatible items from the
 * `zeiler-account` crate). The UI reads views of it and makes the user's own changes;
 * playback progress is never sent from here — the Player passes a `PlaybackContext` to
 * `player_start` and Rust tracks progress from the player's events. Every committed
 * change, whoever made it, is announced as `library://changed` with the changed ids.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { MetaItem, PosterShape, Video } from "./types";

// --- input -----------------------------------------------------------------------------

/** What the library keeps of a meta (Rust `zeiler_account::MetaSummary`). */
export interface MetaSummary {
  id: string;
  type: string;
  name: string;
  /** http(s) only; anything else is dropped in Rust */
  poster: string | null;
  posterShape: PosterShape;
  /** all of the meta's hints: the item keeps them, as Stremio's apps do */
  behaviorHints: {
    defaultVideoId: string | null;
    hasScheduledVideos: boolean;
    isLive: boolean;
    [key: string]: unknown;
  };
}

/** One video of a meta: enough for the watched marks and the next episode. */
export interface VideoRef {
  id: string;
  season: number | null;
  episode: number | null;
  /** RFC 3339 */
  released: string | null;
}

/** What a playback is, for `player_start`'s `context`. */
export interface PlaybackContext {
  meta: MetaSummary;
  /** the episode id, or the movie's own video id */
  videoId: string;
  /** the title's videos (empty for a movie) */
  videos: VideoRef[];
}

export function metaSummary(meta: MetaItem): MetaSummary {
  return {
    id: meta.id,
    type: meta.type,
    name: meta.name,
    poster: meta.poster ?? null,
    posterShape: meta.posterShape,
    behaviorHints: {
      ...meta.behaviorHints,
      defaultVideoId: meta.behaviorHints.defaultVideoId ?? null,
    },
  };
}

export function videoRefs(videos: Pick<Video, "id" | "season" | "episode" | "released">[]): VideoRef[] {
  return videos.map((v) => ({
    id: v.id,
    season: v.season ?? null,
    episode: v.episode ?? null,
    released: v.released ?? null,
  }));
}

// --- views -----------------------------------------------------------------------------

export type LibraryFilter = "continueWatching" | "library" | "history";
export type LibrarySort =
  | "lastWatched"
  | "name"
  | "nameReverse"
  | "timesWatched"
  | "watched"
  | "notWatched";

export interface ResumePoint {
  videoId: string;
  /** ms */
  timeOffset: number;
  /** ms */
  duration: number;
}

/** One library item (Rust `library::LibraryEntry`). Times RFC 3339, positions ms. */
export interface LibraryEntry {
  id: string;
  type: string;
  name: string;
  poster: string | null;
  posterShape: PosterShape;
  /** added to the library (and not removed) */
  inLibrary: boolean;
  inContinueWatching: boolean;
  addedAt: string | null;
  /** last change of any kind; Continue Watching sorts by it */
  updatedAt: string;
  lastWatched: string | null;
  /** the video played last, or the next episode Continue Watching moved on to */
  videoId: string | null;
  /** resume position in `videoId` (0: none; 1: from the beginning) */
  timeOffset: number;
  /** duration of `videoId` (0: unknown) */
  duration: number;
  timesWatched: number;
  /** watched at least once */
  watched: boolean;
  /** where to resume `videoId`, when there is a real position */
  resume: ResumePoint | null;
}

/** An item with the watched marks of the videos it was asked about. */
export interface LibraryDetail {
  entry: LibraryEntry;
  watchedVideoIds: string[];
}

export interface LibraryQuery {
  filter: LibraryFilter;
  /** the `library` view's order (the others have their own) */
  sort?: LibrarySort;
  type?: string | null;
  limit?: number | null;
}

// --- commands --------------------------------------------------------------------------

export const libraryList = (query: LibraryQuery) =>
  invoke<LibraryEntry[]>("library_list", { query });
/** null when the title was never added, played or marked */
export const libraryGet = (id: string, videos: VideoRef[] = []) =>
  invoke<LibraryDetail | null>("library_get", { id, videos });
/** Add to / remove from the library; null when removing a title that was never there. */
export const librarySet = (meta: MetaSummary, inLibrary: boolean) =>
  invoke<LibraryEntry | null>("library_set", { request: { meta, inLibrary } });
/**
 * Mark the whole title (`videoIds` empty, no `season`), some episodes, or one season;
 * `videos` is the full list. Marking the episode Continue Watching points at moves it
 * on, as Stremio's apps do.
 */
export const libraryMarkWatched = (request: {
  meta: MetaSummary;
  videos: VideoRef[];
  videoIds: string[];
  /** a whole season instead of `videoIds` */
  season?: number | null;
  watched: boolean;
}) => invoke<LibraryDetail | null>("library_mark_watched", { request });
/** Take a title out of Continue Watching (drops its resume position). */
export const libraryClearProgress = (id: string) =>
  invoke<LibraryEntry | null>("library_clear_progress", { id });

// --- events ----------------------------------------------------------------------------

export const LIBRARY_CHANGED = "library://changed";

/** Subscribe to library changes (the ids that changed). */
export function onLibraryChanged(handler: (ids: string[]) => void): Promise<UnlistenFn> {
  return listen<{ ids: string[] }>(LIBRARY_CHANGED, (e) => handler(e.payload.ids));
}
