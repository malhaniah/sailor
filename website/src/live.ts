// The release in the browser: the one the page was built with, then the live one from
// the GitHub API, so a page built before a release still offers the newest download.

import { LATEST_API, type Release, normalizeRelease, toApiShape } from "./release.ts";

const CACHE_KEY = "zeiler:latest-release";
// The anonymous API allows 60 requests an hour per address: ask once per tab session
// and per quarter-hour at most
const CACHE_MS = 15 * 60 * 1000;

export function builtRelease(): Release | null {
  const el = document.getElementById("release-data");
  try {
    return el?.textContent ? normalizeRelease(JSON.parse(el.textContent)) : null;
  } catch {
    return null;
  }
}

function cached(): Release | null {
  try {
    const raw = sessionStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const { at, release } = JSON.parse(raw) as { at: number; release: unknown };
    return Date.now() - at < CACHE_MS ? normalizeRelease(release) : null;
  } catch {
    return null;
  }
}

/** The live release, or null when GitHub can't be reached (the page keeps its own). */
export async function liveRelease(): Promise<Release | null> {
  const hit = cached();
  if (hit) return hit;
  try {
    const res = await fetch(LATEST_API, {
      headers: { accept: "application/vnd.github+json" },
      signal: AbortSignal.timeout(6000),
    });
    if (!res.ok) return null;
    const release = normalizeRelease(await res.json());
    try {
      // in the API's shape, so normalizeRelease checks it again on the way out
      sessionStorage.setItem(CACHE_KEY, JSON.stringify({ at: Date.now(), release: toApiShape(release) }));
    } catch {
      // storage full or blocked: just don't cache
    }
    return release;
  } catch {
    return null;
  }
}

/** Whether the live release differs from what the page shows. */
export function isNewer(live: Release, shown: Release): boolean {
  return (
    live.tag !== shown.tag ||
    live.assets.length !== shown.assets.length ||
    live.assets.some((a, i) => a.url !== shown.assets[i]?.url || a.digest !== shown.assets[i]?.digest)
  );
}
