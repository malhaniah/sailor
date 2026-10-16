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
| Playback | mpv, two backends behind one session | Embedded libmpv (`libmpv2` 6.x crate; video under the transparent webview — Windows `wid`+HWND_BOTTOM, the default on Windows releases; macOS render-API NSOpenGLView next) or the user's own `mpv` in its own window, remote-controlled over mpv's JSON IPC (Linux, macOS for now, and anywhere embedded is unavailable). libmpv is a link-time dependency, so the embedded backend is the `embedded` cargo feature: Windows release builds turn it on and ship a hash-pinned LGPL `libmpv-2.dll` (`scripts/fetch-libmpv.mjs`); plain dev builds leave it off. See "Player backends". |
| Content neutrality | No seeded stream addons; one built-in metadata addon | Unlike Harbor (which also seeds Torrentio), Zeiler builds in only Cinemeta — metadata only, from a manifest snapshot compiled into the app (`src-tauri/builtin/`), switchable in Settings → Content. Stream addons are installed by user-pasted URL only, behind a confirmation dialog showing the manifest (name, version, resources, p2p/adult hints). |
| Updates | tauri-plugin-updater, install on next launch | Signed (minisign) packages from `releases/latest/download/latest.json`; releases are therefore regular, not pre-releases. Checked at launch (installed immediately, before the user starts anything) and every 6 h (downloaded to the cache, installed on the next launch). Signing key: `TAURI_SIGNING_PRIVATE_KEY` secret; release builds pass `--config src-tauri/tauri.release.conf.json` to emit updater artifacts. Dev builds never self-update. |

## Workspace layout

```
zeiler/
  Cargo.toml              # virtual workspace: ["src-tauri", "crates/*"], resolver 3
  package.json  vite.config.ts  index.html  src/   # React UI at repo root
  crates/
    zeiler-account/       # library items, progress rules, watched bitfield (stremio-core compatible)
    zeiler-addon/         # Stremio addon protocol: types, URL builder, HTTP client, fan-out
    zeiler-engine/        # librqbit session + local stream endpoint for the player
    zeiler-player/        # player session: external mpv over JSON IPC; embedded libmpv (feature)
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

## Playback entry point

Zeiler's own Player screen (`#/player`) is the single place playback starts, on every
OS. The stream picker never resolves or launches anything itself for `url` and
`infoHash` streams; `ytId` and `externalUrl` streams still leave the app. Its secondary
action, "Open in external mpv", opens the same Player screen with `backend: "external"`
on the launch, so mpv gets its own window and Zeiler's remote controls whatever the
backend setting says.

- **Handoff** (`src/lib/playback-handoff.ts`). The picker builds a typed `PlayerLaunch`
  — the source (a direct URL with its headers, or a torrent: infoHash, fileIdx, sources,
  fileMustInclude, filename), meta (type, id, name, poster, videos), video (id, season,
  episode, title, thumbnail), stream (name, behaviorHints incl. bingeGroup, filename,
  videoHash, videoSize; subtitles), transportUrl and addonName, a `live` flag and the
  `backend` to use (`auto`, or `external` from "Open in external mpv") — and calls
  `openPlayer(launch)`. That stashes the launch in memory under a random one-shot
  id and navigates to `#/player?h=<id>`; the Player `takeLaunch(id)`s it exactly once.
  Stream URLs, headers and transport URLs (which may embed credentials) therefore never
  enter `location.hash` or the history stack. A reload or a stale history entry finds no
  launch and the Player shows "nothing to play" with a Back action; because the
  Rust-side player outlives a webview reload, that screen also stops a built-in video
  left playing (`player_stop { only: "embedded" }`), which would otherwise sit hidden
  behind it. An external mpv the user left playing with Back is left alone (also when
  Forward lands on a spent player entry). Only
  dev builds still accept the legacy `url=`/`infoHash=` params of the
  `VITE_ZEILER_AUTOPLAY` hook. `openPlayer` returns the launch id, and the Player reports
  a launch that ends in an error (`reportLaunchFailed`), so the picker underneath marks
  that stream "Unavailable, try another." as it did when it played streams itself.
- **App stays mounted** (`src/main.tsx`). The Player renders next to the app shell, which
  stays mounted but hidden (`display:none` + `inert`), so the view stack, the picker's
  stream list and its scroll offsets survive playback and Back (`history.back`) returns
  exactly where the user was. Window-level key listeners of the hidden shell (Escape pops
  frames in `src/lib/view.tsx`, `/` opens search in `src/chrome/Topbar.tsx`) check the
  single `playerActive()` signal from the handoff module and stay quiet while the player
  owns the keyboard.
- **Backends** (`src/pages/player/`). The page calls `player_start { request, backend }`
  with the launch's `backend` and renders by the backend Rust reports (see "Player
  backends"; `auto` follows the `playerBackend` setting, chosen in Settings → Player:
  Auto / Built-in / External mpv). Before the reply it shows the loading state of the
  backend it expects (from `player_backends` and the setting).
  - *Embedded* (Windows releases: `embedded` feature + libmpv present): mpv renders
    under the transparent webview, with the controls drawn as auto-hiding overlays
    (`PlayerChrome.tsx`); `f` makes the Zeiler window fullscreen, Escape leaves
    fullscreen first (and the player only when not fullscreen), and leaving the
    player screen any other way ends fullscreen too. After a fullscreen
    toggle and on every window resize, move and scale change the page re-sends the
    video rect and calls `player_surface_refresh` on a kick schedule (0, 60, 200,
    500, 1000, 2000 ms; `surface-sync.ts`).
  - *External mpv* (Linux and macOS, Windows dev builds without the feature, a
    fallback, or a forced launch): mpv plays in its own window and the page is its full remote
    control (`ExternalRemote.tsx`): the title's art fills the window, dimmed; title,
    episode and source at the top; the same `ControlsBar` as embedded docked at the
    bottom — play/pause, seek bar with elapsed/total time and the buffered range
    (`demuxer-cache-time`), volume and mute, audio and subtitle menus from
    `track-list` (subtitles with Off), speed, fullscreen of the mpv window
    (`cycle fullscreen`) and Stop. Shortcuts while Zeiler has focus: Space, ←/→ (5 s),
    ↑/↓ (volume 5), m, f, Escape (back). When the user closes mpv (`shutdown`) the page
    says "mpv was closed" with Back and Play again. When embedded fell back to mpv,
    a dismissible notice gives `fallbackReason`. If mpv's IPC never came up
    (`remoteControl: false`) the page shows the passive "Playing in mpv" card instead.
  - **Back keeps external mpv playing** (as in M1): Back/Escape leave the page while
    mpv keeps playing in its window, and its torrent keeps streaming; the page says so
    next to the controls. Stop (`player_stop`) closes mpv and removes the torrent.
    Back while mpv is still opening the stream cancels it (stops mpv, removes the
    torrent), and leaving after playback ended, failed or mpv was closed stops the
    player too; the Back tooltip and the footnote follow the phase
    (`backKeepsPlaying` in `usePlaybackSession.ts`). An explicit Built-in choice on a
    build or OS without it fails with a pointer to Settings → Player.
  Torrents are resolved by the Player (`engine_resolve`, honouring `fileMustInclude`)
  with live engine stats. The playback session owns the torrent: when the page goes
  away by any route (Back, Escape, native history navigation) the player is stopped
  and the torrent removed from the engine per `deleteOnStop`, and a resolve that
  completes after the page is gone removes the torrent it just added (librqbit holds a
  magnet only once its metadata arrives, so an earlier remove finds nothing). External
  mpv that is still playing is the exception above.
- **Player page layout.** `PlayerPage.tsx` (shell, renders by backend), `launch.ts`,
  `usePlaybackSession.ts` (start, resolve, event pump, cleanup), `usePlayerControls.ts`
  (actions and keyboard shortcuts shared by both screens), `playback-clock.ts`
  (time-pos, duration, pause, buffering, buffered range in a small subscribable store so
  a position tick re-renders only the seek bar and readouts), `mpv-state.ts`,
  `PlayerChrome.tsx` (embedded), `surface-sync.ts` (embedded surface after window
  changes), `ExternalRemote.tsx` (external), `ControlsBar.tsx` (both), `states.tsx`,
  `TorrentStats.tsx`. Later player features add files here.

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
- Player (backend-neutral; see "Player backends"): `player_backends()` →
  `{ embedded: { available, reason?, libmpvVersion? }, external: { available, path? } }` ·
  `player_start { request: { url, title?, headers, userAgent?, referrer?, startAtSec? },
  backend?: "auto" | "embedded" | "external", context?: { meta, videoId, videos } }` →
  `{ backend, fallbackReason?, remoteControl }` (with `context`, the playback's progress
  is saved to the library; see "Library and progress") · `player_load { request,
  context? }` · `player_stop { only? }` (any backend;
  `only: "embedded" | "external"` stops just a session on that backend) ·
  `player_set_property { name, value }` · `player_command { args }` ·
  `player_get_property { name }` → JSON value · `player_set_geometry { geometry: { x, y,
  width, height } }` (embedded video rect, physical px) · `player_surface_refresh()`
  (re-assert the embedded surface after a window change; no-op unless built-in video
  plays)
- Deprecated player aliases (one release; `commands/mpv/aliases.rs`; the UI no longer
  calls any of them): `player_status()` →
  `{ available, path? }` (= `player_backends().external`) ·
  `player_embedded_available()` → bool · `player_play { request }` (start external) ·
  `player_embedded_start { request }` (start embedded, no fallback) ·
  `player_embedded_stop()` (stops only an embedded session) ·
  `player_embedded_set_property { name, value }` · `player_embedded_command { args }`
- Library (see "Library and progress"): `library_list { query: { filter:
  "continueWatching" | "library" | "history", sort?, type?, limit? } }` → LibraryEntry[] ·
  `library_get { id, videos? }` → `{ entry, watchedVideoIds }` or null ·
  `library_set { request: { meta, inLibrary } }` → LibraryEntry or null ·
  `library_mark_watched { request: { meta, videos, videoIds, season?, watched } }` →
  `{ entry, watchedVideoIds }` or null (a season, some videos, or with neither the
  whole title) ·
  `library_clear_progress { id }` → LibraryEntry or null
- Settings: `settings_get()` → Settings · `settings_patch { patch }` → Settings (merges
  the given top-level keys server-side; unknown keys and wrongly typed values are
  rejected) · `settings_set { settings }` → Settings (**deprecated**, replaces every
  setting; kept for one release)
- Updates: `update_status()` → `{ current, ready? }` · `update_check()` → new version or
  null · `update_install_now()`

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
- `player://event` — player events from either backend (`PlayerEvent`, kebab-case
  tagged; see "Player backends")
- `library://changed` — `{ ids }`, the library items that changed, after every committed
  change (the user's, or saved playback progress)

High-frequency data (engine stats, player position) will move to `tauri::ipc::Channel`
subscriptions when the embedded player lands.

## Persistence

All durable state is Rust-owned JSON under the app data dir; there is no localStorage.
`src-tauri/src/persist.rs` holds the rules: a missing file reads as defaults; an
unparseable one is moved to `*.json.corrupt.bak` and replaced by defaults; any other IO
error fails startup; writes are atomic and durable (`*.json.tmp`, flushed to disk, then
renamed over the target). `AppState` keeps
`settings.json` and `addons.json`. Every later feature owns a `JsonStore<T>` (load / get /
update-and-persist under its own lock), registered with `app.manage` — it does not grow
`AppState`. New files carry a `{"version": N, ...}` envelope. Per-user files take their
path from `AppState::profile_dir()` (the data dir until profiles exist, then the active
profile's folder); re-creatable caches go under the cache dir. The first such store is
the library, `library.json` (`{ version, uid, items }`, see "Library and progress").

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
| Per device | **`mpvPath`**, **`engineListenPort`**, **`engineUpnp`**, **`uploadLimitKibps`**, **`downloadLimitKibps`**, **`deleteOnStop`**, **`autoUpdate`**, **`playerBackend`** (`auto`/`embedded`/`external`), `marketplaceEnabled`, `marketplaceSources`, `stremioDeepLink` |
| Per profile | **`theme`**, **`instantPlay`**, **`hideAnime`**, **`hideLiveTv`**, `showAdult` (the single adult flag and age gate, also used by the marketplace), `hideP2pAddons`, `homeRows`, `subtitles{}` (languages, auto-load, style), `audio{}` (languages, normalize, volume max), `skip{}` (intro/outro auto-skip), `bingeWatching`, `upNextSeconds` |
| Built-in toggles (scope is an owner decision; device until then) | **`builtinCinemeta`**, `builtinOpensubtitles`, `builtinKitsu` |

Per-profile keys live in `settings.json` until local profiles exist, then move into the
profile store. There is no `profile{}` group in Settings.

## Library and progress

The library is local-first and Stremio-compatible, so a later, opt-in account sync can
exchange items with Stremio's own apps without loss or surprises.

- **Data** (`crates/zeiler-account`, pure: no I/O, no clock). `LibraryItem` mirrors
  stremio-core's `libraryItem` field for field — lenient in (floats, nulls, strings in
  integer fields; unknown fields kept at every level), strict out (stremio-core's field
  names and order, integers, `video_id`, RFC 3339 `Z` times). The watched-episodes
  bitfield is a port of stremio-watched-bitfield (`{anchor}:{len}:{base64(zlib)}`,
  parsed from the right because episode ids contain `:`, re-aligned on the anchor).
  `removed`/`temp` mean what they mean in Stremio: a title played but never added is
  `removed + temp` and shows only in Continue Watching. Video order follows
  stremio-core too: the next episode is taken from the meta's own order (duplicates
  dropped, specials last, videos without episode numbers by date), the bitfield from
  `watched_bitfield`'s; a season or episode that is not a 32-bit number reads as
  absent, never as an error. All of the meta's behaviour hints are kept on the item.
  Marking the episode Continue Watching points at moves it on, as Stremio's apps do.
- **Store** (`src-tauri/src/library/`). `library.json` under `profile_dir()`, one bucket
  keyed by id (`uid: null` until account sync). A damaged item is skipped, not the file.
  Every change is made on a copy and committed only when the item really changed
  (`_mtime` stamped then), then announced as `library://changed`.
- **Progress comes from the player, not the UI.** The Player screen passes the launch's
  `context` (meta summary, video id, the title's videos) to `player_start`; the shell
  feeds that session's `PlayerEvent`s to a tracker (`library/progress.rs`), whichever
  backend plays and also after Back leaves external mpv playing. stremio-core's player
  rules apply: time watched accumulates only from playback (a `seek` event, a jump
  beyond what the wall clock allows, or a step back counts as a seek); more than 70% of
  the duration watched marks the video watched (its bit set, `timesWatched + 1`);
  stopping past 90% drops the resume position and moves Continue Watching on to the
  next released episode (never across into the specials). Live TV is never tracked.
  Writes happen every 15 s while playing, on pause, at the end of the file and when the
  session ends (replaced, stopped, loaded over, shut down, or the app exits, which waits
  up to 3 s). One worker applies them in order, and a session is always closed before
  the next one starts, so a session's last write can never land after the next one's.
  A playback that never reported a position writes nothing. The last-resort external
  launch without JSON IPC has no position feed and saves nothing.
- **Robust counting.** Watched time is the distance between consecutive reports of
  one run. When the stored position moved underneath a run (the user dismissed the
  title or marked it watched while external mpv plays on, or a write failed), the run
  re-anchors first, so a skip is never counted. Only the file the playback loaded (or
  its redirect) is tracked; another file opened in the same mpv ends the session. The
  context arrives as plain JSON and is read inside the command, so a malformed one
  costs only the library entry, never the playback. A failed write is retried once,
  and a session's batches count as its first until one is saved.

## Player backends

`zeiler-player` owns one backend-neutral `Session` (`Embedded | External`) with
`load`, `set_property`, `command`, `get_property` (answered through a oneshot) and
`quit`; both backends emit the same `PlayerEvent` stream and observe the same property
set (`OBSERVED` in `crates/zeiler-player/src/lib.rs`). The shell
(`src-tauri/src/commands/mpv/`) keeps at most one session and forwards its events as
`player://event`; events of a replaced session are dropped.

- **Choosing a backend** (in Rust, because embedded init and video-output failures
  are synchronous). `player_start`'s `backend` defaults to `auto`, which follows the
  `playerBackend` setting. `auto` = embedded when this build has it and it starts,
  else external; an embedded init/VO failure falls back to external and the result
  carries `fallbackReason`. An explicit `embedded` never falls back (`unsupported` when
  this build/OS has none, `player` when it fails). External needs mpv
  (`mpvPath` or PATH/known locations).
- **Embedded** (`embedded` feature; on by default in Windows releases): libmpv actor +
  event thread (`crates/zeiler-player/src/embedded.rs`), `config=no`, no
  scripts/ytdl/OSC/input, `vo=gpu-next,` (the trailing comma falls back through mpv's
  default VO list instead of failing on RDP, VMs or old GPUs), `hwdec=auto-safe`. On
  Unix it resets `LC_NUMERIC` to `"C"` before creating a core (GTK sets the user's
  locale, and libmpv refuses to start otherwise). `EmbeddedOptions { surface }` says
  where the video goes, fixed at spawn:
  - `Surface::Wid(hwnd)` — a child window of the main window (Windows; with
    `force-window=immediate`);
  - `Surface::RenderApi` — reserved for macOS (`vo=libmpv`, drawn by the host into an
    NSOpenGLView, M13); not implemented yet, spawning with it fails;
  - `Surface::Detached` — mpv's own top-level window;
  - `Surface::Headless` — `vo=null`, `ao=null`, for tests.

  Per-OS surface code: `commands/mpv/windows.rs` (child HWND pinned under the
  transparent WebView2), `macos.rs` / `linux.rs` (not available yet: macOS is next,
  Linux keeps mpv in its own window), `stub.rs` (feature off). `player_backends`
  reports why built-in playback is unavailable in a sentence Settings → Player shows
  as-is (per OS: coming soon on macOS, mpv window on Linux, a Windows build without
  the feature, or a missing `libmpv-2.dll`).
- **Windows surface and transparency policy** (`commands/mpv/windows.rs`). The mpv
  child window is pinned at `HWND_BOTTOM` with the UI's rect and made transparent to
  hit-testing; the WebView2 above it gets a transparent default background **only
  while built-in video plays**: `spawn_embedded` sets it, and `player_stop` (or a start
  that ends on external mpv or fails) restores WebView2's opaque default, as at
  startup; a built-in session replaced by another keeps it, so nothing flashes (Harbor instead makes the webview transparent once at setup;
  Zeiler keeps the rest of the app on the ordinary opaque path). The page paints its
  own background whenever no built-in session runs, so that white default never
  shows: the player screen turns transparent (and clears the document background)
  only once `player_start` has answered "embedded" (`videoBehind`), not on the
  loading screen's early guess, and paints the canvas again when the session ends.
  After the first successful pin the keyboard focus goes back to the webview once
  (mpv's new child window may take it); `player_surface_refresh` re-applies the
  transparency, re-pins the child with the last geometry and re-focuses. Focusing
  means the WebView2's own `MoveFocus` (`Webview::set_focus`), and only while the main
  window is the foreground window — it never pulls focus from another app.
  An init or VO failure of the embedded core falls back to external mpv under `auto`
  (see "Choosing a backend").
- **External, JSON IPC** (`crates/zeiler-player/src/external_ipc.rs`): spawns the
  user's mpv with `--input-ipc-server=<endpoint> --idle=yes --keep-open=yes
  --force-window=immediate --title=Zeiler --ytdl=no`, keeping the user's own config,
  scripts and bindings. The URL, headers (`change-list http-header-fields clr` +
  `append`), user agent, referrer, `start` and title go over the connection — never on
  argv. Endpoint: Windows named pipe `\\.\pipe\zeiler-mpv-<uuid v4>`; Unix socket
  `zeiler-mpv-<8 hex>.sock` in a private 0700 directory (`$XDG_RUNTIME_DIR` when short,
  else `/tmp/zeiler-<uid>`; macOS caps socket paths at 104 bytes). Connects with retry
  (50 × 100 ms), observes every property, then loads. When mpv exits (including the
  user closing its window) the session emits `shutdown`, the shell forgets it, and the
  socket is removed. If the IPC never comes up, the v0.1 launch (URL on stdin, headers
  on argv, no control) is the last resort and `player_start` reports
  `remoteControl: false`.
- **Events.** `property { name, value }` for every observed property (`time-pos`
  throttled to 200 ms, held rather than dropped; lists as real JSON; track ids in
  mpv's string form `"1"`/`"no"`/`"auto"` on both backends; `null` once a property
  becomes unavailable, e.g. `duration`/`chapter` after `end-file` — libmpv2 drops those
  notifications, so the embedded backend re-reads the properties it last reported at
  `start-file`/`file-loaded`/`end-file` and on an early wake-up, and reports `null` for
  any that are gone) ·
  `start-file` · `file-loaded` · `seek` · `playback-restart` · `end-file { reason,
  fileError? }` (preceded by `error` when reason is `error`) · `error { message }` ·
  `client-message { args }` · `shutdown`. One cache property is observed,
  `demuxer-cache-time` (buffered-until timestamp; `demuxer-cache-state` is a large,
  chatty node no UI needs).
- **Per-stream HTTP.** Both backends clear `http-header-fields` before every load and
  append the request's headers plus its referrer as a `Referer` header (mpv's
  `referrer` option, once set, can only be emptied, and mpv would then send an empty
  `Referer:` with later streams). mpv sends that option as its own `Referer` line ahead
  of the list, so when the user's mpv config sets `referrer`, external mpv sets the
  option per load instead (the stream's referrer, else the user's) and appends no
  second `Referer`. A request without a user agent gets the backend's
  default back (libmpv's `libmpv`; the user's own for external mpv), never an empty one.
- **Requests.** `PlaybackRequest` is an IPC payload: http(s) only; header names must be
  HTTP tokens and no header value, user agent or referrer may contain CR, LF or NUL
  (refused with an error naming the field). The title is display text — addon stream
  names are often multi-line (`"Addon\n1080p"`) — so it is flattened to one line
  (control characters become a space, NULs are dropped), never refused. Its shell-only `options` (pre-load mpv options such as preferred languages) are
  `#[serde(skip)]` — the webview can never set them.
- **Validation policy** (`commands/mpv/validate.rs`, table-driven, unit-tested). The UI
  may set only listed properties, each value checked (booleans, including `fullscreen`
  — the external mpv window's; embedded video has no window of its own; numeric ranges such as
  `speed` 0.25–4, `sub-delay`/`audio-delay` ±600, `volume` 0–1000; `#RRGGBB[AA]`
  colours; enums; track ids as integer ≥ 1 or `no`/`auto`; subtitle fonts from a fixed
  list). Commands: `seek <number> [flags]`, `cycle <settable property> [up|down]`
  (the target is checked against the property table so `cycle` cannot reach around
  it), `stop`, `frame-step`, `frame-back-step`, `sub-remove <id>`, `sub-reload <id>`.
  Reads: diagnostics (codecs, fps, bitrates, drops, cache, `mpv-version`) and the
  observed set. Never exposed: `loadfile`, `sub-add`, `af`, `vf`, `glsl-shaders`, `run`,
  `script-message`, `keybind`, `define-section`; features that need them get dedicated,
  validated commands.

## Navigation names

The frontend navigates with a frame stack (`src/lib/view.tsx`). Every switch over
`RootView` or `Frame` kinds ends in `unreachable(x)`, so a new kind that is not handled
everywhere fails `tsc`. Reserved now, unreachable from the UI until their milestones:
root view `live`; frames `browse` (catalog browser) and `addon-detail` (marketplace).
Reserved Settings section ids: `account`, `subtitles`, `playback`.

The Player is not a frame: it is the `#/player?h=<id>` hash route, drawn over the
still-mounted shell (see "Playback entry point"). The frame stack is untouched while it
is up; leaving the player pops the history entry it pushed.

## Security posture

- Strict CSP (no remote script/style; `img-src https: http:` because addon posters are
  arbitrary hosts; loopback wildcards for the engine stream only).
- Least-privilege capabilities; no fs/shell/http plugin — the webview can only invoke our
  typed commands. Player property writes, reads and commands pass the allow-list tables
  in `commands/mpv/validate.rs`; stream URLs, headers and tokens reach mpv over its
  private IPC connection, never argv.
- Addons are untrusted JSON: stream/external URLs are scheme-allow-listed before reaching
  mpv or the OS (mpv would happily open `file://`); `proxyHeaders` are treated as
  attacker-controlled; infoHashes validated as 40-char hex.
- Addon transport URLs may embed credentials (config-in-URL pattern) — they are secrets;
  never logged, never sent anywhere but the addon host.
- Playback goes through the in-memory one-shot handoff, never the location hash: stream
  URLs, headers and transport URLs stay out of the history stack, and release builds
  ignore URL-carrying player params.
- Engine HTTP binds 127.0.0.1 with per-session basic-auth credentials.
- Deep links (`zeiler://`; `stremio://` opt-in later) are untrusted argv input and are
  parsed/validated before any navigation or install prompt.

## Packaging & licensing notes

- `license = "GPL-3.0-or-later"` on every crate; LICENSE at root; THIRD-PARTY notices for
  Apache-2.0 (librqbit) and any MIT-attributed code adapted from stremio-core/Harbor.
- libmpv is **not** statically linked. Windows releases ship `libmpv-2.dll` beside the
  exe via `bundle.resources` (`src-tauri/tauri.embedded.conf.json`, which also installs
  the LGPL-2.1 text from `src-tauri/licenses/` as `licenses/LGPL-2.1.txt` and the fetch
  script's notice — versions, provenance, source links and the written source offer —
  as `licenses/libmpv-NOTICE.txt`). It is the LGPL
  flavour of zhongfly/mpv-winbuild, pinned by tag and archive SHA-256 in
  `scripts/fetch-libmpv.mjs`; THIRD-PARTY-NOTICES.md names the exact mpv and FFmpeg
  commits and carries the source offer. The archive has no `.def`, so the script writes
  one from the DLL's export table (`LIBRARY libmpv-2.dll`) and generates `mpv.lib` with
  MSVC `lib /def:`. The app links it DELAYLOAD (`src-tauri/build.rs`) so it starts —
  and falls back to external mpv — when the DLL is missing. macOS (M13) will bundle a
  vendored dylib under Frameworks; Linux keeps the distro mpv. If a GPL libmpv/FFmpeg
  build is ever shipped, the corresponding source offer goes in the release notes.
- `deny.toml` allows `LGPL-2.1` for the `libmpv2`/`libmpv2-sys` bindings.
- Windows installer: NSIS per-user only (no MSI/WiX). Unsigned builds will trip
  SmartScreen/Smart App Control — SignPath Foundation (free for GPL) is the plan.

## CI coverage of the player

- **Linux** (`ci.yml`, ubuntu-24.04): installs `libmpv-dev` and runs clippy and the tests
  of `zeiler-player --features embedded` — the actor's headless tests
  (`crates/zeiler-player/tests/embedded.rs`: lavfi tone and test pattern, track list as
  JSON, pause/seek events, per-stream headers/user agent/referrer against a loopback
  HTTP server and their absence on the next load, quit → shutdown).
- **Windows** (`ci.yml`, windows-latest): restores `src-tauri/libmpv` from a cache keyed
  on the fetch script (else runs it, with MSVC `lib.exe` found through vswhere), lints
  `-p zeiler -p zeiler-player --features zeiler/embedded` (`ZEILER_SKIP_LIBMPV_CHECK`,
  check mode never links) and runs the player tests with `src-tauri/libmpv` on `LIB`
  (libmpv2-sys only emits `link-lib=mpv`) and on `PATH` (test executables load
  `libmpv-2.dll` directly; only the app is DELAYLOAD).
- **Pull requests** build the Windows NSIS installer with built-in playback (no updater
  artifacts, no signing secrets) and attach it to the run as an artifact, so it can be
  tried before a release. `release.yml` builds the Windows entry with
  `--features embedded --config src-tauri/tauri.embedded.conf.json --config
  src-tauri/tauri.release.conf.json` (the CLI merges repeated `--config` in order).
- Tests that need the `mpv` program (`tests/external_mpv.rs`) stay `#[ignore]`d: CI
  runners have no mpv binary.

## Milestones

1. **v0.1 (current)** — workspace + addon client + catalogs/meta/streams UI + torrent engine
   + external-mpv playback; CI (fmt, clippy, test, build).
2. **v0.2** — embedded libmpv (Windows `wid`, on by default in Windows releases),
   subtitles (addon + local file), library/continue-watching persistence, updater +
   release pipeline.
3. **v0.3** — macOS/Linux embedded playback, stream ranking (Harbor-core-inspired
   parse→trust→score→rank), deep links, binge/next-episode (bingeGroup).
4. **Later** — casting, Live TV/EPG, PiP, theme system, optional Stremio account sync
   (isolated, off by default).
