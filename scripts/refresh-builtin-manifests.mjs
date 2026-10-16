#!/usr/bin/env node
// Refreshes the built-in addon manifest snapshots in src-tauri/builtin/ from the live
// addons. Needs network; writes only into src-tauri/builtin/.
//
//   node scripts/refresh-builtin-manifests.mjs           # rewrite the snapshots that changed
//   node scripts/refresh-builtin-manifests.mjs --check   # only report; exit 1 if any is stale
//
// A snapshot is kept byte for byte as the addon served it, and is accepted only if:
// - it is a JSON manifest object whose id matches the snapshot it replaces (the app
//   matches the user's own install of a built-in by that id, so a changed id needs a
//   human look, not a refresh);
// - it declares only metadata or subtitle resources (catalog, meta, addon_catalog,
//   subtitles), and so no `stream` resource: Zeiler never builds in a stream addon.
// Review the diff, then run `cargo test -p zeiler builtin` before committing.
//
// BUILTINS mirrors the table in src-tauri/src/builtin.rs; a test there checks that every
// transport URL and snapshot file appears here.

import { readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const BUILTINS = [
  { url: "https://v3-cinemeta.strem.io/manifest.json", file: "cinemeta-manifest.json" },
  { url: "https://anime-kitsu.strem.fun/manifest.json", file: "kitsu-manifest.json" },
  {
    url: "https://opensubtitles-v3.strem.io/manifest.json",
    file: "opensubtitles-v3-manifest.json",
  },
];

/** Resources a built-in may declare: metadata or subtitles, never streams. */
const ALLOWED_RESOURCES = new Set(["catalog", "meta", "addon_catalog", "subtitles"]);
const MAX_BYTES = 1024 * 1024;
const TIMEOUT_MS = 30_000;

const dir = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "builtin");
const checkOnly = process.argv.includes("--check");

/** The manifest's resource names; the object form `{ name, types, idPrefixes }` counts too. */
function resourceNames(manifest) {
  if (!Array.isArray(manifest.resources)) return [];
  return manifest.resources
    .map((r) => (typeof r === "string" ? r : r && typeof r.name === "string" ? r.name : null))
    .filter((name) => name !== null);
}

/** The reason a fetched manifest can't replace the snapshot, or null. */
function rejection(manifest, previous) {
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest)) {
    return "not a JSON object";
  }
  if (typeof manifest.id !== "string" || manifest.id === "") return "no manifest id";
  if (typeof manifest.name !== "string") return "no manifest name";
  if (previous && manifest.id !== previous.id) {
    return `the manifest id changed from ${previous.id} to ${manifest.id}`;
  }
  const names = resourceNames(manifest);
  if (names.length === 0) return "it declares no resources";
  const disallowed = names.filter((name) => !ALLOWED_RESOURCES.has(name));
  if (disallowed.length > 0) {
    return `it declares ${disallowed.join(", ")}; built-ins serve metadata or subtitles only`;
  }
  return null;
}

async function readSnapshot(path) {
  try {
    const text = await readFile(path, "utf8");
    return { text, manifest: JSON.parse(text) };
  } catch (e) {
    if (e.code === "ENOENT") return null;
    throw new Error(`${path}: ${e.message}`);
  }
}

async function fetchText(url) {
  const response = await fetch(url, {
    headers: { "user-agent": "Zeiler (built-in snapshot refresh)" },
    signal: AbortSignal.timeout(TIMEOUT_MS),
    // the app keeps asking the URL in the table, so a moved addon needs a human look
    redirect: "error",
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const text = await response.text();
  if (Buffer.byteLength(text) > MAX_BYTES) throw new Error(`larger than ${MAX_BYTES} bytes`);
  return text;
}

async function refresh({ url, file }) {
  const path = join(dir, file);
  const previous = await readSnapshot(path);
  const text = await fetchText(url);
  let manifest;
  try {
    manifest = JSON.parse(text);
  } catch (e) {
    throw new Error(`not JSON: ${e.message}`);
  }
  const reason = rejection(manifest, previous?.manifest);
  if (reason) throw new Error(`rejected: ${reason}`);

  const label = `${manifest.id} v${manifest.version ?? "?"}`;
  if (previous?.text === text) return { stale: false, message: `${file}: unchanged (${label})` };
  const change = previous
    ? `v${previous.manifest.version ?? "?"} -> v${manifest.version ?? "?"}`
    : "new snapshot";
  if (!checkOnly) await writeFile(path, text);
  return {
    stale: true,
    message: `${file}: ${checkOnly ? "stale" : "updated"} (${label}, ${change})`,
  };
}

let failed = false;
let stale = false;
for (const builtin of BUILTINS) {
  try {
    const result = await refresh(builtin);
    stale ||= result.stale;
    console.log(result.message);
  } catch (e) {
    failed = true;
    console.error(`${builtin.file} (${builtin.url}): ${e.message}`);
  }
}
if (failed || (checkOnly && stale)) process.exit(1);
