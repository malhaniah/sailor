# Completeness critique of the seven Zeiler area specs

Zeiler paths are absolute. Harbor paths are relative to `HARBOR=/tmp/claude-0/-home-user-sailor/3ec38e8b-1ace-545e-a82a-ac4418d8c2a1/scratchpad/ref/harbor`. Everything below was checked read-only against both trees.

## 1. Owner goals not covered, or only partly covered

**1. Built-in player on Linux, and on macOS for a long time.**
- Linux embedding is only "Phase B": optional, with no prototype. macOS is embedded steps 6–7 (XL work).
- Until those land, Linux and macOS users get an external mpv window. The in-player external JSON-IPC backend at least gives them a remote-control UI.
- Harbor's Linux route (an HTML5 `<video>` element) is not viable here and no spec says so. The CSP sets `media-src 'self'` (`/home/user/sailor/src-tauri/tauri.conf.json:33`), and WebKitGTK cannot decode MKV/HEVC.
- The owner needs to accept "Windows first, Linux external for now" explicitly.

**2. Live TV playback.**
- No player spec handles live streams: duration of 0 or infinity, no seek bar, no progress writes, no skip or next episode.
- Catalog step 8 hands the `iptv_resolve` result straight to `player_embedded_start`/`player_play`. IPTV stream URLs carry Xtream credentials, so they would bypass the one-shot handoff and end up in the location hash.

**3. Essentials for "enjoying the experience" that no spec owns:**
- **First run and empty states.** Zeiler ships no stream addon, so a new user browses catalogs but the picker has nothing to play. No spec defines a first-run flow or a "no streams" state in PlayPicker that points to Sign in, the Addons page or the marketplace. It must stay content-neutral: no named addons.
- **Local library, resume and Continue Watching.**
  - This is buried in the XL account spec, but it is an ARCHITECTURE v0.2 milestone (`/home/user/sailor/docs/ARCHITECTURE.md:121-122`).
  - `/home/user/sailor/src/views/Library.tsx:14-16` already promises it "with the embedded player milestone".
  - It should be its own milestone, before sign-in.
  - The Home "Continue Watching" row has no owner: catalog `rooms.ts` doesn't include it, and account step 8 covers only the Library view.
- **Stream ranking and sorting in the picker** (ARCHITECTURE v0.3, "stream ranking"). No spec covers it, and the in-player "quality" switcher reuses unsorted picker rows.
- **Watched marks on the Detail episode list.** Account step 8 has the commands but lists no Detail UI.

**4. Goal 7 (built-in OpenSubtitles) has a blocking policy gate that nobody scheduled.**
- `/home/user/sailor/CONTRIBUTING.md:16-18` allows metadata-only built-ins.
- The amendment ("metadata or subtitles only, switchable, never streams") needs the owner's sign-off before either spec's OpenSubtitles v3 (OSv3) step can merge.

## 2. Claims in the specs that are wrong (verified)

### Embedded-playback spec

- **"embedded.rs (376 lines)… setlocale TODO lines 175-177".** The file is 510 lines, and the TODO is at `/home/user/sailor/crates/zeiler-player/src/embedded.rs:215-217`.
- **"embedded.rs has zero tests".** Wrong: `embedded.rs:446-509` has four unit tests (readable errors, error-code coverage, init causes, version mismatch). The part that is right is that CI never compiles them.
- **"Player.tsx engineResolve omits fileMustInclude".** Wrong: `/home/user/sailor/src/pages/Player.tsx:88` parses it and line 256 passes it on.
- **"Verify @tauri-apps/cli accepts a repeated --config".** It does. The CLI binary's help says "Configurations are merged in the order they are provided", so renaming the overlay to `tauri.windows.conf.json` is unnecessary.
- **Step 4/5 CI commands for Windows (also in the conventions spec's testing step).** `node scripts/fetch-libmpv.mjs` followed by `cargo test -p zeiler-player --features embedded` will not link:
  - `libmpv2-sys-4.0.1/build.rs` only emits `cargo:rustc-link-lib=mpv`.
  - The link-search path and DELAYLOAD live only in `/home/user/sailor/src-tauri/build.rs:8-23`, and they apply only to the `zeiler` crate.
  - The test executable also needs `libmpv-2.dll` on PATH at load time, because it is not delay-loaded.
  - Fix: add `LIB` (or `RUSTFLAGS=-L native=src-tauri/libmpv`) plus PATH setup to the CI step.
- **Harbor macOS geometry ("likely bug").** Confirmed:
  - `$HARBOR/src/views/player/hooks/use-player-bridge.ts:60-65` multiplies by `devicePixelRatio`.
  - `$HARBOR/src-tauri/src/mpv_render_mac.rs:218-236` uses those values directly as an NSView frame, which is in points.
- **Missing from the spec: `deny.toml` licence allow-list.** `/home/user/sailor/deny.toml:5-21` has no `LGPL-2.1`, while `libmpv2` 6.0 and `libmpv2-sys` 4.0.1 are `LGPL-2.1`. `cargo deny check` with the `embedded` feature will fail once embedded ships.

### In-player spec

- **Step 3 adds a new `player_stop`.** That name already exists: `/home/user/sailor/src-tauri/src/commands/player.rs:46-50`, registered at `src-tauri/src/lib.rs:67`. Repurpose the existing command; do not register a duplicate.
- **Step 1 adds `options: Vec<(String,String)>` to PlaybackRequest, "never set by the UI".** This is not enforceable as written:
  - `PlaybackRequest` is `#[serde(default)] Deserialize` (`/home/user/sailor/crates/zeiler-player/src/lib.rs:33-42`).
  - It is the IPC payload of `player_play` and `player_embedded_start` (`commands/mpv.rs:77-84`).
  - So the webview could inject arbitrary mpv options. The field must be `#[serde(skip)]`, or live in a shell-only struct.
- **Step 2's Unix socket path `<cache_dir>/ipc/mpv-<uuid>.sock` can overflow macOS's 104-byte `sun_path`.** `/Users/<u>/Library/Caches/io.zeiler.desktop/ipc/mpv-<36 chars>.sock` is 90 bytes plus the length of the username. Use a short random name in a short 0700 directory.
- **Step 13 (the Player.tsx split) is listed last.** Steps 6, 10 and 12 already create `src/pages/player/*` and edit Player.tsx, so the split must come first.
- **Smaller slips:**
  - The route-param list omits `fileMustInclude`.
  - OBSERVED is at `embedded.rs:39-55`, not 34-50.
  - `COMMAND_ALLOW` admits `cycle <any property>` (`commands/mpv.rs:58`), which bypasses PROPERTY_ALLOW. The new validation table should check `cycle`'s target too.

### Catalog spec

- **The built-in "no stream" guard test is unsound.** `is_resource_supported` returns false whenever the sample id misses `idPrefixes` (`/home/user/sailor/crates/zeiler-addon/src/manifest.rs:101-105`). An addon serving streams only for `kitsu:` ids would therefore pass a `tt` sample. Assert that no resource is named `stream`. The same fix applies to the conventions step 0 test and to the runtime check that accepts a refreshed manifest.
- **The CatalogRef key `${manifest.id}|type|id|extra` collides** for two configured installs of the same addon (same manifest id, different URLs). Add a short non-reversible hash of the transport URL.
- **The OSv3 manifest id `org.stremio.opensubtitlesv3` is a guess.** Harbor hides `org.stremio.opensubtitles` and `com.opensubtitles.v3` (`$HARBOR/src/lib/addons-store/store.ts:11-14`). Dedupe by the id in the fetched snapshot.
- **Attribution needs two fixes, not one.** Besides the copyright line ("Copyright (c) 2026 Harbor"), the URL is off:
  - Zeiler cites `github.com/9000000/harbor` (`/home/user/sailor/THIRD-PARTY-NOTICES.md` and `embedded.rs:12`). That is the reference checkout's git origin.
  - Harbor's own README links `github.com/harborstremio/harbor` (`$HARBOR/README.md:636-638`).

### Account spec

- **Wrong line references.** "commands/addons.rs:168-178" cannot be right: the file has 66 lines, and identity is matched at `addons.rs:33-41`. "client.rs:94-104 / 106-136" also miss: the file has 123 lines, with the UA at 22-31 and the size cap at 33-64.
- **"CI images lack libdbus-1-dev" is overstated.** It arrives transitively through `libwebkit2gtk-4.1-dev` → `libatspi2.0-dev` (this Ubuntu 24.04 container has 1.14.10 installed, and libatspi2.0-dev depends on libdbus-1-dev). Listing it explicitly is harmless but not required.
- **Step 5 says "move addons.json into profiles/<primary>/", while its own risks say copy, not move.** It must be a copy:
  - `migrate_legacy_data` (`/home/user/sailor/src-tauri/src/state.rs:84-106`) reads `data_dir/addons.json`.
  - So do older builds.
- **Addon sync handles only Add/Remove.** The marketplace's `addon_reorder` is never pushed, and because the account collection is treated as the source of truth, the next pull reverts the local order.

### Marketplace spec

- **The stremio:// handler cannot be "opt-in and reversible" on macOS.** `tauri-plugin-deep-link` 2.6.2 returns `UnsupportedPlatform` for `register`, `unregister` and `is_registered` on macOS (`src/lib.rs:140-165`). macOS needs a static scheme in `tauri.conf.json`, which claims stremio:// at all times.
- **`show_adult_addons` duplicates the catalog spec's `show_adult`.**

### Verified correct (no change needed)

- **Config:**
  - The CSP (`connect-src` ipc only, `frame-src 'none'`).
  - Release runs on ubuntu-22.04; the deb depends on `["mpv"]`.
  - CI never builds `embedded`.
  - Nothing in production navigates to `#/player`.
- **Data:** the Cinemeta snapshot has 19/22 genres, no Music, and year options running 2026→1920/1960.
- **libmpv2 6.0 API:**
  - The VersionMismatch check is at `libmpv2-6.0.0/src/mpv.rs:328-334`.
  - `RenderContext<'a>` holds `PhantomData<&'a Mpv>` (`render.rs:11-15, 210-213`).
- **Dependencies:**
  - tokio lacks the `net` feature.
  - Already in Cargo.lock: chardetng, encoding_rs, flate2, objc2 0.6.5, objc2-app-kit 0.3.2, gtk 0.18.2, webkit2gtk 2.0.2.
  - `serde_with` is not yet a dependency of src-tauri.

## 3. Conflicts between specs, and how to resolve them

**C1. Player launch contract: four incompatible designs.**
- The designs:
  - Embedded step 1: a one-shot handoff `#/player?h=<id>`, with App kept mounted.
  - In-player step 8: a `play-context.ts` store, a hash of `type/metaId/videoId`, and `pendingPicker` applied when App remounts.
  - Account step 7: `metaId/type/videoId/name/poster` in the hash.
  - Conventions: snapshot the stack, or make the player a Frame.
- **Resolution:** one `PlayerLaunch` handoff carrying:
  - the request or torrent
  - meta: type, id, name, poster, videos
  - video: id, season, episode
  - stream: behaviorHints including bingeGroup, filename, videoHash and videoSize; subtitles
  - transportUrl and addonName
  - a `live` flag
- App stays mounted. "Open the picker for the next video" becomes a ViewProvider action instead of a remount.
- **Must-fix for keeping App mounted: its global key listeners stay live.**
  - Escape: `/home/user/sailor/src/lib/view.tsx:102-115` pops the picker frame, while the Player's own Escape (`Player.tsx:429`) calls `history.back()`. The user lands on Detail, not the picker.
  - Slash: `/home/user/sailor/src/chrome/Topbar.tsx:15-26` opens the hidden SearchOverlay, which focuses its input (`SearchOverlay.tsx:29`). From then on the Player ignores every key (`Player.tsx:402`).
  - Fix: a `playerActive` guard in those listeners, plus `inert` on the hidden App root.

**C2. Settings lost update.**
- `useSettings().save` sends the whole stale object (`/home/user/sailor/src/lib/store.tsx:79-90`), and `settings_set` replaces everything (`commands/settings.rs:13-27`).
- Player-side writes (subtitle quick style, auto-skip toggles, backend preference) are therefore wiped by the next App save.
- **Resolution:** a server-merged `settings_patch` command plus a `settings://changed` event, both in M0.

**C3. Player IPC naming and where fallback is decided.**
- Embedded step 2 adds `player_backends` and decides the fallback in Player.tsx. In-player step 3 adds `player_start{backend}`, decides it in Rust, and collides on `player_stop`.
- **Resolution:** Rust decides, because init and VO failures are synchronous (`readable_init`, `embedded.rs:115-131`). `player_start` returns the backend it used. `player_backends` replaces `player_status` and `player_embedded_available`. Old names stay as aliases for one release.

**C4. The same files are edited by both player specs.**
- `commands/mpv.rs`: embedded step 3 splits it into `commands/mpv/{mod,windows,macos,linux,stub}.rs`; in-player adds validators, subtitle-add and OSD prompts to it.
- `embedded.rs`: the Surface enum versus a PlayerEvent move, a Session enum and GetProperty. Both add observed properties (`demuxer-cache-state` vs `demuxer-cache-time`).
- **Resolution:** do the split and API change once, in M2.

**C5. External mpv IPC endpoint.** Embedded step 8 Phase A uses `$XDG_RUNTIME_DIR/zeiler-mpv-<pid>.sock`; in-player step 2 uses `cache_dir/ipc/<uuid>`. Use the in-player design with the short path from §2.

**C6. OSv3 built-in specified twice** (in-player 4(e) and catalog step 2). One owner: the built-in registry milestone. Both already name the setting `builtinOpensubtitles`.

**C7. Settings schema: flat vs nested, device vs profile.**
- Proposed keys by spec:
  - Conventions: nested `subtitles`, `skip` and `profile` groups.
  - In-player: `subtitles{}`, `audio{}`, `skip{}`, `bingeWatching`, `upNextSeconds`.
  - Embedded: `playerBackend`.
  - Catalog: `builtinKitsu`, `builtinOpensubtitles`, `showAdult`, `homeRows`.
  - Marketplace: `marketplaceEnabled`, `marketplaceSources`, `showAdultAddons`, `hideP2pAddons`, `stremioDeepLink`.
  - Account: moves `theme`, `instantPlay`, `hideAnime` and `hideLiveTv` into `profiles.json`, which makes conventions' `profile` group obsolete.
- **Resolution: fix one key map in M0.**
  - Merge the two adult flags into one `showAdult` with one age gate.
  - Per profile: content filters, homeRows, subtitle/audio language and style, skip, binge, theme.
  - Per device: mpvPath, engine settings, playerBackend, autoUpdate, marketplace settings, deep link.
  - The built-in toggles need an explicit owner decision.
  - Drop the `profile` group.
- **Every new enum-typed field must be `DefaultOnError`** (playerBackend, borderStyle, assOverride, `stremio.mode`). Any parse error makes `read_json` move all of settings.json to `.corrupt.bak` and reset every setting (`state.rs:207-226`).

**C8. Storage locations.**
- Account puts files under `profiles/<id>/`. In-player puts `player-prefs.json` in AppState. Catalog uses `AppState::persist_iptv` and keeps its built-in manifest cache in `data_dir`. Conventions says each feature gets its own `JsonStore`, and caches belong in `cache_dir`.
- **Resolution:** M0 adds a `profile_dir()` helper that returns `data_dir` until profiles exist. All new per-user files go through it via `JsonStore`.

**C9. Secrets policy.** The authKey goes to the OS keychain (account spec), but Xtream username and password sit in plaintext in `iptv.json`, and IPTV URLs travel inside PlaybackRequest. **Resolution:** reuse the account milestone's `SecretStore` for Xtream credentials, and send IPTV playback only through the one-shot handoff.

**C10. Adult filter implemented twice** (TS in catalog Browse, Rust in `marketplace directory/adult.rs`). One Rust implementation in zeiler-addon.

**C11. Addon identity and order.**
- Marketplace adds replace-in-place and reorder. Account sync wraps add/remove only. Account's "never push built-in Cinemeta" must cover every built-in.
- **Resolution:** land the marketplace install path first; then account adds `Op::Reorder` (or pushes the full ordered list after a fresh get).

**C12. Navigation and section names.**
- Frames: conventions uses `addon-detail`, marketplace uses `{kind:"addon"}`, catalog adds `browse`. Root view `live` comes from both catalog and conventions.
- Subtitle settings: conventions makes them a separate settings section, while in-player puts them in a card inside the Player section.
- **Resolution:** freeze names in M0: frames `browse` and `addon-detail`; root `live`; settings sections `account`, `subtitles`, `playback`.

**C13. Player UI directory.** `src/components/player/` (conventions) vs `src/pages/player/` (in-player). Pick one in M1.

**C14. Ordering.** Conventions lets account/profile land any time after M0, but account's progress tracking needs C1's context and position events from both backends. The library has to follow the player core.

## 4. Recommended implementation order

Every milestone must pass on Linux, Windows and macOS: fmt, `clippy -D warnings`, tsc and `cargo test --locked`. No milestone seeds a stream addon.

| # | Milestone | Contents | CI and ship notes |
|---|---|---|---|
| **M0** | Foundations (no user-visible change) | `persist.rs` JsonStore; `builtin.rs` registry (Cinemeta only) with a no-`stream`-resource test; the C7 key map with `DefaultOnError`; `settings_patch` plus changed event; `profile_dir()`; IpcError kinds; exhaustive `never` checks; pre-registered frames and sections; doc and type drift fixes (`ResolvedStream`, ARCHITECTURE IPC list) | Pure refactor and tests; green everywhere |
| **M1** | In-app player route (all OSes) | PlayerLaunch handoff; App kept mounted with the `playerActive` guard and `inert`; PlayPicker routes URL and torrent streams to the player; Player.tsx split plus playback-clock store; external path unchanged (fire-and-forget card) | tsc only; ships everywhere |
| **M2** | Player core + external mpv JSON-IPC backend | `commands/mpv/` split; backend-neutral `player_start/load/stop/set_property/command/get_property` with table validation (including `cycle`); `PlayerEvent` in lib.rs; `options` not deserializable; tokio `net`; short socket paths; `player_backends` + Settings radio; fixture tests for the line codec | Named-pipe code compiles only on Windows CI and Unix-socket code on Linux/macOS, so keep them cfg-split to avoid dead-code warnings. Linux and macOS get full remote-control UIs |
| **M3** | Windows embedded on by default | `vo="gpu-next,"`, Surface enum, Unix setlocale, dwidth/dheight/cache properties, `player_surface_refresh` kick schedule, init failure falls back to external IPC | Windows CI: fetch (cached), clippy with `ZEILER_SKIP_LIBMPV_CHECK`, tests with LIB/PATH set. Linux CI: `libmpv-dev` + headless `--features embedded` tests. `deny.toml` gains LGPL-2.1. release.yml Windows: `--features embedded --config tauri.embedded.conf.json --config tauri.release.conf.json`. Notices, source offer, and release-body/README updates |
| **M4** | Local library, resume, Continue Watching | stremio-core-compatible LibraryItem and progress arithmetic (account steps 1 and 3 types, no network); `library_*` commands; Library view; Home CW row; Detail watched marks | Pure Rust tests |
| **M5** | Policy gate + built-in rows | Owner-approved CONTRIBUTING/ARCHITECTURE amendment; OSv3 (subtitles) and Anime Kitsu (metadata) snapshots; refresh script; per-built-in toggles and disclosures; meta-type fallback for `kitsu:`/`mal:` ids | Snapshot tests offline; live tests `#[ignore]` |
| **M6** | Subtitles and audio | `subtitles_get` fan-out; `subfile.rs` normalization; `player_subtitle_add` / pick-local; ranking and auto-select; menu, delay, style and prefs; audio delay/normalize/speed; stats; chapter ticks | Rust fixture tests; adds tauri-plugin-dialog (Rust-side only) |
| **M7** | Skip and next episode | `zeiler-skip` crate (AniSkip, TheIntroDB), chapter fallback, pill, Up Next card, auto-skip off by default with Undo, bingeGroup next episode via `player_load` | Fixture tests; privacy toggle |
| **M8** (parallel after M0) | Catalogs | `default_extra` / `filter_props`; `catalog_load` validation and cache; rooms.ts; Browse view; manifest-driven GenreTiles; lazy rows with a concurrency cap; row customization; adult gating via the shared Rust filter | Kitsu rows activate once M5 lands |
| **M9** | Live | A: addon `tv`/`channel` room plus Sidebar entry. B: `zeiler-iptv` crate (M3U, XMLTV, Xtream), with credentials in SecretStore (after M11/M12 or an M0 stub) and playback via the handoff with `live` set | Parser fixtures |
| **M10** | Marketplace | A: URL normalization, id/host replace, reorder, update detection, masked URLs. B: opt-in directory (stremio-addons.net + Cinemeta `addon_catalog`), detail frame, text-only docs. C: stremio:// handler on Windows/Linux at runtime; macOS needs an owner decision because the static scheme is always claimed | Fixture tests; live tests ignored |
| **M11** | Local profiles | `profiles.json`, copy-not-move migration, Argon2 PIN, avatars, picker, per-profile addons, library and prefs | Temp-dir migration tests |
| **M12** | Stremio account | `zeiler-account` crate; link-code flow first, then password; keychain with memory fallback (tests use MemoryStore only); library sync; addon sync with Add/Remove/Reorder, the addonsLocked guard, and never pushing built-ins | Mock-server tests; owner sign-off on privacy-doc changes |
| **M13** | macOS embedded | Render-API NSOpenGLView port with points-based geometry; vendored `@rpath` dylibs with provenance; `otool` and `codesign` checks | macOS CI: `brew install mpv` for tests, `SKIP_LIBMPV_CHECK` for clippy |
| **M14** (optional) | Linux embedded Phase B | Only after a GtkGLArea + transparent WebKitGTK prototype succeeds; needs a 24.04 or Debian 12 build host or a `libloading` binding | — |

**Parallelism:**
- After M0, two tracks can run side by side: track A (M1→M2→M3→M4→M6→M7) and track B (M8→M9A→M10).
- The M5 policy gate must land before M6 and before Kitsu rows in M8.
- M11→M12 come after M4 and M10A. M13 comes after M3.
- To avoid conflicts, give each shared file one owner: `state.rs`, `lib.rs` handler blocks, `view.tsx`, `App.tsx`, `Settings.tsx`, `types.ts`, `Cargo.lock`.