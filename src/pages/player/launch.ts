/**
 * Which launch the Player screen plays: the one-shot handoff named by `#/player?h=<id>`
 * (src/lib/playback-handoff.ts). Dev builds also accept the legacy query params used by
 * the VITE_ZEILER_AUTOPLAY hook (src/main.tsx):
 *   url=<http(s) stream>            direct stream
 *   infoHash=<40 hex>&sources=<json array>&fileIdx=<n>&filename=<name>
 *     &fileMustInclude=<json array> torrent — resolved by the player via engine_resolve
 * plus optional title, headers (JSON object), ua, referrer, startAt (seconds).
 * Release builds ignore them, so a URL can never be smuggled in through the hash.
 */
import { type PlayerLaunch, takeLaunch } from "../../lib/playback-handoff";

// The launch taken for the route currently on screen. React may call a render-phase
// initializer more than once for one mount (a discarded render, StrictMode in dev), so
// the one-shot take is memoized per route until the page unmounts.
let current: { hash: string; launch: PlayerLaunch | null } | null = null;

/** The launch for this player route; repeated calls for the same route agree. */
export function launchForRoute(hash: string): PlayerLaunch | null {
  if (current?.hash === hash) return current.launch;
  const launch = readLaunch(hash);
  current = { hash, launch };
  return launch;
}

/** Forgets the route's launch once its page is gone, so a later visit to the same history
 * entry finds nothing (the handoff id is one-shot). */
export function releaseRoute(hash: string): void {
  if (current?.hash === hash) current = null;
}

/** The one-shot launch id in a player route (`#/player?h=<id>`); null for legacy routes. */
export function launchIdForRoute(hash: string): string | null {
  return new URLSearchParams(hash.split("?")[1] ?? "").get("h");
}

function readLaunch(hash: string): PlayerLaunch | null {
  const query = new URLSearchParams(hash.split("?")[1] ?? "");
  const id = query.get("h");
  if (id) return takeLaunch(id);
  if (import.meta.env.DEV) return legacyLaunch(query);
  return null;
}

function legacyLaunch(p: URLSearchParams): PlayerLaunch | null {
  let headers: [string, string][] = [];
  try {
    const parsed: unknown = JSON.parse(p.get("headers") ?? "{}");
    if (parsed && typeof parsed === "object") {
      headers = Object.entries(parsed as Record<string, string>).map(([k, v]) => [k, String(v)]);
    }
  } catch {
    /* addon data is messy; ignore bad header blobs */
  }
  const stringList = (name: string) => {
    try {
      const parsed: unknown = JSON.parse(p.get(name) ?? "[]");
      return Array.isArray(parsed) ? parsed.map(String) : [];
    } catch {
      return [];
    }
  };
  const num = (name: string) => {
    const v = p.get(name);
    const n = v === null ? NaN : Number(v);
    return Number.isFinite(n) ? n : null;
  };

  const url = p.get("url");
  const infoHash = p.get("infoHash");
  const filename = p.get("filename");
  const common = {
    title: p.get("title") ?? filename,
    startAtSec: num("startAt"),
    meta: null,
    video: null,
    stream: null,
    transportUrl: null,
    addonName: null,
    live: false,
    backend: "auto" as const,
  };
  if (url) {
    return {
      ...common,
      source: {
        kind: "url",
        url,
        headers,
        userAgent: p.get("ua"),
        referrer: p.get("referrer"),
      },
    };
  }
  if (infoHash) {
    return {
      ...common,
      source: {
        kind: "torrent",
        infoHash,
        fileIdx: num("fileIdx"),
        sources: stringList("sources"),
        fileMustInclude: stringList("fileMustInclude"),
        filename,
      },
    };
  }
  return null;
}
