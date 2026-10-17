# Feature plan handoff

How to pick up the feature plan locally. Written when the cloud session handed over on
2026-10-16, updated after M4a, M4b, M5 and M6 were finished locally and released as
v0.2.0 the same day, and again on 2026-10-17 when M7 to M13 were merged for v0.3.0.

## Where things stand

- **Released:** v0.2.1 (2026-10-16): M0 to M6, Zeiler's own player screen, built-in
  playback on Windows, the library with Continue Watching, the Kitsu and OpenSubtitles
  built-ins, subtitles and audio controls, plus the fixes in #8 to #14 (subtitles kept
  off another file opened in external mpv, libmpv from Zeiler's own mirror, dependency
  licences checked in CI, a release is a draft until all four builds are in it).
- **On main, releasing as v0.3.0:** ten PRs merged on 2026-10-17, in this order: #16
  (the zeiler.io download website, `website/`), #17 (M8 catalogs), #20 (M9 A, Live TV
  from addons), #18 (M7 skip and next episode), #22 (M6 leftovers), #19 (M10
  marketplace), #23 (M11 local profiles), #25 (M12 Stremio account sync), #24 (M9 B, user
  IPTV), #21 (M13 built-in playback on macOS; M14 Linux prototyped and stopped), then #26
  (security policy, README rewrite, issue and PR templates). Every milestone of the plan
  is built except the optional M14.
- **Before releasing:** the HANDOFF gates passed on the merged tree on a Mac (including
  the macOS embedded and real-mpv tests); CI is green on main. An integration review
  found no code defect that survived verification; its unverified and low findings are
  listed under "Open findings from the v0.3.0 review" below.

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
| M6+ PR | M6 leftovers | Volume normalization (`player_set_audio_normalize`: a fixed `dynaudnorm` under the `@zeiler-normalize` label, so the user's own `af` stays; `audio.normalize`, applied at every start and load), volume above 100% (`audio.volumeMax`, 100–200, default 130 = mpv's own rather than the spec's 200, which would amplify for everyone; sent as the pre-load `volume-max` only when not 130, so external mpv keeps its own; the slider and ↑ follow it), the stats overlay (key i, `StatsOverlay.tsx`, the read allow-list once a second), chapter ticks on the seek bar with a hover time and chapter title, an HLS quality menu (`QualityMenu.tsx`: only with more than one video track, `vid`). The OpenSubtitles hash when the stream gives none (`zeiler-addon/src/videohash.rs`: two range requests against the stream itself, basic auth from the engine URL's userinfo; `player_video_hash` reads the request Rust remembered, so the URL never comes back through the webview, only for the running session, and refuses a file that changed meanwhile; the subtitle hook asks the addons once the file loaded, without a hash after 20 s). A second subtitle (`secondary-sid` picker in the Subtitles menu, key c). `subSource` in the per-title choices (`embedded`/`stream`/`addon`; the next episode's automatic pick prefers that source in the remembered language). Style fields `fontSize`, `shadowColor` (as a Shadow choice with `shadowOffset`; mpv reads it as `sub-back-color`, so it never replaces a box's colour), `alignX`, `alignY`, in Settings with the preview. Two reviews with adversarial refuters: eleven findings confirmed and fixed (the hash of a stopped session's file, the second subtitle counted as the main one, `shadowColor` overwriting the box colour, the URL in a hash error log, external mpv's own `volume-max`, a remembered addon source not waited for, normalize at every load, one 15 s bound for the hash, hash and size from the same file) |
| `1d1fd8f` | M6 | Subtitles and audio. `subtitles_get` fan-out with the stream's hash, size and file name. `zeiler-addon/src/subfile.rs`: http(s) only, 10 MiB cap also after gunzip, archives and web pages refused, BOM / UTF-8 / `chardetng` decoding, and only text that *starts* as SRT, WebVTT, ASS or MicroDVD (a saved file is local to mpv, which would follow an EDL's or CUE sheet's paths). Files go to `<cache_dir>/subs/` and are added with `player_subtitle_add`/`_reencode`/`_pick_local` (dialog plugin from Rust only) or a window drop, on both backends, never for a file that changed meanwhile. `sid`/`slang`/`alang` are set before load from the settings and the title's remembered choices (`player-prefs.json`, LRU 200). The UI ranks candidates and makes a language-first automatic pick (`src/lib/subtitles.ts`, `useTracks.ts`). Subtitle menu (sources, language rail, Off/On, inline errors, file, encoding, delay, quick style) and audio menu (tracks, delay); keys s, z/x, [/]. Settings → Subtitles & audio with nested `subtitles{}`/`audio{}` groups read per field, merge-patched and strict at any depth (`settings_groups.rs`). The OpenSubtitles disclosure now describes when it is asked. Two reviews with adversarial refuters; ten findings confirmed and fixed |
| M8 PR | M8 | Catalogs. `ManifestCatalog::default_extra`/`is_boardable`/`filter_props` (Cinemeta "New" becomes a row, "New · 2026"). `catalog_load` only for an addon in effect and a catalog and extras it declares, cached in memory (`catalog_cache.rs`, 15 min, 256 pages), and returns `{metas, fetched}` so paging counts the titles Rust filtered. One adult filter in Rust (`zeiler-addon/src/adult.rs`: `is_adult_text`, `is_adult_manifest`, `is_adult_meta`, `without_adult_options`; soft tags such as Ecchi hide a genre option but not a title) behind `showAdult` (Settings → Content, 18+ confirm): `addon_list` strips adult genre options and adult-named catalogs (Kitsu's Hentai, Ecchi, Yaoi, Yuri) and flags adult addons, `catalog_load` refuses them and drops titles with adult genres. `src/lib/rooms.ts` (Home: movies and series, then anime unless `hideAnime`, then every other type; anime catalogs by type, id prefixes or name go to the Anime room), genre spotlight rows per room, lazy rows (`useCatalogRows`: 4 eager, the rest when near, 6 requests at once and 2 per addon, stale ones never sent), Home row customization (`homeRows`, `home_rows.rs`; keys never hold transport URLs), the Browse frame (`src/views/Browse.tsx`, replaced Filter: type, catalog and every option-backed extra; choices kept in the frame), manifest-driven GenreTiles. Two reviews with an adversarial refuter: ten findings confirmed (four in part) and fixed, five refuted |
| M9-A PR | M9 (A) | Live TV from addons. A `live` room (`src/views/Live.tsx`) for the installed addons' `tv` and `channel` catalogs (`rooms.ts` `LIVE_TYPES`): lazy rows (wide cells for landscape posters), their genre extras as category tiles, Browse. The Sidebar's Live TV entry opens it; `hideLiveTv` hides the room and sends `tv` back to Shows. Home and Discover leave tv/channel to the room (Harbor's rule). A channel opens Detail, which already plays a live meta's default video; the launch is `live` for `isLive` metas and type `tv`. A room hidden in Settings while it is open (Live TV, Anime) goes back to Home. Nothing seeded. Two reviews, each refuting its own findings: nothing serious; the low ones (live types win over the anime heuristic, channel line-ups not deduplicated across rows, the hidden-room redirect, copy, dead Sidebar code) fixed |
| (M10 PR) | M10 | Marketplace. `normalize_transport_input` takes setup-page links (`/configure`, `#/configure`), a trailing `/` and a missing `/manifest.json`. `addon_preview` reports the installed match (same URL / same id / same host) and the setup page, and compares the live manifest with the directory listing it came from; `addon_add { replace }` swaps a reconfigured copy in place; `addon_reorder` (an exact permutation); `addon_check_updates` (six at a time, 10 s, reused 6 h) with a capability diff (`zeiler-addon/src/changes.rs`: streams, P2P, adult, setup or a new id need review) and `addon_refresh` for one-click cosmetic updates. Opt-in directory (`marketplaceEnabled`, off): `zeiler-addon/src/directory.rs` (stremio-addons.net, all pages, and Cinemeta's `addon_catalog` community/official, merged by manifest id, remote flags dropped), `commands/marketplace.rs` (index cached 1 h in memory and `<cache>/marketplace-index.json`, a stale index kept when nothing answers, errors name the directory only), Addons → Installed / Built-in / Browse tabs, the `addon-detail` frame (facts, text-only documentation), "not reviewed by Zeiler" on every listing. Adult: M8's `zeiler_addon::is_adult_manifest` plus the `nsfw` category, hidden unless `showAdult` (M8's 18+ switch; the Browse tab asks too); `hideP2pAddons`. Every install, from any source, goes through one confirmation dialog (`InstallHost`; P2P needs a tick). Opt-in `stremio://` handler (`deeplink.rs`): runtime registration on Windows and Linux, argv from the single-instance plugin, never an install; macOS left out (owner decision). Relative manifest logos resolve against the transport URL (live and listed); non-http(s) ones are dropped. Two reviews with adversarial refuters: 11 findings confirmed and fixed (update-check lock held across the network, stale directories served after both sources were switched off, adult listings leaking through `nsfw=exclude`, keyed URLs in HTTP error text, Windows unregister deleting a Stremio app's handler, among others) |
| (M11 PR) | M11 | Local profiles. `profiles.json` plus `profiles/<id>/` folders (addons, library, per-title choices, `prefs.json`, avatar); first run copies (never moves) the per-user files and the profile keys of `settings.json` into the primary profile; a damaged `profiles.json` adopts the folders on disk. `settings_get` still returns device settings merged with the active profile's keys (`PROFILE_KEYS` in `profiles.rs`, which already names M7's `skip`, `bingeWatching`, `upNextSeconds` and M8's `homeRows`). Switching swaps addons, settings, library and player choices in place (`JsonStore::switch_to`), refused while a player runs; the UI remounts its shell keyed by the profile. Argon2id PIN (4–8 digits, backoff after three wrong tries; a convenience lock, files not encrypted), avatars checked by magic bytes (512 KB), "Who's watching?" picker at launch with several profiles or a PIN (a locked profile mounts nothing until its PIN), sidebar profile chip, Settings → Profiles & account. Two reviews with an adversarial refuter; the confirmed findings fixed (a profile stayed unlocked after switching away, the launch picker wasn't modal, a failed switch could be half-applied, addon writes could race a switch, a failed first-run copy was never retried, the update cache crossed profiles, among others) |
| (M12 PR) | M12 | Stremio account, per profile and opt-in. New crate `crates/zeiler-sync` (the account API client: login, loginWithToken, link codes, getUser, logout, addonCollectionGet/Set, datastoreMeta/Get/Put; HTTPS only, no redirects, URL-free errors; plus the pure rules: stremio-core's library plan and merge, `rebase_addon_op` for Add/Remove/Reorder, never pushing a built-in) tested only against mock servers and recorded shapes. `src-tauri/src/account/`: the session key in the OS keychain via `keyring` 3.6 (memory when not remembered or when the keychain fails, never a file), `account.json` per profile, background sync (launch, 15 min, a minute after local changes, Sync now), addon pushes after every add/remove/replace/reorder/refresh, `addonsLocked` after a failed pull. UI: Settings → Profiles & account → Stremio account (browser sign-in with code and polling, then email and password; the sign-in's addon question; sync switches; privacy text). CI and release install `libdbus-1-dev` for the Secret Service backend. Also changes M11: PIN buttons show only on the active profile (the PIN is set there). Two reviews with an adversarial refuter: all 16 findings confirmed (3 in part) and fixed or listed below (a lock-order deadlock, the link poll restarting on re-render, pulls undoing unsent changes, another account's library uploaded, protected/loopback entries imported, keychain session left after deleting a profile, among others) |
| M9-B PR | M9 (B) | User IPTV. `crates/zeiler-iptv`: M3U (`#EXTINF` attributes, `#EXTGRP`, `#EXTVLCOPT` user agent and referrer, `url-tvg`, divider rows dropped, stable non-reversible channel ids), Xtream Codes (`player_api.php`, `/live/u/p/id.m3u8`, `xmltv.php`) and XMLTV (quick-xml stream, gzip by magic bytes, only now−2 h…now+24 h kept), with size caps (80 MB playlists, 100 MB guides) and errors that never quote a URL. `src-tauri/src/secrets.rs`: IPTV's secrets on M12's keychain store (`account/secrets.rs` `KeyringStore`, UTF-8 bytes) with a memory fallback, never plaintext on disk; keyring calls run off the async runtime. `iptv.json` is per profile: `profile_select` switches it and clears the IPTV caches. `iptv.rs` + `commands/iptv.rs`: `iptv.json` under the profile dir holds no secrets; channels cached 6 h, guides 1 h; `iptv_resolve` hands a channel's stream to the one-shot launch, marked `live`; https only (also across redirects) unless the user accepted plain http for that source (`allowHttp`, asked in the add form). CodeQL's checks drove that and the test fixtures' run-time passwords. Live room tabs: Addon channels and Your playlists (`LivePlaylists.tsx`: sources, add form for M3U or Xtream, groups, search, favourites, now/next). The player's `live` flag (`src/pages/player/live.ts`, read by `ControlsBar`): no seek bar, time, speed menu or seek/speed keys; a LIVE badge; no stream URL on the error card). Also: the player now logs a URL as scheme and host only (`redact_url`; Xtream puts the account in the path) and external mpv runs `--really-quiet` (it printed "Playing: <url>"). Two reviews, each refuting its own findings; confirmed and fixed: guide downloads piling up (one per source at a time, failures cached 5 min, UI debounced), the account in logs and in the Referer on redirects, the guide's global cap (now only the source's channels, 96 per channel), non-UTF-8 guides (decoded by encoding_rs), programmes without `stop`, titles with commas, channel ids that changed with URL tokens, real channels taken for dividers, a removed source re-cached, `hasEpg` frozen at add, keychain secrets as UTF-16, Debug of inputs with accounts; a check of the fixes found six more (mpv's fallback launcher, Refresh keeping a stale guide, a failed channel reload retried per request, a removal racing the cache, unbalanced quotes, UTF-16 declared without a BOM), also fixed |
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
| ~~M10~~ | Marketplace | Done (see above). Open: macOS `stremio://` needs an owner decision (a static scheme in tauri.conf.json claims it at all times; runtime registration is unsupported there) |
| ~~M11~~ | Local profiles | Done (see above) |
| ~~M12~~ | Stremio account | Done (see above). Left for later (account-profile.json): playback progress pushed every 180 s while playing (it goes with the next sync), an exit flush, `getUser` at startup (email and premium refreshed only at sign-in), sync on window focus, `logout { removeSyncedData }` (sign-out keeps the library), shared sessions between profiles (`shareWith`), sign-in from the sidebar chip, a sign-in nudge in the Library, a "From your Stremio account" badge and greyed-out account entries Zeiler can't use (they are simply not imported), an ignored live test with a test account, and keeping queued addon changes across a restart. **Owner sign-off needed** (critique §4 M12) on the new privacy text in README ("Privacy") and ARCHITECTURE ("Account sync", security posture) |
| M13 | macOS built-in playback | Done in PR #21 (see the table above) |
| M14 | Optional | Linux embedded: prototyped, not implemented. See "M14 prototype" below |

The M6 leftovers (player-ui.json steps 4(f), 6, 7 and 12) are done; see their row
above. Still left from M6, on purpose: pre-adding unselected subtitles per language
(`autoAddPerLang`), because Settings promises that a subtitle is downloaded only when it
is shown; and anime detection by genre, and stripping Japanese for titles that are not
anime.

The download website (OS detection, a downloads page, a product tour) shipped in #16 as
`website/`, with its own CI job (`website.yml`). The domain is `zeiler.io`; hosting is the
owner's (a `CNAME` for GitHub Pages is on main).

## Open findings from the v0.3.0 review

An integration review of the merged tree (six lenses, each finding attacked by refuters)
left these for a follow-up. None was confirmed as a shipped defect; the first two deserve
a decision:

- IPTV: `iptv_resolve` (`src-tauri/src/commands/iptv.rs`) plays a channel's `http://`
  stream URL without the source's `allowHttp` consent; the consent gate covers only the
  playlist, EPG and Xtream account URLs. Xtream stream URLs carry the account in the path.
  Decide whether an unconsented http stream is refused (then a source whose playlist is
  https but whose streams are http needs a way to consent) or documented as allowed.
- Hide Live TV (`settings.hideLiveTv`, `src/App.tsx`) also hides the user's own IPTV
  playlists, while the setting's text only promises that addon catalogs move.
- Low: `profile_select`/`account_logout` wait on `sync_lock` behind a running sync; a
  profile switch does not kick a sync for the new profile; sign-in resolves the active
  profile after the network round trip; IPTV `storage_of` reports "os" for a memory-kept
  secret after a switch; a keychain read error reads as "no entry"; the skip client and
  the OpenSubtitles hasher are not https-only and follow redirects with reqwest's
  defaults; `profile_delete` forgets a Stremio session without revoking it; playlist
  `#EXTVLCOPT` user agent and referrer skip the length checks of the source-level ones.
- Docs: `docs/ARCHITECTURE.md` lists shipped work as future, omits four events and
  three crates, and documents `catalog_load` as `{ metas }` (it returns `{ metas, fetched }`);
  the macOS `THIRD-PARTY-LICENSES.txt` lacks the libmpv cross-reference paragraph; the
  macOS library and signature check in `release.yml` runs after tauri-action has
  uploaded, so a failure leaves a draft rather than preventing the upload.

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

# embedded player (needs libmpv: Linux `libmpv-dev`; Windows `node scripts/fetch-libmpv.mjs`;
# macOS `brew install mpv` with LIBRARY_PATH=$(brew --prefix)/lib for the tests, and
# `node scripts/fetch-libmpv.mjs` for the bundled dylibs, see CONTRIBUTING.md)
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

- **M12 against the real Stremio API: never run.** No real account was signed in to and
  no real credentials were entered while building it. The client is tested against
  local mock servers with request shapes taken from stremio-core, and the link-code
  endpoints' query format (`type=Create`, `type=Read&code=`) and the `qrcode` field are
  inferred from stremio-core's encoding. Before release, the owner should sign in once
  with a test account (both the browser code and the password), check that Stremio's
  own apps read the library items Zeiler writes, and try addon sync both ways. The
  keychain backends (macOS Keychain, Windows Credential Manager, Linux Secret Service)
  were not exercised either: tests use the in-memory store.

- M11 profiles in the real app: the picker, PIN entry, switching and the settings were
  smoke-tested in the browser with mocked IPC; the migration, the switch, the PIN rules
  and the prefs overlay are unit-tested against temp dirs. Not yet tried: the first-run
  migration on a real v0.2.x data dir, and switching with a real library and player
  history.

- M10 in the real app: the Addons tabs, the directory, the install confirmation and the
  settings were smoke-tested in the browser with mocked IPC; the directory parsers
  against live stremio-addons.net and Cinemeta answers with `cargo test -p zeiler-addon
  --test live -- --ignored` (2026-10-16). Not yet tried: `stremio://` registration and
  links on Windows and Linux (CI builds them), opening a setup page with the opener,
  and reading the clipboard from the webview.
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
  the webview, focus hand-back, and Escape/fullscreen. The owner has now watched v0.3.0
  on Windows and found fullscreen from a maximized window stopping short by the
  taskbar (tao 0.37.1 trims a maximized undecorated window to the work area even when
  fullscreen); `fix_fullscreen_client_area` (commands/mpv/windows.rs) subclasses the
  main window to give a fullscreen window its whole rect, as tauri-apps/tao#1353 does
  upstream. Not yet tried on Windows; drop it when Zeiler gets a tao with #1353.
- The M6 leftovers in the real app. Menus, keys, the stats overlay, the Settings fields
  and the hash's use were smoke-tested in the browser with mocked IPC; the hash itself
  (two range requests, basic auth from the URL) against a loopback server in tests; the
  labelled `af` add/remove, `volume-max` and the second subtitle's `selected`/
  `main-selection` flags on mpv 0.41 over its JSON IPC. Not tried: the hash against the
  engine's real loopback stream or a debrid link, an actual HLS stream's variants in the
  Quality menu, and the shadow and alignment fields on screen.
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
- M9 IPTV against a real provider: the parsers are unit-tested with fixtures and the
  Playlists tab was smoke-tested with mocked IPC (add, groups, favourites, play hands a
  live launch to the Player; the hash carries only the id). Not tried: a real M3U, an
  Xtream server, a large gzipped XMLTV guide, the OS keychain on each OS (macOS may
  prompt for an unsigned build; a Linux box without a Secret Service daemon falls back
  to memory; Windows Credential Manager holds about 2,500 bytes per secret, so a very
  long playlist plus guide address falls back to memory too), and the Player's LIVE
  controls with a real stream (the mocked player never got past loading). Left for
  later: the EPG grid, catch-up, top-network rows, Replay reloading a live stream.
- The Sailor → Zeiler NSIS hook (`src-tauri/windows/hooks.nsh`) runs only when a real
  Sailor v0.1.2 install updates.
- The window's size, position and maximized state across launches (`window_state.rs`,
  replacing tauri-plugin-window-state, which never tracked the main window). Checked on
  the Apple Silicon Mac in a dev build with built-in playback: moved, resized, maximized
  and made fullscreen in the playing player through the window calls the app makes
  (from a temporary test hook, as the shell had no Accessibility access), and quit the
  way Cmd+Q does (`terminate:`) or by closing the window. It came back at the same
  place and size; maximized, with its smaller size to restore down to; windowed after a
  quit in fullscreen; and centered when its place was off every monitor. Not tried:
  Windows (an Aero Snap maximize, and fullscreen from a window restored maximized with
  #33's client-area fix), Linux, and two monitors with different scale factors.

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
