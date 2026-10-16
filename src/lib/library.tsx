/**
 * The library in the UI. `LibraryProvider` keeps Continue Watching (the Home row and
 * the progress bars on posters) and re-fetches it on every `library://changed`; the
 * hooks below read one view (the Library page's tabs) or one title (Detail, the stream
 * picker) and re-fetch the same way. Everything is read from Rust (library-ipc.ts):
 * the UI never computes library state itself, so a change made anywhere — a click
 * here, or progress Rust tracks from the player — shows up everywhere.
 */
import {
  type ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { errorMessage, metaGet } from "./ipc";
import {
  type LibraryDetail,
  type LibraryEntry,
  type LibraryQuery,
  type VideoRef,
  libraryGet,
  libraryList,
  onLibraryChanged,
} from "./library-ipc";
import { useSearch } from "./store";
import type { MetaPreview } from "./types";
import { useView } from "./view";

type ChangeListener = (ids: string[]) => void;

interface LibraryCtx {
  /** Continue Watching, most recently changed first */
  continueWatching: LibraryEntry[];
  /** the first Continue Watching read has finished (or failed) */
  loaded: boolean;
  /** how far into its resume point each Continue Watching title is (0..1), by id */
  progress: ReadonlyMap<string, number>;
  /** listen to library changes (the ids that changed); returns the unsubscribe */
  subscribe: (listener: ChangeListener) => () => void;
}

const LibraryContext = createContext<LibraryCtx>({
  continueWatching: [],
  loaded: false,
  progress: new Map(),
  subscribe: () => () => {},
});

export const useLibrary = () => useContext(LibraryContext);

export function LibraryProvider({ children }: { children: ReactNode }) {
  const [continueWatching, setContinueWatching] = useState<LibraryEntry[]>([]);
  const [loaded, setLoaded] = useState(false);
  const listeners = useRef(new Set<ChangeListener>());
  // a read started later wins: an earlier answer arriving last is dropped
  const request = useRef(0);

  const load = useCallback(() => {
    const mine = ++request.current;
    libraryList({ filter: "continueWatching" })
      .then((entries) => mine === request.current && setContinueWatching(entries))
      .catch(() => {})
      .finally(() => mine === request.current && setLoaded(true));
  }, []);

  useEffect(() => {
    load();
    const unlisten = onLibraryChanged((ids) => {
      load();
      for (const listener of [...listeners.current]) listener(ids);
    });
    return () => {
      request.current++;
      void unlisten.then((off) => off());
    };
  }, [load]);

  const subscribe = useCallback((listener: ChangeListener) => {
    listeners.current.add(listener);
    return () => {
      listeners.current.delete(listener);
    };
  }, []);

  const value = useMemo<LibraryCtx>(() => {
    const progress = new Map<string, number>();
    for (const entry of continueWatching) {
      const fraction = resumeFraction(entry);
      if (fraction !== null) progress.set(entry.id, fraction);
    }
    return { continueWatching, loaded, progress, subscribe };
  }, [continueWatching, loaded, subscribe]);

  return <LibraryContext.Provider value={value}>{children}</LibraryContext.Provider>;
}

/** How far into its resume point a title is (0..1), or null when it has none. */
export function useTitleProgress(id: string): number | null {
  return useLibrary().progress.get(id) ?? null;
}

/** One view of the library (`library_list`), re-read on every change. */
export function useLibraryList(query: LibraryQuery): {
  /** null until the first read of this query answers */
  entries: LibraryEntry[] | null;
  error: string | null;
} {
  const { subscribe } = useLibrary();
  const [entries, setEntries] = useState<LibraryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  const { filter, sort, type, limit } = query;

  useEffect(() => {
    const load = () => {
      const mine = ++request.current;
      libraryList({ filter, sort, type, limit }).then(
        (list) => {
          if (mine !== request.current) return;
          setEntries(list);
          setError(null);
        },
        (e) => mine === request.current && setError(errorMessage(e)),
      );
    };
    setEntries(null);
    setError(null);
    load();
    const unsubscribe = subscribe(load);
    return () => {
      unsubscribe();
      request.current++;
    };
  }, [filter, sort, type, limit, subscribe]);

  return { entries, error };
}

/**
 * One title (`library_get`) with the watched marks of `videos` (pass a memoized list:
 * a new array re-reads), re-read whenever it changes. `id` null reads nothing.
 */
export function useLibraryItem(
  id: string | null,
  videos: VideoRef[],
): {
  /** null when the title was never added, played or marked (or not read yet) */
  detail: LibraryDetail | null;
  /** the first read has answered */
  loaded: boolean;
  /** read it again (after a change of ours, which may not have changed anything) */
  refresh: () => void;
} {
  const { subscribe } = useLibrary();
  const [detail, setDetail] = useState<LibraryDetail | null>(null);
  const [loaded, setLoaded] = useState(false);
  const request = useRef(0);

  const refresh = useCallback(() => {
    if (id === null) return;
    const mine = ++request.current;
    libraryGet(id, videos)
      .then((next) => mine === request.current && setDetail(next))
      .catch(() => {})
      .finally(() => mine === request.current && setLoaded(true));
  }, [id, videos]);

  useEffect(() => {
    setDetail(null);
    setLoaded(false);
    refresh();
    const unsubscribe = subscribe((ids) => {
      if (id !== null && ids.includes(id)) refresh();
    });
    return () => {
      unsubscribe();
      request.current++;
    };
  }, [id, refresh, subscribe]);

  return { detail, loaded, refresh };
}

/**
 * Opens a library title: its Detail page, or — to resume — the stream picker for the
 * video it stopped at (as Stremio's Continue Watching does), which needs the full meta
 * first. Falls back to the Detail page when that video can't be found. `pending` is
 * the id being opened; opening another, going anywhere else (root views stay mounted
 * under the screens pushed over them, so that is a change of the top frame, or the
 * search opening) or leaving cancels it.
 */
export function useOpenLibraryEntry(): {
  open: (entry: LibraryEntry, mode: "details" | "resume") => void;
  pending: string | null;
} {
  const { openMeta, openPicker, top } = useView();
  // the search overlay is no frame: opening it is going somewhere else too
  const { open: searching } = useSearch();
  const [pending, setPending] = useState<string | null>(null);
  const request = useRef(0);
  useEffect(() => {
    request.current++;
    setPending(null);
    return () => {
      request.current++;
    };
  }, [top, searching]);

  const open = useCallback(
    (entry: LibraryEntry, mode: "details" | "resume") => {
      const mine = ++request.current;
      const preview = previewOf(entry);
      if (mode === "details") {
        setPending(null);
        openMeta(preview);
        return;
      }
      setPending(entry.id);
      metaGet(entry.type, entry.id)
        .then(({ meta }) => {
          if (mine !== request.current) return;
          const live = !meta || meta.behaviorHints.isLive || meta.type === "tv";
          if (meta && !live && meta.videos.length > 0) {
            const video = meta.videos.find((v) => v.id === entry.videoId);
            if (video) return openPicker(meta, video);
          } else if (meta && !live && meta.type !== "series") {
            // a movie: the picker asks for its default video, which is what was played
            return openPicker(meta, null);
          }
          openMeta(preview);
        })
        .catch(() => mine === request.current && openMeta(preview))
        .finally(() => mine === request.current && setPending(null));
    },
    [openMeta, openPicker],
  );

  return { open, pending };
}

// --- helpers -----------------------------------------------------------------------------

/** The catalog-style preview of a library title, for the Detail frame. */
export function previewOf(entry: LibraryEntry): MetaPreview {
  return {
    id: entry.id,
    type: entry.type,
    name: entry.name,
    poster: entry.poster,
    posterShape: entry.posterShape,
    genres: [],
  };
}

/** How far into its resume point an entry is (0..1), or null with no position. */
export function resumeFraction(entry: LibraryEntry): number | null {
  const resume = entry.resume;
  if (!resume || resume.duration <= 0) return null;
  return Math.min(1, Math.max(0, resume.timeOffset / resume.duration));
}

/** A position in ms as a clock: "12:34", or "1:02:03" from an hour on. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** "23 min left" for an entry's resume point, when its duration is known. */
export function timeLeft(entry: LibraryEntry): string | null {
  const resume = entry.resume;
  if (!resume || resume.duration <= resume.timeOffset) return null;
  const minutes = Math.ceil((resume.duration - resume.timeOffset) / 60_000);
  return minutes >= 60
    ? `${Math.floor(minutes / 60)} h ${minutes % 60} min left`
    : `${minutes} min left`;
}

/** A Continue Watching card's line: "S1 E2 · 23 min left", or "Up next" once it moved on. */
export function continueLine(entry: LibraryEntry): string {
  return [episodeLabel(entry), timeLeft(entry) ?? (entry.resume ? null : "Up next")]
    .filter(Boolean)
    .join(" · ");
}

/**
 * "S1 E2" for an entry whose video id is the usual `<id>:<season>:<episode>` (or
 * `<id>:<episode>`, as anime addons use); null for anything else.
 */
export function episodeLabel(entry: LibraryEntry): string | null {
  const videoId = entry.videoId;
  if (!videoId || !videoId.startsWith(`${entry.id}:`)) return null;
  const parts = videoId.slice(entry.id.length + 1).split(":");
  if (!parts.every((part) => /^\d+$/.test(part))) return null;
  if (parts.length === 2) return `S${Number(parts[0])} E${Number(parts[1])}`;
  if (parts.length === 1) return `E${Number(parts[0])}`;
  return null;
}

/** "Today", "Yesterday", "3 days ago", or the date. */
export function formatDay(iso: string | null): string | null {
  if (!iso) return null;
  const date = new Date(iso);
  if (!Number.isFinite(date.getTime())) return null;
  const startOf = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOf(new Date()) - startOf(date)) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  return date.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}
