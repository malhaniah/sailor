# Zeiler architecture

Zeiler is a free, open-source (GPL-3.0-or-later) desktop client for the Stremio ecosystem.
It hosts no media, indexes no media, and ships no content addon — users install their own
addon URLs. This document records the load-bearing decisions and the shape of the codebase.

## Decisions

| Decision | Choice | Why |
| --- | --- | --- |
| License | GPL-3.0-or-later | Copyleft for a FOSS media client; compatible with every dependency we use (MIT, Apache-2.0, LGPL-2.1, MPL-2.0). |
| Shell | Tauri 2.12.x (stable; not the 3.0 alphas) | Proven by Harbor; Rust core + webview UI is the fastest path to a polished media UI. |
| UI | Vite 8 + React 19 + TypeScript ~6.0 + Tailwind 4 | Current stable stack; matches the create-tauri-app template. |
| Protocol client | Hand-rolled `zeiler-addon` crate | `stremio-core` is git-only (frozen 0.1.0), framework-shaped (Elm runtime + static `Env`), hardwired to api.strem.io (incl. unauthenticated SeekLog telemetry), and assumes Stremio's `server.js` for torrents. The protocol surface is small; we mirror stremio-core's lenient serde semantics (MIT, referenced with attribution). |
| Torrent engine | librqbit, in-process | Pure Rust, Apache-2.0, first-class streaming (sequential priority + HTTP Range endpoint). Pinned to a git rev of `main` until 9.0.2 ships (crates.io 9.0.1 lacks three Windows fixes: NTFS sparse marking #681, DHT death on UDP 10054 #690, stream-during-init #689). |
| Playback | libmpv (`libmpv2` 6.x crate) | v0 falls back to spawning a system `mpv`; the embedded surface (video under a transparent webview) is the `zeiler-player` roadmap: Windows `wid`+HWND_BOTTOM, macOS render-API NSOpenGLView, Linux GtkGLArea overlay. libmpv is a link-time dependency, so the embedded backend is behind the `embedded` cargo feature until `libmpv-2.dll` + `mpv.lib` are vendored via a hash-pinned fetch script. |
| Content neutrality | No seeded stream addons; one built-in metadata addon | Unlike Harbor (which also seeds Torrentio), Zeiler builds in only Cinemeta — metadata only, from a manifest snapshot compiled into the app (`src-tauri/builtin/`), switchable in Settings → Content. Stream addons are installed by user-pasted URL only, behind a confirmation dialog showing the manifest (name, version, resources, p2p/adult hints). |
| Updates | tauri-plugin-updater, install on next launch | Signed (minisign) packages from `releases/latest/download/latest.json`; releases are therefore regular, not pre-releases. Checked at launch (installed immediately, before the user starts anything) and every 6 h (downloaded to the cache, installed on the next launch). Signing key: `TAURI_SIGNING_PRIVATE_KEY` secret; release builds pass `--config src-tauri/tauri.release.conf.json` to emit updater artifacts. Dev builds never self-update. |

## Workspace layout

```
zeiler/
  Cargo.toml              # virtual workspace: ["src-tauri", "crates/*"], resolver 3
  package.json  vite.config.ts  index.html  src/   # React UI at repo root
  crates/
    zeiler-addon/         # Stremio addon protocol: types, URL builder, HTTP client, fan-out
    zeiler-engine/        # librqbit session + local stream endpoint for the player
    zeiler-player/        # player control; external-mpv backend now, embedded libmpv later
  src-tauri/              # Tauri shell: commands, state, config, capabilities
  docs/  scripts/  .github/workflows/
```

Domain crates never depend on `tauri`; they test with plain `cargo test`.

## The addon protocol (what `zeiler-addon` implements)

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
| `infoHash` + `fileIdx` + `sources` | `zeiler-engine`: magnet built from infoHash + `tr=` params (strip `tracker:` prefixes — librqbit ignores `AddTorrentOptions.trackers` for magnets), `only_files=[fileIdx]` (largest non-padding file via a `list_only` pre-add when fileIdx is absent), then a `127.0.0.1` Range-capable stream URL is handed to the player. |
| `ytId` | Deferred (yt-dlp integration later); shown as external link. |
| `externalUrl` | Opened in the system browser after scheme allow-listing (http/https only). |
| rar/zip/nzb/playerFrame | Unsupported — hidden or marked as such. |

Engine policy: per-infohash cache folder under the app cache dir, `overwrite: true`,
delete-on-stop by default (bounded disk usage), DHT + persistence under the app data dir
(never rqbit's shared default), loopback-only HTTP with auth, upload cap exposed in settings.

## Tauri IPC surface

Commands (all async, `Result<T, IpcError>`; payloads camelCase). This list matches the
`generate_handler!` in `src-tauri/src/lib.rs` exactly — update both together.

- Addons: `addon_preview { url }` → Descriptor (fetch + parse, not persisted — feeds the
  confirm dialog) · `addon_add { url }` → Descriptor · `addon_remove { transportUrl }` ·
  `addon_list()` → Descriptor[] (built-ins first, then the user's addons)
- Content: `catalog_load { args: { transportUrl, type, id, extra } }` → `{ metas }` ·
  `meta_get { type, id }` → `{ meta, asked, errors }` (first successful Meta across
  supporting addons) · `streams_get { type, id }` → per-addon stream groups (parallel
  fan-out)
- Engine: `engine_resolve { request: { infoHash, fileIdx?, sources, filename?,
  fileMustInclude } }` → `{ streamUrl, infoHash, fileIdx, fileName, fileSize }` ·
  `engine_stats { infoHash }` → librqbit stats JSON · `engine_remove { infoHash,
  deleteFiles }`
- External player: `player_status()` → `{ available, path? }` · `player_play { request:
  { url, title?, headers, userAgent?, referrer?, startAtSec? } }` · `player_stop()`
- Settings: `settings_get()` → Settings · `settings_patch { patch }` → Settings (merges
  the given top-level keys server-side; unknown keys and wrongly typed values are
  rejected) · `settings_set { settings }` → Settings (**deprecated**, replaces every
  setting; kept for one release)
- Updates: `update_status()` → `{ current, ready? }` · `update_check()` → new version or
  null · `update_install_now()`
- Embedded player (`embedded` feature; stubs otherwise): `player_embedded_available()` →
  bool · `player_embedded_start { request }` · `player_embedded_stop()` ·
  `player_embedded_set_property { name, value }` · `player_embedded_command { args }` ·
  `player_set_geometry { geometry: { x, y, width, height } }`

Errors serialize as `{ kind, message }`; `message` is a full sentence shown as-is. Kinds:
`invalidInput`, `addon`, `notFound`, `engine`, `player`, `storage`, plus kinds reserved
for later milestones — `network` (service unreachable or timed out; retryable),
`auth` (credentials rejected), `sessionExpired` (sign in again), `locked` (wrong PIN,
or addon sync locked after a failed pull), `conflict` (clashes with existing state,
e.g. the same addon under another URL) and `unsupported` (not on this platform/build).
The TS mirror is `IpcErrorKind` in `src/lib/types.ts`.

Events (named `<domain>://<event>`, rare notifications only):

- `settings://changed` — the full new Settings, after every successful
  `settings_patch`/`settings_set`. `useSettings()` subscribes, so every part of the UI
  sees the latest values.
- `update://ready` / `update://installing` (version) · `update://failed` (message)
- `player://event` — embedded player events (`PlayerEvent`, kebab-case tagged)

High-frequency data (engine stats, player position) will move to `tauri::ipc::Channel`
subscriptions when the embedded player lands.

## Persistence

All durable state is Rust-owned JSON under the app data dir; there is no localStorage.
`src-tauri/src/persist.rs` holds the rules: a missing file reads as defaults; an
unparseable one is moved to `*.json.corrupt.bak` and replaced by defaults; any other IO
error fails startup; writes are atomic (`*.json.tmp` + rename). `AppState` keeps
`settings.json` and `addons.json`. Every later feature owns a `JsonStore<T>` (load / get /
update-and-persist under its own lock), registered with `app.manage` — it does not grow
`AppState`. New files carry a `{"version": N, ...}` envelope. Per-user files take their
path from `AppState::profile_dir()` (the data dir until profiles exist, then the active
profile's folder); re-creatable caches go under the cache dir.

Built-in addons are a table in `src-tauri/src/builtin.rs` (transport URL, compiled-in
manifest snapshot from `src-tauri/builtin/`, an enabled-by-settings predicate). A test
asserts that no built-in declares a `stream` resource.

### Settings schema

`settings.json` is additive only: a new key always has a default, and an existing key
never changes name or type. Reading is lenient per top-level key — a wrongly typed value
resets just that setting to its default (with a warning) instead of moving the whole file
aside — while `settings_patch` is strict. Nested groups must be lenient per field inside
(serde_with `DefaultOnError` where the type's `Default` is the intended default),
otherwise one bad sub-field resets the whole group. Settings are mirrored wholesale to
the webview, so secrets (Stremio authKey, IPTV credentials) never go here.

Reserved key map — names are fixed now; each key is added only when its feature ships:

| Scope | Keys (existing **bold**) |
| --- | --- |
| Per device | **`mpvPath`**, **`engineListenPort`**, **`engineUpnp`**, **`uploadLimitKibps`**, **`downloadLimitKibps`**, **`deleteOnStop`**, **`autoUpdate`**, `playerBackend` (enum), `marketplaceEnabled`, `marketplaceSources`, `stremioDeepLink` |
| Per profile | **`theme`**, **`instantPlay`**, **`hideAnime`**, **`hideLiveTv`**, `showAdult` (the single adult flag and age gate, also used by the marketplace), `hideP2pAddons`, `homeRows`, `subtitles{}` (languages, auto-load, style), `audio{}` (languages, normalize, volume max), `skip{}` (intro/outro auto-skip), `bingeWatching`, `upNextSeconds` |
| Built-in toggles (scope is an owner decision; device until then) | **`builtinCinemeta`**, `builtinOpensubtitles`, `builtinKitsu` |

Per-profile keys live in `settings.json` until local profiles exist, then move into the
profile store. There is no `profile{}` group in Settings.

## Navigation names

The frontend navigates with a frame stack (`src/lib/view.tsx`). Every switch over
`RootView` or `Frame` kinds ends in `unreachable(x)`, so a new kind that is not handled
everywhere fails `tsc`. Reserved now, unreachable from the UI until their milestones:
root view `live`; frames `browse` (catalog browser) and `addon-detail` (marketplace).
Reserved Settings section ids: `account`, `subtitles`, `playback`.

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
- Deep links (`zeiler://`; `stremio://` opt-in later) are untrusted argv input and are
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
