# Sailor architecture

Sailor is a free, open-source (GPL-3.0-or-later) desktop client for the Stremio ecosystem.
It hosts no media, indexes no media, and ships no content addon — users install their own
addon URLs. This document records the load-bearing decisions and the shape of the codebase.

## Decisions

| Decision | Choice | Why |
| --- | --- | --- |
| License | GPL-3.0-or-later | Copyleft for a FOSS media client; compatible with every dependency we use (MIT, Apache-2.0, LGPL-2.1, MPL-2.0). |
| Shell | Tauri 2.12.x (stable; not the 3.0 alphas) | Proven by Harbor; Rust core + webview UI is the fastest path to a polished media UI. |
| UI | Vite 8 + React 19 + TypeScript ~6.0 + Tailwind 4 | Current stable stack; matches the create-tauri-app template. |
| Protocol client | Hand-rolled `sailor-addon` crate | `stremio-core` is git-only (frozen 0.1.0), framework-shaped (Elm runtime + static `Env`), hardwired to api.strem.io (incl. unauthenticated SeekLog telemetry), and assumes Stremio's `server.js` for torrents. The protocol surface is small; we mirror stremio-core's lenient serde semantics (MIT, referenced with attribution). |
| Torrent engine | librqbit, in-process | Pure Rust, Apache-2.0, first-class streaming (sequential priority + HTTP Range endpoint). Pinned to a git rev of `main` until 9.0.2 ships (crates.io 9.0.1 lacks three Windows fixes: NTFS sparse marking #681, DHT death on UDP 10054 #690, stream-during-init #689). |
| Playback | libmpv (`libmpv2` 6.x crate) | v0 falls back to spawning a system `mpv`; the embedded surface (video under a transparent webview) is the `sailor-player` roadmap: Windows `wid`+HWND_BOTTOM, macOS render-API NSOpenGLView, Linux GtkGLArea overlay. libmpv is a link-time dependency, so the embedded backend is behind the `embedded` cargo feature until `libmpv-2.dll` + `mpv.lib` are vendored via a hash-pinned fetch script. |
| Content neutrality | No seeded stream addons; one built-in metadata addon | Unlike Harbor (which also seeds Torrentio), Sailor builds in only Cinemeta — metadata only, from a manifest snapshot compiled into the app (`src-tauri/builtin/`), switchable in Settings → Content. Stream addons are installed by user-pasted URL only, behind a confirmation dialog showing the manifest (name, version, resources, p2p/adult hints). |
| Updates | tauri-plugin-updater, install on next launch | Signed (minisign) packages from `releases/latest/download/latest.json`; releases are therefore regular, not pre-releases. Checked at launch (installed immediately, before the user starts anything) and every 6 h (downloaded to the cache, installed on the next launch). Signing key: `TAURI_SIGNING_PRIVATE_KEY` secret; release builds pass `--config src-tauri/tauri.release.conf.json` to emit updater artifacts. Dev builds never self-update. |

## Workspace layout

```
sailor/
  Cargo.toml              # virtual workspace: ["src-tauri", "crates/*"], resolver 3
  package.json  vite.config.ts  index.html  src/   # React UI at repo root
  crates/
    sailor-addon/         # Stremio addon protocol: types, URL builder, HTTP client, fan-out
    sailor-engine/        # librqbit session + local stream endpoint for the player
    sailor-player/        # player control; external-mpv backend now, embedded libmpv later
  src-tauri/              # Tauri shell: commands, state, config, capabilities
  docs/  scripts/  .github/workflows/
```

Domain crates never depend on `tauri`; they test with plain `cargo test`.

## The addon protocol (what `sailor-addon` implements)

- An addon is a **transport URL** ending in `/manifest.json`. Resources are fetched by
  replacing that suffix with `/{resource}/{type}/{id}[/{extra}].json` (string replace —
  preserves config path prefixes and query strings, exactly like stremio-core).
- Path segments are percent-encoded with the `encodeURIComponent` set
  (alphanumerics plus `- _ . ! ~ * ' ( )` stay literal; `:` → `%3A`, space → `%20`).
  `extra` is `name=value&name=value`, values individually encoded.
- Resources: `catalog`, `meta`, `stream`, `subtitles`, `addon_catalog`.
- Routing: `catalog` is matched against `manifest.catalogs` (type+id) only; other
  resources match `manifest.resources` entries — short string form inherits manifest-level
  `types`/`idPrefixes`, object form uses its own; missing `types` ⇒ unsupported;
  absent or empty `idPrefixes` ⇒ match all (stremio-core behavior); otherwise `starts_with`.
- Series episodes: video id = `{metaId}:{season}:{episode}`; streams are requested with the
  **video id** and the meta **type** (`/stream/series/tt0108778%3A1%3A1.json`).
- Parsing is deliberately lenient (real addons are messy): unknown fields ignored, nulls →
  defaults, `title`/`name` aliases, numbers-or-strings accepted, invalid list items skipped.
- The client follows redirects (Cinemeta answers 307 for paginated catalogs), caps response
  sizes, applies timeouts, and never runs in the webview — **all addon HTTP happens in Rust**,
  so CORS/mixed-content never applies and the CSP stays strict.

## Stream resolution flow

For a chosen video id, `streams_get` fans out to all supporting addons in parallel
(per-addon timeout, partial results attributed per addon). Each returned stream maps to:

| Stream source | Handling |
| --- | --- |
| `url` (http/https) | Played directly; `behaviorHints.proxyHeaders.request` → mpv `http-header-fields`. |
| `infoHash` + `fileIdx` + `sources` | `sailor-engine`: magnet built from infoHash + `tr=` params (strip `tracker:` prefixes — librqbit ignores `AddTorrentOptions.trackers` for magnets), `only_files=[fileIdx]` (largest non-padding file via a `list_only` pre-add when fileIdx is absent), then a `127.0.0.1` Range-capable stream URL is handed to the player. |
| `ytId` | Deferred (yt-dlp integration later); shown as external link. |
| `externalUrl` | Opened in the system browser after scheme allow-listing (http/https only). |
| rar/zip/nzb/playerFrame | Unsupported — hidden or marked as such. |

Engine policy: per-infohash cache folder under the app cache dir, `overwrite: true`,
delete-on-stop by default (bounded disk usage), DHT + persistence under the app data dir
(never rqbit's shared default), loopback-only HTTP with auth, upload cap exposed in settings.

## Tauri IPC surface (v0)

Commands (all async, `Result<T, IpcError>`; payloads camelCase):

- `addon_preview(url)` → Descriptor (fetch + parse, not persisted — feeds the confirm dialog)
- `addon_add(url)` / `addon_remove(transportUrl)` / `addon_list()`
- `catalog_load { transportUrl, type, id, extra }` → `{ metas }`
- `meta_get { type, id }` → first successful Meta across supporting addons
- `streams_get { type, id }` → per-addon stream groups (parallel fan-out)
- `engine_resolve { infoHash, fileIdx?, sources, filename? }` → `{ streamUrl, torrentId, … }`
- `engine_stats { torrentId }` / `engine_remove { torrentId, deleteFiles }`
- `player_status()` / `player_play { url, title?, headers? }` (external mpv backend in v0)
- `settings_get()` / `settings_set(settings)`

Events are reserved for rare notifications; high-frequency data (engine stats, player
position) will move to `tauri::ipc::Channel` subscriptions when the embedded player lands.

## Security posture

- Strict CSP (no remote script/style; `img-src https: http:` because addon posters are
  arbitrary hosts; loopback wildcards for the engine stream only).
- Least-privilege capabilities; no fs/shell/http plugin — the webview can only invoke our
  typed commands.
- Addons are untrusted JSON: stream/external URLs are scheme-allow-listed before reaching
  mpv or the OS (mpv would happily open `file://`); `proxyHeaders` are treated as
  attacker-controlled; infoHashes validated as 40-char hex.
- Addon transport URLs may embed credentials (config-in-URL pattern) — they are secrets;
  never logged, never sent anywhere but the addon host.
- Engine HTTP binds 127.0.0.1 with per-session basic-auth credentials.
- Deep links (`sailor://`; `stremio://` opt-in later) are untrusted argv input and are
  parsed/validated before any navigation or install prompt.

## Packaging & licensing notes

- `license = "GPL-3.0-or-later"` on every crate; LICENSE at root; THIRD-PARTY notices for
  Apache-2.0 (librqbit) and any MIT-attributed code adapted from stremio-core/Harbor.
- libmpv is **not** statically linked. Windows ships `libmpv-2.dll` (hash-pinned fetch,
  LGPL build preferred) beside the exe via `bundle.resources`; Linux declares the distro
  `libmpv2` package; macOS bundles a prebuilt dylib under Frameworks. If a GPL libmpv/FFmpeg
  build is ever shipped, the corresponding source offer goes in the release notes.
- Windows installer: NSIS per-user only (no MSI/WiX). Unsigned builds will trip
  SmartScreen/Smart App Control — SignPath Foundation (free for GPL) is the plan.

## Milestones

1. **v0.1 (current)** — workspace + addon client + catalogs/meta/streams UI + torrent engine
   + external-mpv playback; CI (fmt, clippy, test, build).
2. **v0.2** — embedded libmpv (Windows `wid` first), subtitles (addon + local file),
   library/continue-watching persistence, updater + release pipeline.
3. **v0.3** — macOS/Linux embedded playback, stream ranking (Harbor-core-inspired
   parse→trust→score→rank), deep links, binge/next-episode (bingeGroup).
4. **Later** — casting, Live TV/EPG, PiP, theme system, optional Stremio account sync
   (isolated, off by default).
