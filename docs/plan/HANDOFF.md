# Feature plan handoff

How to pick up the feature plan locally. Written when the cloud session handed over on
2026-10-16, and updated after M4a, M4b, M5 and M6 were finished locally and released as
v0.2.0 the same day.

## Where things stand

- **Released:** v0.2.0, with every milestone below (M0 to M6): Zeiler's own player screen,
  built-in playback on Windows, the library with Continue Watching, the Kitsu and
  OpenSubtitles built-ins, and subtitles and audio controls. Before it, v0.1.3 was the first
  release named Zeiler.
- **Merged through:** PR #4 (M0 to M4a), #5 (M4b), #6 (M5) and #7 (M6 and the v0.2.0
  version bump). Everything below is on main.
- **Since v0.2.0, released as v0.2.1:** #8/#11 and #12 (another file opened in external
  mpv no longer gets the title's subtitles, and a subtitle add fails if the file changed
  meanwhile), #9 (libmpv from Zeiler's own mirror, with its sources), #10 (dependency
  licences checked in CI and their notices in every bundle) and #14 (a release is a draft
  until all four builds are in it).

| Commit | Milestone | What it did |
|---|---|---|
| `445d7c3` | M0 | Foundations: `persist.rs` JsonStore, `builtin.rs` registry with a no-stream test, per-key lenient settings, `settings_patch` + `settings://changed`, IpcError kinds, reserved view names |
| `d213ced` | M1 | Every playback opens Zeiler's own Player screen (one-shot handoff id in the hash), and the app stays mounted underneath |
| `a6c0356` | M2a | Backend-neutral player API (`player_start/load/stop/set_property/command/get_property/backends`), external mpv over JSON IPC, allow-list validation |
| `7838aae` | M2b | Full remote controls for external mpv; Auto / Built-in / External choice in Settings |
| `21887d4` | M3 | Built-in (embedded libmpv) playback is the Windows default; CI covers it; the Windows installer artifact; licensing |
| `1bf37c9` | M4a | `crates/zeiler-account` (stremio-core-compatible LibraryItem, watched bitfield, progress rules, video order, sorts), `library.json` under `profile_dir()`, `library_*` commands + `library://changed`, progress tracked in Rust from player events via `player_start`'s `context`; three review rounds with adversarial verification |
| `d085e4c` | M4b | The library on screen (`src/lib/library.tsx` provider and hooks, re-read on `library://changed`): My Library tabs (Continue Watching / Library / History, the six sorts), a Continue Watching row on Home with dismiss, Detail's Add to Library and watched marks (title, season, episode; only released episodes), Resume / Continue on Play, a "Resume from 12:34" prompt in the stream picker, progress bars on posters; two reviews plus a check of their fixes |
| `02ffc1b` | M5 | Built-ins: Anime Kitsu (metadata) and OpenSubtitles v3 (subtitles) snapshots next to Cinemeta, `scripts/refresh-builtin-manifests.mjs`, each built-in trimmed to its role (Kitsu's own `subtitles`, which redirect to OSv3, are dropped) and streams embedded in a built-in's meta stripped; dedupe by URL or snapshot manifest id; `builtinKitsu`/`builtinOpensubtitles` (an older settings file inherits `builtinCinemeta`, Kitsu also off with `hideAnime`) with Settings → Content switches and disclosures; `find_meta` retries `kitsu:`/`mal:`/`anilist:`/`anidb:` ids as series/movie/anime without re-asking an unreachable addon; policy text now "metadata or subtitles only, switchable, never streams"; two reviews with adversarial verification |
| M7 PR | M7 | Skip and next episode. `crates/zeiler-skip`: AniSkip for `kitsu:`/`mal:` ids (Kitsu's mappings turn a Kitsu id into MyAnimeList's), TheIntroDB for `tt…`/`tmdb:` ids, open starts/ends and clamping, LRU caches with negatives, 8 s / 1 MiB, never fatal; API shapes checked live on 2026-10-16 and recorded as fixtures; loopback-server tests. `skip_segments_get` behind `skip.onlineLookup`; the file's chapter names as the fallback (`src/lib/skip.ts`); synthetic 90 s credits with a next episode. Skip pill (Enter), Up Next card (last `upNextSeconds`, countdown, close cancels autoplay), "Skipped intro · Undo" toast; auto-skip off by default, once per segment per file, only on entering by playing, never the synthetic credits; credits auto-skip plays the next episode. External mpv: `player_osd_prompt` shows the offer in mpv's window and binds Enter in a forced input section (`script-message zeiler-skip` → `client-message`). Next episode by stremio-core's order (`src/lib/next-video.ts`, same rules as `next_video_id`), a same-addon same-`bingeGroup` stream prefetched in the last 90 s or at the credits, loaded with `player_load` (`loadNext` in `usePlaybackSession`: a new run, the new launch for the subtitle hook — which asks the addons again with the new stream's hints — progress context, torrent ownership); no match leaves the player and opens the next episode's picker (`requestPicker`, App replaces the picker frame). n / p and the media keys. Settings: `skip{}`, `bingeWatching`, `upNextSeconds` (Player & quality, with the privacy disclosure). Two reviews with adversarial refuters: seven findings confirmed and three plausible ones fixed (one next-episode load at a time, the old file's end of file no longer interrupting it, a failed resolve leaving the episode playing with a notice, every torrent the session resolves removed once, Close cancelling a pending autoplay, credits auto-skip waiting for the next episode's stream, late online segments at 0:00, ordered mpv prompts, lenient AniSkip/TheIntroDB parsing) |
| `1d1fd8f` | M6 | Subtitles and audio. `subtitles_get` fan-out with the stream's hash, size and file name. `zeiler-addon/src/subfile.rs`: http(s) only, 10 MiB cap also after gunzip, archives and web pages refused, BOM / UTF-8 / `chardetng` decoding, and only text that *starts* as SRT, WebVTT, ASS or MicroDVD (a saved file is local to mpv, which would follow an EDL's or CUE sheet's paths). Files go to `<cache_dir>/subs/` and are added with `player_subtitle_add`/`_reencode`/`_pick_local` (dialog plugin from Rust only) or a window drop, on both backends, never for a file that changed meanwhile. `sid`/`slang`/`alang` are set before load from the settings and the title's remembered choices (`player-prefs.json`, LRU 200). The UI ranks candidates and makes a language-first automatic pick (`src/lib/subtitles.ts`, `useTracks.ts`). Subtitle menu (sources, language rail, Off/On, inline errors, file, encoding, delay, quick style) and audio menu (tracks, delay); keys s, z/x, [/]. Settings → Subtitles & audio with nested `subtitles{}`/`audio{}` groups read per field, merge-patched and strict at any depth (`settings_groups.rs`). The OpenSubtitles disclosure now describes when it is asked. Two reviews with adversarial refuters; ten findings confirmed and fixed |
| M8 PR | M8 | Catalogs. `ManifestCatalog::default_extra`/`is_boardable`/`filter_props` (Cinemeta "New" becomes a row, "New · 2026"). `catalog_load` only for an addon in effect and a catalog and extras it declares, cached in memory (`catalog_cache.rs`, 15 min, 256 pages), and returns `{metas, fetched}` so paging counts the titles Rust filtered. One adult filter in Rust (`zeiler-addon/src/adult.rs`: `is_adult_text`, `is_adult_manifest`, `is_adult_meta`, `without_adult_options`; soft tags such as Ecchi hide a genre option but not a title) behind `showAdult` (Settings → Content, 18+ confirm): `addon_list` strips adult genre options and adult-named catalogs (Kitsu's Hentai, Ecchi, Yaoi, Yuri) and flags adult addons, `catalog_load` refuses them and drops titles with adult genres. `src/lib/rooms.ts` (Home: movies and series, then anime unless `hideAnime`, then every other type; anime catalogs by type, id prefixes or name go to the Anime room), genre spotlight rows per room, lazy rows (`useCatalogRows`: 4 eager, the rest when near, 6 requests at once and 2 per addon, stale ones never sent), Home row customization (`homeRows`, `home_rows.rs`; keys never hold transport URLs), the Browse frame (`src/views/Browse.tsx`, replaced Filter: type, catalog and every option-backed extra; choices kept in the frame), manifest-driven GenreTiles. Two reviews with an adversarial refuter: ten findings confirmed (four in part) and fixed, five refuted |
| M9-A PR | M9 (A) | Live TV from addons. A `live` room (`src/views/Live.tsx`) for the installed addons' `tv` and `channel` catalogs (`rooms.ts` `LIVE_TYPES`): lazy rows (wide cells for landscape posters), their genre extras as category tiles, Browse. The Sidebar's Live TV entry opens it; `hideLiveTv` hides the room and sends `tv` back to Shows. Home and Discover leave tv/channel to the room (Harbor's rule). A channel opens Detail, which already plays a live meta's default video; the launch is `live` for `isLive` metas and type `tv`. A room hidden in Settings while it is open (Live TV, Anime) goes back to Home. Nothing seeded. Two reviews, each refuting its own findings: nothing serious; the low ones (live types win over the anime heuristic, channel line-ups not deduplicated across rows, the hidden-room redirect, copy, dead Sidebar code) fixed |
| PR #21 | M13 | Built-in playback on macOS. libmpv's render API draws into an `NSOpenGLView` under the WKWebView (`commands/mpv/macos.rs`, a port of Harbor's `mpv_render_mac.rs`; the page's physical px become points, a new core takes the surface over from the one it replaces, the webview's `drawsBackground` goes back on when it is removed). `zeiler-player`'s `Surface::RenderApi(host)` with a `RenderHost` (`render.rs`): the render context is created before the first load and freed before `quit`, and holds the core so it can never be destroyed under it. libmpv, FFmpeg (LGPL, Secure Transport, VideoToolbox, dav1d), libplacebo and the libass stack are built from pinned sources (`scripts/build-libmpv-macos.sh`, macOS 12.0, `@rpath`, ad-hoc signed) in the `libmpv-macos` workflow, mirrored with their sources on the `libmpv-macos-0.41.0` pre-release, fetched by `fetch-libmpv.mjs`, bundled under `Contents/Frameworks`, and checked by `scripts/check-macos-app.sh` (release.yml and the workflow). macOS CI: clippy with `ZEILER_SKIP_LIBMPV_CHECK`, tests against Homebrew's libmpv. Two reviews with adversarial refuters; eleven findings confirmed and fixed (among them Swift objects built for the build machine's macOS); the rest are follow-ups below |

## Decisions already made by the owner

- **Player work:** this plan owns all of it (embedded, skip, subtitles). Embedded runs on
  Windows and, since M13, macOS. Linux keeps mpv in its own window, with full controls in
  Zeiler (M14 was prototyped and stopped, see below).
- **Marketplace:** lists all community addons, labelled "not reviewed by Zeiler". Adult
  addons are hidden by default. Every install still goes through the confirmation dialog.
- **Built-in addons:** metadata and subtitles only (Cinemeta, OpenSubtitles v3, an anime
  metadata addon). Zeiler never seeds a stream addon; a CI test enforces this.
- **Names and IDs:** the app is Zeiler, the bundle identifier is `io.zeiler.desktop`, and
  the repo is `malhaniah/zeiler`. Never create a new repo named `sailor`: v0.1.2 installs
  find updates through GitHub's rename redirect.

## The plan files

- `critique.md` is the reconciled plan and wins over the specs: milestone table in §4,
  conflict resolutions C1–C14 in §3, verified corrections in §2.
- `specs/*.json` are the per-area specs (player-embedded, player-ui, catalogs,
  account-profile, marketplace, conventions). Each has `implementation_plan` steps, risks
  and the Harbor files to adapt.
- Paths inside these files come from the cloud container. Read `/home/user/sailor` as your
  repo root. The Harbor reference (MIT) was a clone of https://github.com/harborstremio/harbor,
  also mirrored at `9000000/harbor`, v0.8.5. Clone it next to the repo if you want it.

## Remaining milestones (in order)

M7 (skip and next episode) is done; see its row above and "Skip and next episode" in
docs/ARCHITECTURE.md.

| # | Milestone | Scope (see critique.md §4 and the named spec) |
|---|---|---|
| M9 (B) | Live | M3U/XMLTV/Xtream, with credentials in a SecretStore and the `live` flag in the player (catalogs.json). Part A (the addon room) is done |
| M10 | Marketplace | URL normalization, replace and reorder, update detection, an opt-in community directory with a detail page and labels; stremio:// handler on Windows and Linux (macOS needs an owner decision) (marketplace.json) |
| M11 | Local profiles | `profiles.json`, migration by copy (not move), Argon2 PIN, avatars, a picker, per-profile addons, library and preferences |
| M12 | Stremio account | Link-code sign-in first, then password; keychain storage; library sync; addon sync with Add/Remove/Reorder; never push built-ins (account-profile.json) |
| M13 | macOS built-in playback | Done in PR #21 (see the table above) |
| M14 | Optional | Linux embedded: prototyped, not implemented. See "M14 prototype" below |

Left from M6 for later (the player-ui.json and critique.md items M6 did not take on):
- step 12 and critique §4: volume normalize (a labelled `af`), volume above 100%
  (`volume-max`), the stats overlay (key i), chapter ticks on the seek bar, an HLS
  quality menu;
- step 4(f), the spec's own "Phase 2": computing the OpenSubtitles hash when a stream
  does not give one;
- step 5: pre-adding unselected subtitles per language (`autoAddPerLang`). This was
  left out on purpose: Settings promises that a subtitle is downloaded only when it is
  shown. Also anime detection by genre, and stripping Japanese for titles that are not
  anime;
- step 6: a secondary subtitle (`secondary-sid`) picker, and the `c` key (`s` cycles);
- step 7: `subSource` in the per-title choices, and the style fields `fontSize` (the
  size is `scale`), `shadowColor`, `alignX` and `alignY`.

Also still open: the download website. The owner asked for OS detection, a downloads page and a
product tour. It is not started, and the domain is `zeiler.io`.

## How each milestone was built

Each milestone went through the same loop, and each step is worth keeping:

1. Implement against `critique.md` plus the area spec, and keep house conventions
   (`specs/conventions.json`, CONTRIBUTING.md).
2. Smoke-test the UI in a real browser: `npm run dev` plus Playwright with
   `window.__TAURI_INTERNALS__` mocked, and a screenshot of every flow. M4b did this
   without Playwright: a throwaway page in a git-ignored `smoke.local/` folder (`*.local`
   is ignored) loads a mock module before `/src/main.tsx`, which answers the commands with
   `mockIPC(handler, { shouldMockEvents: true })` from `@tauri-apps/api/mocks` and calls
   `emit` from `@tauri-apps/api/event` for `library://changed`.
3. Two independent reviews: correctness and behaviour preservation, then spec, security and
   cross-platform. Have someone try to refute each finding; fix only the confirmed ones.
4. Run every gate after the last edit, then commit one milestone per commit with the why in
   the body.

## Gates (run all before every commit)

On Windows, run `node scripts/fetch-libmpv.mjs` from PowerShell, not Git Bash: Git Bash's
GNU `tar` cannot read the .7z, while Windows' own `tar.exe` (bsdtar) can. Stop `npm run
dev` before `npm ci`: the dev server keeps native modules open and `npm ci` fails with
EPERM halfway, leaving `node_modules` broken. The embedded test needs `src-tauri\libmpv`
on `LIB` and `PATH`, as `ci.yml` sets them.

```bash
npm ci && npm run build          # tauri::generate_context! needs ../dist
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm run typecheck

# embedded player (needs libmpv: Linux `libmpv-dev`; Windows `node scripts/fetch-libmpv.mjs`)
cargo clippy -p zeiler-player --features embedded --all-targets --locked -- -D warnings
cargo test -p zeiler-player --features embedded --locked

# real-mpv integration tests (need mpv and ffmpeg on PATH; ignored in CI)
cargo test -p zeiler-player --test external_mpv --locked -- --ignored
```

On Linux you can also check Windows-only code without a Windows machine:

```bash
rustup target add x86_64-pc-windows-gnu    # plus apt install mingw-w64
cargo clippy --target x86_64-pc-windows-gnu --workspace --all-targets --locked -- -D warnings
ZEILER_SKIP_LIBMPV_CHECK=1 cargo clippy --target x86_64-pc-windows-gnu -p zeiler --features embedded --all-targets --locked -- -D warnings
```

Never run `cargo update` in a feature commit. The lockfile pins `rust-ini` 0.21.1 on
purpose; see the comment in `Cargo.toml`.

## M14 prototype (Linux embedded): stopped

The plan allowed M14 only if a GtkGLArea prototype worked first. It half works, so M14 was
not implemented. The prototype is on the branch `claude/m14-gtkglarea-prototype`
(`prototypes/linux-gl-overlay/`, with its workflow `m14-prototype.yml`; not for merging):
a GTK window with a `GtkOverlay`, a `GtkGLArea` under a `WebKitWebView` with a transparent
background, libmpv's render API into the GL area, a page that paints a red box and leaves
the rest transparent, and a solid green lavfi video. It runs in CI on ubuntu-24.04 (WebKitGTK
4.1, mpv 0.37, Xvfb, Mesa llvmpipe) and checks two pixels of a screenshot.

- **The composition works.** GL content in the `GtkGLArea` shows through the transparent
  page, and the page draws on top, in every run with WebKitGTK's default (accelerated)
  compositing: a plain GL clear came out blue under the red box each time.
- **mpv's frames do not, reliably.** With mpv rendering, about a quarter of identical runs
  showed the green video under the red box; the rest showed black from the very first
  frame to the last. mpv's log (VO configured, textures, FBO format, frame drops) and the
  GL state at the first render (context, framebuffer, scissor, blend, colour mask) are the
  same in green and black runs, and creating the render context at the first render
  instead of at realize did not help (0 of 8). With `WEBKIT_DISABLE_COMPOSITING_MODE=1` the
  page itself stopped painting.
- Found on the way: the GL loader must match GDK's context (GLX on X11, EGL on Wayland).
  WebKitGTK initializes EGL in the process, and asking EGL for a GLX context's functions
  gives entry points that draw nothing.
- **What could not be checked here** (macOS machine, Linux only in CI): a real GPU and
  driver (Mesa, NVIDIA), Wayland, a desktop compositor, vaapi interop. The black frames may
  be an llvmpipe/Xvfb artefact; Celluloid drives mpv the same way on real desktops.
- **Before implementing M14:** run the prototype on a real Linux desktop (X11 and Wayland,
  Mesa and NVIDIA) and find the black-frame race; and decide how Linux gets libmpv: the
  release builds on ubuntu-22.04, whose libmpv1 (client API 1.x) libmpv2 6.0 refuses, so
  M14 needs a newer build host (raising the glibc floor) or a `dlopen`-based binding that
  falls back to external mpv. Then the shell side: Tauri's webview moved into a
  `GtkOverlay` above a `GtkGLArea`, geometry in logical px, `LC_NUMERIC` after GTK init.

## Follow-ups from M13

- libmpv is a load-time dependency on macOS (Windows links it DELAYLOAD): if a bundled
  library ever failed to load, the app would not start, instead of falling back to external
  mpv. Weak-linking `libmpv` with a runtime probe would mirror Windows.
- mpv's `tls-verify` is off by default on every OS (it predates M13); with Secure Transport
  on macOS it could be turned on and use the system trust store.
- Secure Transport (FFmpeg's TLS on macOS) stops at TLS 1.2; a server that requires 1.3
  would need OpenSSL or mbedTLS in the bundle (FFmpeg would then become LGPL-3.0).

## Not yet verified on a real machine

- macOS built-in playback was watched through on an Apple Silicon Mac (macOS 27, dev
  build with the bundle built from source): Big Buck Bunny, 9:56, under the controls;
  fullscreen in and out, an error, Back, and a new video replacing a playing one. The
  release `.app` was built locally and passed `check-macos-app.sh`. Not run: the Intel
  build (CI built it, linked it and ran the tests on macos-15-intel), macOS 12 to 26 (the
  bundle targets 12.0, and the build fails on objects built for anything newer), a window
  resize by hand, and an external display with another scale factor.

- Library progress with a real stream in the app. It is unit-tested against a real
  libmpv event trace (recorded headless by `record_a_progress_trace` in
  `crates/zeiler-player/tests/embedded.rs`, replayed in `src-tauri/src/library/progress.rs`)
  and the Player's `context` was checked in the browser with mocked IPC, but nobody has
  watched an episode through and looked at `library.json`. External mpv was not installed
  on the machine that built M4a, so its event feed and the real-mpv tests
  (`--test external_mpv -- --ignored`) were not run there.
- Whether Stremio's own apps read Zeiler-written items as expected (needs M12 sync).
- The Library UI (M4b) against the real backend. It was smoke-tested in the browser
  against an imitation of the `library_*` commands, not in the app with a real
  `library.json`; mpv was not installed on the machine that built M4b either.

- Windows built-in playback at runtime: WebView2 transparency, re-pinning the video under
  the webview, focus hand-back, and Escape/fullscreen. CI linked the code and ran the
  headless libmpv tests on Windows, but nobody has watched a video yet. Try the
  `windows-installer` artifact from PR #4.
- M7 in the real app. The skip pill, Up Next card, auto-skip with Undo, the next episode
  by `player_load` (URL and torrent, the old torrent dropped) and the picker fallback
  were smoke-tested in the browser with mocked IPC and a simulated mpv, on both screens;
  the AniSkip, Kitsu and TheIntroDB answers were checked live with curl and replayed in
  tests, but the app has not asked them yet. The prompt's mpv commands were tried on mpv
  0.41 over its JSON IPC (Enter in the section came back as `client-message
  ["zeiler-skip"]`), not from the app; loading the next file after keep-open's end of
  file is a real-mpv test (`the_next_file_plays_after_the_end_of_the_last`). Not tried:
  a season pack's next file (same torrent, another `fileIdx`) through the engine.
- M6 subtitles in the real app. `sub-add` was run on the built-in core only, in the
  headless libmpv test (`subtitles_are_added_as_selected_external_tracks`); the menus,
  the automatic pick and the per-title choices were smoke-tested in the browser with
  mocked IPC; a real OpenSubtitles v3 subtitle was fanned out for, downloaded and decoded
  by `cargo test -p zeiler-addon --test live -- --ignored`. Not yet tried: `sub-add`,
  the style fallbacks and the Box background on external mpv (none was installed), the
  OS file dialog and drag-and-drop on any OS, and the Linux and macOS builds of the
  dialog plugin (CI will build them). Another video opened in external mpv's window
  (the hook then leaves the title's subtitles and choices out of it) was tried with
  mocked events only.
- The M5 built-ins in the real app: the Settings → Content switches, the Anime room and
  Kitsu details were smoke-tested in the browser with mocked IPC only, and the live Kitsu
  and OpenSubtitles responses only through `cargo test -p zeiler-addon --test live --
  --ignored`. mpv was not installed on the machine that built M5 either, so the real-mpv
  tests were not run there.
- M8 catalogs against real addons: rooms, Browse, lazy rows, Home customization and the
  adult switch were smoke-tested in the browser with mocked IPC (snapshot manifests, made-up
  pages); `catalog_load`'s checks, the cache and the adult filter by unit tests. Left for
  later: a catalog that turns out to be anime only after it loads leaves Movies and Shows
  but does not join the Anime room (it stays on Home and Discover); built-in manifests are
  still snapshots only (Cinemeta's "New" years end at the snapshot's year until it is
  refreshed); `force` (skip the catalog cache) has no UI yet.
- The Sailor → Zeiler NSIS hook (`src-tauri/windows/hooks.nsh`) runs only when a real
  Sailor v0.1.2 install updates.

## Releasing

1. Bump the version in `Cargo.toml`, `package.json` and both lockfiles (`npm version
   X --no-git-tag-version`, `cargo update --workspace`).
2. Merge to `main`.
3. Push a `vX.Y.Z` tag, or run `release.yml` by hand on `main`. Its `draft` job creates
   the version's release as a draft, the four builds upload into it, and `publish` makes
   it public (and creates the tag) only once every build succeeded and `latest.json` has
   all ten platform entries. Until then `releases/latest`, where the updater looks, stays
   on the previous release.
4. If a build fails, the release stays a draft: re-run the failed jobs, and `publish`
   runs again after them. If you bump the version instead, delete the stale draft by
   hand. To seed a release with a locally built installer, seed it as a draft. If the
   bundle targets change, update the list of platform keys in `publish` too.

Keep the same updater signing key (`TAURI_SIGNING_PRIVATE_KEY`); rotating it would cut
every installed copy off from updates.
