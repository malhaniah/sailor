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
| Playback | mpv, two backends behind one session | Embedded libmpv (`libmpv2` 6.x crate; video under the transparent webview — Windows `wid`+HWND_BOTTOM, macOS the render API into an NSOpenGLView; the default on Windows and macOS releases) or the user's own `mpv` in its own window, remote-controlled over mpv's JSON IPC (Linux, and anywhere embedded is unavailable). libmpv is a link-time dependency, so the embedded backend is the `embedded` cargo feature: Windows and macOS release builds turn it on and ship a hash-pinned LGPL libmpv (`scripts/fetch-libmpv.mjs`: `libmpv-2.dll` on Windows, dylibs built from pinned sources in `Contents/Frameworks` on macOS); plain dev builds leave it off. See "Player backends". |
| Content neutrality | No seeded stream addons; built-ins are metadata or subtitles only, switchable, never streams | Unlike Harbor (which also seeds Torrentio), Zeiler builds in only metadata and subtitle addons — Cinemeta and Anime Kitsu (metadata) and OpenSubtitles v3 (subtitles) — from manifest snapshots compiled into the app (`src-tauri/builtin/`), each switchable in Settings → Content. A test fails the build if a built-in declares a `stream` resource. Stream addons are installed only by the user, behind a confirmation dialog showing the live manifest (name, version, resources, p2p/adult hints): from a pasted URL, an opt-in `stremio://` link, or the opt-in addon directory. |
| Addon directory | Opt-in browsing of third-party listings (stremio-addons.net, Stremio's `addon_catalog`) | Zeiler curates, ranks and seeds nothing: no featured rails, no recommendations, no bundled list; the only orderings are the directory's own stars, listing date, or name. Every listing is labelled "not reviewed by Zeiler"; adult listings are hidden unless `showAdult`; every install goes through the confirmation dialog, which fetches the live manifest and shows where it differs from the listing. Nothing is fetched before `marketplaceEnabled`. |
| Stremio account | Opt-in, per profile; link-code sign-in first, password second; the key in the OS keychain | Off until a profile signs in. Hand-rolled client of the account API (`zeiler-sync`, stremio-core's request shapes, an honest `Zeiler/<version>` user agent); never the telemetry or premium endpoints (events, seekLog, getSkipGaps, …). See "Account sync". |
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
    zeiler-iptv/          # user IPTV: M3U and Xtream channel lists, XMLTV guides
    zeiler-player/        # player session: external mpv over JSON IPC; embedded libmpv (feature)
    zeiler-skip/          # skip segments (intro/recap/credits) from AniSkip and TheIntroDB
    zeiler-sync/          # opt-in Stremio account: API client + library/addon sync rules
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
  - *Embedded* (Windows and macOS releases: `embedded` feature + libmpv present): mpv renders
    under the transparent webview, with the controls drawn as auto-hiding overlays
    (`PlayerChrome.tsx`); `f` makes the Zeiler window fullscreen, Escape leaves
    fullscreen first (and the player only when not fullscreen), and leaving the
    player screen any other way ends fullscreen too. After a fullscreen
    toggle and on every window resize, move and scale change the page re-sends the
    video rect and calls `player_surface_refresh` on a kick schedule (0, 60, 200,
    500, 1000, 2000 ms; `surface-sync.ts`).
  - *External mpv* (Linux, Windows and macOS dev builds without the feature, a
    fallback, or a forced launch): mpv plays in its own window and the page is its full remote
    control (`ExternalRemote.tsx`): the title's art fills the window, dimmed; title,
    episode and source at the top; the same `ControlsBar` as embedded docked at the
    bottom — play/pause, seek bar with elapsed/total time and the buffered range
    (`demuxer-cache-time`), volume and mute, the audio and subtitle menus (see
    "Subtitles and audio"), speed, fullscreen of the mpv window
    (`cycle fullscreen`) and Stop. Shortcuts while Zeiler has focus: Space, ←/→ (5 s),
    ↑/↓ (volume 5), m, f, s (next subtitle), z/x (subtitle delay −/+0.1 s), [ / ]
    (slower/faster), Escape (back). When the user closes mpv (`shutdown`) the page
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
  `TorrentStats.tsx`, `useTracks.ts` (subtitles and audio), `SubtitleMenu.tsx`,
  `AudioMenu.tsx`, `menu.tsx` (the popup and its rows). Later player features add files
  here.

## Subtitles and audio

Every subtitle reaches mpv as a UTF-8 file Zeiler wrote itself; the webview can never
run `sub-add` (see "Validation policy").

- **Sources.** Embedded tracks (`track-list`), the subtitles a stream lists, every
  subtitle addon (`subtitles_get`: the built-in OpenSubtitles v3 while it is on, and any
  the user installed; Kitsu as a built-in declares none), a file the user picks
  (`player_subtitle_pick_local`: the OS dialog of `tauri-plugin-dialog`, opened from
  Rust; no dialog permission is granted to the webview) or drops on the window (Rust
  handles the drop while a player runs, extensions srt/ass/ssa/vtt/sub, and emits
  `subtitles://dropped`). The addons are asked with the stream's `videoHash`,
  `videoSize` and `filename` while the video opens when a subtitle will show by itself
  (automatic subtitles on, or a subtitle remembered for the title, and the title not
  remembered as "off"), otherwise only when the Subtitles menu first opens (Settings
  says so).
- **Files** (`crates/zeiler-addon/src/subfile.rs`, `src-tauri/src/subtitles/`). A link
  is http(s) only, followed through redirects, capped at 10 MiB (and a gzip file at 10
  MiB unpacked), 20 s. Gzip is unpacked by its magic bytes; ZIP, RAR and 7z archives,
  web pages and binary data are refused with a reason the menu shows. A byte-order mark
  decides the encoding, else the menu's encoding choice, else valid UTF-8 (a stray
  invalid byte does not count against it), else `chardetng`'s guess. The text must
  *start* as a subtitle — `WEBVTT`, an `[Script Info]` header (ASS/SSA), a MicroDVD
  frame line, or a SubRip cue (number and timing) — or it is refused: mpv reads a file
  by its content, and a file Zeiler saved is local to mpv, so an mpv EDL, a CUE sheet or
  a playlist served as a "subtitle" would make mpv open the local files or network shares
  it names (mpv refuses that for content it fetched itself). The result goes to
  `<cache_dir>/subs/<random>.<ext>`, emptied at startup and whenever a session starts or
  stops, and is added with `sub-add <path> select|auto <title> <lang>` on whichever
  backend plays; the title is flattened to one line of at most 120 characters and the
  language must be a language code. An add that finishes after the file changed is not
  applied (the shell counts loads). The UI finds the new track by the returned file
  name in `external-filename`; "read as another encoding" adds the same subtitle again
  (`player_subtitle_reencode`) and removes the old track.
- **Ranking and the automatic pick** (`src/lib/subtitles.ts`). Languages are compared as
  ISO 639-1 codes (`pt-BR` kept), mapped from ISO 639-2 and OpenSubtitles' own codes
  through `src/lib/languages.json`, the table Rust also uses for mpv. Candidates are
  ranked by preferred language (an exact region first), then source (the stream's own,
  then addons in their order), then how well a label matches the stream's file name
  (release group, source, resolution; hearing-impaired last). A moment after each
  `file-loaded`, when a subtitle may show by itself and the user has not picked one for
  the file: language by language, for the first preferred language that has a full (not
  forced) subtitle anywhere, keep what mpv picked if it is in that language, else select
  an embedded track in it, else download its best candidate. While the addons are still
  answering, the pick waits unless it already has a subtitle in the first preferred
  language; a candidate whose download fails gives way to the next (three tries).
  Preferred languages are the title's remembered one, then `subtitles.langs`, else the
  system language and English. The title's delay, speed and audio track apply at once.
- **Languages for mpv.** `player_start` and `player_load` read the title's remembered
  choices and set, before loading: `sid=no` when the title is remembered as "off" or
  automatic subtitles are off with nothing remembered, else `sid=auto` (always sent:
  options stay set in a core across loads); `alang` (the title's audio language, then
  `audio.langs`, or for anime `audio.animeLangs`) and `slang` (the title's subtitle
  language, then `subtitles.langs`), each language in every form (`ar,ara`;
  `fr,fre,fra`), and only when there are some, so external mpv keeps its own lists. So
  mpv's own selection already follows the user when the file opens. A pre-load option
  mpv refuses is logged and skipped, never fatal.
- **Style.** `subtitles.style` fields are mpv property values, checked by the same rules
  as property writes. Only fields the user set are applied, after the session starts
  (`sub-outline-*` first, then mpv < 0.38's `sub-border-*`); unset means mpv's look, or
  external mpv's config. Styled (ASS) subtitles keep their own look unless the user
  turns on `assOverride: "force"`. The player's quick style (size, position, background)
  applies at once (`player_set_sub_style`, which answers with what mpv refused, shown as
  a notice) and is saved as a one-field settings patch. The background box needs
  `sub-border-style` (mpv 0.38+); an older external mpv keeps its outline. Only external
  mpv reports refusals; the built-in core is the pinned libmpv, which has every property.
- **Per title** (`player-prefs.json` under `profile_dir()`, the 200 titles used last):
  the subtitle language picked (or "off") and the audio language, when the user picks
  them, and where that subtitle came from (`subSource`: `embedded`, `stream` or
  `addon`; in the remembered language the next file's automatic pick tries that source
  first); the subtitle delay and the speed whenever they change and settle (also from
  keys in the external mpv window). The next file of the title starts with them.
- **Hash.** When the stream names no OpenSubtitles hash, the addons are asked once the
  file has loaded, with the hash Rust reads from the stream itself
  (`player_video_hash`: the size plus the 64-bit words of the first and last 64 KiB,
  from two range requests with the stream's own headers, or the engine's loopback URL;
  the request is the one Rust remembered at `player_start`/`player_load`, so it never
  comes back through the webview; only while that session still runs and no other file
  came since, and a file that changed meanwhile gets none; 15 s for both requests, and
  the error logged without the URL). A server that ignores ranges is given up on after
  the first chunk. A file that has not loaded within 20 s is asked for without a hash.
- **Second subtitle.** The Subtitles menu can show a second subtitle above the first
  (`secondary-sid`, any track already in the player); `c` cycles it.
- **Menus** (`SubtitleMenu.tsx`, `AudioMenu.tsx`). Subtitles: Off/On, a language rail,
  tabs by source (in the video, addons, files), tags (Forced, SDH, Default, format,
  addon), a spinner while a subtitle downloads and its error inline, "Load from file…",
  the encoding of an added subtitle, the delay (±0.1/±0.5 s), and the quick style.
  Audio: tracks by language with title, channels (Stereo, 5.1, 7.1), codec and
  Default/File, the audio delay, and volume normalization. Both work the same on
  either backend.
- **Audio level.** Normalization (`player_set_audio_normalize`, `audio.normalize`)
  adds a fixed `lavfi=[dynaudnorm=f=200:g=15:m=10:r=0.6]` under the `@zeiler-normalize`
  label (adding again replaces it; removing it leaves the user's own filters), applied
  at every start and every load; `af` itself stays off the webview's list.
  `audio.volumeMax` (100–200, 130 by default, mpv's own) goes to mpv as the pre-load
  `volume-max` when it is not 130 (so external mpv otherwise keeps the user's own), and
  the volume slider and ↑ reach it.
- **A second subtitle** is per file: every load sends `secondary-sid=no`. mpv marks the
  second track `selected` too; the UI tells the main one by `sid` (or
  `main-selection`).
- **Shadow colour.** mpv 0.38+ reads `sub-shadow-color` as another name for
  `sub-back-color`, so `shadowColor` applies only while `backColor` is unset, and the
  Shadow choice leaves a Box background's colour alone.
- **Also in the control bar:** chapter ticks on the seek bar (hover shows the time and
  the chapter), a Quality menu when the stream has more than one video track (HLS or
  DASH variants, by height and bitrate; `vid`), and the stats overlay (key i: codecs,
  hardware decoding, frame rates, bitrates, dropped frames, buffer, download speed,
  read once a second through `player_get_property`).

## Skip and next episode

- **Skip segments.** `skip_segments_get` (`src-tauri/src/commands/skip.rs`,
  `crates/zeiler-skip`) asks AniSkip for `kitsu:`/`mal:` video ids (a Kitsu id is mapped
  to MyAnimeList's through Kitsu's own mappings first) and TheIntroDB for `tt…`/`tmdb:`
  ids, with season and episode from the video id; anything else is not looked up. It
  runs once per file, when its duration is first known, and only while
  `skip.onlineLookup` is on (Settings discloses what is sent). Answers, negative ones
  too, are cached (256 entries) per video length, for both services: AniSkip matches
  submissions by episode length, and TheIntroDB gets `duration_ms` to pick the release
  version (theatrical, extended cut…) closest to it. A failure is an empty list and is
  not cached; 8 s per request, 1 MiB per answer, never waited on. An open start or end
  is the start or end of the video; segments are clamped to the duration.
  Without an answer the UI classifies the file's chapter names (`src/lib/skip.ts`,
  Harbor's rules). With a next episode and no credits found, the last 90 s count as
  credits (only for a video at least 4.5 min long).
- **The pill and skipping by itself** (`useSkip.ts`, `SkipLayer.tsx`). While a segment
  plays (until 0.75 s before its end) a Skip pill shows (unless `skip.showButton` is
  off); Enter takes it. Credits with a next episode say "Next Episode" and play it.
  `skip.autoSkipIntro/Recap/Credits` (all off by default) skip once per segment per
  file, only when playback enters it by playing on (never after a seek), only for
  AniSkip/TheIntroDB segments or a chapter named exactly for it, never the synthetic
  credits, with an Undo toast for 5 s; credits skipped by themselves play the next
  episode when there is one. An online answer arrives after playback started, so a
  segment it brings counts as entered while playback is in its first 10 s, unless the
  user seeked in the file. With external mpv the offer also shows in
  mpv's window (`player_osd_prompt`: `show-text`, and Enter bound in a forced input
  section of its own that sends `script-message zeiler-skip`, back as a
  `client-message`; disabling the section restores the user's binding).
- **Next episode** (`useNextEpisode.ts`, `src/lib/next-video.ts`). The video after the
  current one by stremio-core's order and `next_video` rules (the same as
  `zeiler_account::next_video_id`). In the last 90 s (or when the credits start, or on
  n), the addons are asked for its streams; a stream from the same addon (or the meta's
  own streams) with the same non-empty `bingeGroup` continues this one and loads in the
  same player: `player_load` with the new video's context (its progress is tracked;
  the replaced file's is closed first), after `engine_resolve` for a torrent; the old
  torrent is removed once the new file is asked for, unless it is the same torrent. The
  page stays: the session counts the load as a new run (the subtitle hook takes its file
  as the title's own and asks the addons again with the new stream's hints) and from
  then on plays the new launch. Without a continuing stream the player is left and the
  next episode's stream picker opens (it replaces the picker the player came from).
  The Up Next card shows in the last `upNextSeconds` (15) and, with `bingeWatching`
  (on), counts down to the end, where the next episode plays by itself; closing the card
  stops that for this episode. Keys: n / p (and the media keys) for the next and the
  previous episode.

## Tauri IPC surface

Commands (all async, `Result<T, IpcError>`; payloads camelCase). This list matches the
`generate_handler!` in `src-tauri/src/lib.rs` exactly — update both together.

- Addons: `addon_preview { url, listingKey? }` → `{ descriptor, maskedUrl, existing?
  { kind: sameUrl|sameId|sameHost, name, transportUrl, maskedUrl }, configureUrl?,
  listingChanges, listingRequiresReview }` (fetch + parse, not persisted — feeds the
  confirm dialog; `listingKey` compares the live manifest with a directory listing) ·
  `addon_add { url, replace? }` → Descriptor (`replace` swaps an installed addon in place,
  keeping its position) · `addon_remove { transportUrl }` · `addon_list()` →
  Descriptor[] (built-ins first, then the user's addons; while `showAdult` is off,
  without adult genre options or adult-named catalogs) · `addon_reorder { order }` →
  Descriptor[] (an exact permutation of the user's transport URLs, else `conflict`) ·
  `addon_check_updates { force }` → `{ transportUrl, maskedUrl, name, currentVersion,
  latestVersion, changes, capabilitiesChanged, requiresReview }[]` (six at a time, 10 s
  each, reused for 6 h) · `addon_refresh { transportUrl }` → Descriptor (a one-click
  update; `conflict` when capabilities changed)
- Marketplace (only while `marketplaceEnabled`): `marketplace_list { query: { search?,
  types, resources, category?, sort: stars|newest|name }, page, limit }` → `{ entries,
  total, hasMore, sourcesOk, sourcesFailed, fetchedAt }` · `marketplace_categories()` ·
  `marketplace_detail { key }` → `{ entry, documentation? }` (plain text) ·
  `marketplace_refresh()`
- Profiles: `profile_list()` → `{ activeId, unlocked, profiles: [{ id, name, color,
  avatar (data: URL), isPrimary, locked, active }] }` (never the PIN hash) ·
  `profile_create { name, color }` · `profile_update { id, name?, color?, pin? }` ·
  `profile_set_pin { id, currentPin?, newPin? }` · `profile_set_avatar { id, dataUrl?,
  pin? }` · `profile_select { id, pin? }` (`locked` for a wrong or missing PIN,
  `conflict` while playing) · `profile_delete { id, pin? }`; each returns the new state
  and emits `profiles://changed`
- Account: `account_status()` → `{ signedIn, user?: { id, email, premium }, storage:
  os|memory, syncLibrary, syncAddons, needsAddonChoice, lastLibrarySync,
  lastAddonsSync, addonsLocked, lastError }` · `account_link_start()` → `{ code, link,
  qrcode? }` · `account_link_poll { code, remember }` → status, or null while pending ·
  `account_login_password { email, password, remember }` · `account_logout()` ·
  `account_set_sync { library?, addons? (false only) }` · `account_addons_preview()` →
  `{ accountOnly, localOnly, both }` (names) · `account_addons_apply { choice:
  useAccount|merge|keepLocal }` · `account_sync_now()` → `{ pulled, pushed, addons? }`;
  events `account://changed` (the new status) and `addons://changed` (a sync replaced
  the profile's addons) (`auth`, `sessionExpired`, `network` error kinds)
- Deep links: `deeplink_take_pending()` → the `stremio://` link that started the app,
  once · `deeplink_stremio_status()` → `{ supported, enabled, registered }` ·
  `deeplink_set_stremio { enabled }` (registers or unregisters with the OS, then stores
  `stremioDeepLink`; `settings_patch` refuses that key). Event `deeplink://stremio`
  carries a link opened while Zeiler runs.
- Content: `catalog_load { args: { transportUrl, type, id, extra, force? } }` →
  `{ metas }` (only from an addon in effect, for a catalog and extras it declares; while
  `showAdult` is off, never an addon flagged adult, an adult option or an adult-named
  catalog, and titles with adult genres are dropped; pages cached in memory for 15 min,
  256 at most, `force` skips the cache) ·
  `meta_get { type, id }` → `{ meta, asked, errors }` (first successful Meta across
  supporting addons; an anime id — `kitsu:`, `mal:`, `anilist:`, `anidb:` — that finds
  none under its type is retried as series, movie and anime, never re-asking an addon that
  timed out) · `streams_get { type, id }` → per-addon stream groups (parallel
  fan-out) · `subtitles_get { type, id, videoHash?, videoSize?, filename? }` →
  per-addon subtitle groups `{ transportUrl, addonName, subtitles, error? }` (parallel
  fan-out, 10 s per addon; the stream's file hints go along as the extras, each left out
  when malformed; see "Subtitles and audio")
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
- Subtitles (`commands/mpv/subtitles.rs`; see "Subtitles and audio"):
  `player_subtitle_add { request: { url, lang?, label?, select?, encoding? } }` →
  `{ file, format, encoding }` (download, normalize, `sub-add`) ·
  `player_subtitle_reencode { file, encoding? }` → the same (the subtitle again, read
  as another encoding) · `player_subtitle_pick_local()` → the same, or null when the
  dialog is cancelled · `player_set_sub_style { style }` → the mpv properties it refused
  (style fields, applied live) ·
  `player_prefs_get { metaId }` → `{ metaId, subLang, subSource, audioLang, subDelaySec,
  rate }` or null · `player_prefs_set { metaId, patch }` → the same ·
  `player_video_hash()` → `{ hash, size }` or null (the file playing, see "Subtitles
  and audio") · `player_set_audio_normalize { on }`
- Skip (see "Skip and next episode"): `skip_segments_get { query: { metaId, videoId,
  durationSec } }` → `[{ kind: "intro" | "recap" | "outro", startSec, endSec, source:
  "aniskip" | "introdb" }]` · `player_osd_prompt { kind: "intro" | "recap" | "outro" |
  "next" | null }` (external mpv's in-window prompt; a no-op for built-in playback)
- Deprecated player aliases (one release; `commands/mpv/aliases.rs`; the UI no longer
  calls any of them): `player_status()` →
  `{ available, path? }` (= `player_backends().external`) ·
  `player_embedded_available()` → bool · `player_play { request }` (start external) ·
  `player_embedded_start { request }` (start embedded, no fallback) ·
  `player_embedded_stop()` (stops only an embedded session) ·
  `player_embedded_set_property { name, value }` · `player_embedded_command { args }`
- Live TV (the user's IPTV, see Security posture): `iptv_sources()` →
  `[{id, name, kind, host, hasEpg, favorites, storage}]` · `iptv_source_add { source:
  {name, kind: m3u|xtream, url?, epgUrl?, xtream?, userAgent?, allowHttp?} }` (checked by loading
  its channels once) · `iptv_source_remove { id }` · `iptv_channels { id, force? }` →
  `{channels: [{id, name, logo, group, tvgId}], groups}` (6 h memory cache) ·
  `iptv_epg { id, tvgIds, fromMs, toMs }` → programmes per guide id (the guide is read
  downloaded whole, up to 100 MB, then unpacked and parsed as a stream, keeping only the
  source's channels a day ahead, for 1 h; one download per source at a time, a failure
  remembered for 5 min) · `iptv_favorite_set { id, channelId, on }` ·
  `iptv_resolve { sourceId, channelId }` → PlaybackRequest (http(s) only)
- Library (see "Library and progress"): `library_list { query: { filter:
  "continueWatching" | "library" | "history", sort?, type?, limit? } }` → LibraryEntry[] ·
  `library_get { id, videos? }` → `{ entry, watchedVideoIds }` or null ·
  `library_set { request: { meta, inLibrary } }` → LibraryEntry or null ·
  `library_mark_watched { request: { meta, videos, videoIds, season?, watched } }` →
  `{ entry, watchedVideoIds }` or null (a season, some videos, or with neither the
  whole title) ·
  `library_clear_progress { id }` → LibraryEntry or null
- Settings: `settings_get()` → Settings · `settings_patch { patch }` → Settings (merges
  the given top-level keys server-side, a group such as `subtitles` field by field;
  unknown keys and wrongly typed or out-of-range values, at any depth, are rejected) ·
  `settings_set { settings }` → Settings (**deprecated**, replaces every setting; kept
  for one release)
- Updates: `update_status()` → `{ current, ready? }` · `update_check()` → new version or
  null · `update_install_now()`

Errors serialize as `{ kind, message }`; `message` is a full sentence shown as-is. Kinds:
`invalidInput`, `addon`, `notFound`, `engine`, `player`, `storage`, `network` (service
unreachable or timed out; retryable), `auth` (credentials rejected), `sessionExpired`
(sign in again), `locked` (wrong PIN, or addon sync locked after a failed pull),
`conflict` (clashes with existing state, e.g. a changed addon list) and `unsupported`
(not on this platform/build).
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
- `subtitles://dropped` — `{ name, added }` or `{ name, error }`: how a subtitle file
  dropped on the window went (only while a player runs)

High-frequency data (engine stats, player position) will move to `tauri::ipc::Channel`
subscriptions when the embedded player lands.

## Persistence

All durable state is Rust-owned JSON under the app data dir; there is no localStorage.
`src-tauri/src/persist.rs` holds the rules: a missing file reads as defaults; an
unparseable one is moved to `*.json.corrupt.bak` and replaced by defaults; any other IO
error fails startup; writes are atomic and durable (`*.json.tmp`, flushed to disk, then
renamed over the target). `AppState` keeps
`settings.json` (device), `profiles.json` and the active profile's `addons.json`. Every
later feature owns a `JsonStore<T>` (load / get / update-and-persist under its own lock),
registered with `app.manage` — it does not grow `AppState`. New files carry a
`{"version": N, ...}` envelope. Per-user files take their path from
`AppState::profile_dir()`, the active profile's folder; re-creatable caches go under the
cache dir.

### Local profiles

`profiles.json` (`{ version, activeId, profiles: [{ id, name, color, avatar, isPrimary,
pinHash, createdAt }] }`) lists the profiles; each has a folder `profiles/<id>/` with its
`addons.json`, `library.json`, `player-prefs.json`, `prefs.json` (its per-profile settings
keys) and avatar picture. The first run of a build with profiles creates the primary
profile and **copies** (never moves) the per-user files and the profile keys of
`settings.json` into its folder, so an older build still finds them; a damaged
`profiles.json` adopts the folders on disk instead of starting over. `settings_get`
keeps its payload: device settings merged with the active profile's keys, and every
settings write also writes the active profile's `prefs.json`. A switch
(`profile_select`) is refused while a player runs; it reads the new folder first, then
swaps the addons, settings, library and per-title choices in place
(`JsonStore::switch_to`), and the UI remounts its shell keyed by the active profile. A
PIN (4–8 digits) is an Argon2id hash checked in Rust, with a growing wait after three
wrong tries; it guards switching to, editing and deleting a profile (only the active
profile counts as unlocked: switching away locks the one left), is set or changed only
on the active profile, and is a convenience lock only: the files are not encrypted.
While the active profile is locked at launch, the UI mounts nothing but "Who's
watching?". A forgotten PIN is removed by deleting that profile's `pinHash` from
`profiles.json` while Zeiler is closed. A damaged `profiles.json` adopts the profile
folders without their PINs and makes the first one the main profile. A switch first
moves the stores that read a file (library, player choices) and puts them back if the
rest fails; addon writes and switches are serialized; the first-run copy fills a
temporary folder and renames it into place. The primary and the active profile can't
be deleted. Avatars are PNG, JPEG, WebP or GIF (checked by their bytes), 512 KB at
most, resized in the UI and returned as `data:` URLs. The first such store is
the library, `library.json` (`{ version, uid, items }`, see "Library and progress").
`iptv.json` (`{ version, sources }`: names, hosts, user agents, favourites — the
addresses and accounts are in the OS keychain) holds the profile's IPTV sources.

Built-in addons are a table in `src-tauri/src/builtin.rs` (transport URL, compiled-in
manifest snapshot from `src-tauri/builtin/`, an enabled-by-settings predicate): Cinemeta,
Anime Kitsu, then OpenSubtitles v3, ahead of the user's addons, so meta asks Cinemeta
(`tt` ids) and Kitsu (`kitsu`/`mal`/`anilist`/`anidb` ids) first. A built-in the user
installed too is left out and their copy is used in their order, matched by transport URL
or by the manifest id in the snapshot (so a configured install or another host of the same
addon counts). Tests assert that no built-in declares a `stream` resource, and that each
declares only `catalog`, `meta`, `addon_catalog` or `subtitles`. Each entry also names
its role, the resources Zeiler uses from it, and its descriptor keeps only those (and its
catalog lists only when the role has catalogs; http artwork is fetched over https): Kitsu's
manifest declares subtitles too, which redirect to OpenSubtitles v3, so they are left out
and the OpenSubtitles switch covers every built-in subtitle request. A meta answered by a
built-in loses any streams embedded in its videos (`MetaLookup.source` says who answered;
`builtin::without_builtin_streams`), since the manifest tests can't see those.
`scripts/refresh-builtin-manifests.mjs` refreshes the snapshots (network; `--check` only
reports): it keeps each one byte for byte as served and refuses one whose id changed, that
declares anything else, or that comes from a redirect.

### Settings schema

`settings.json` is additive only: a new key always has a default, and an existing key
never changes name or type. Reading is lenient per top-level key — a wrongly typed value
resets just that setting to its default (with a warning) instead of moving the whole file
aside — while `settings_patch` is strict. Nested groups are lenient per field inside,
otherwise one bad sub-field resets the whole group: `src-tauri/src/settings_groups.rs`
reads every settings object (the top level and each group) key by key from its defaults,
each key strictly typed and checked, keeping the default of a key that does not fit (so
a default of `true` stays `true`, which `DefaultOnError` could not do). A patch merges an
object value into its group field by field (arrays replace), so a writer names only what
it changes, and it is refused unless every value it names survives that read unchanged —
an unknown field, a wrong type or an out-of-range value at any depth. Settings are
mirrored wholesale to the webview, so secrets (Stremio authKey, IPTV credentials) never
go here.

The groups so far: `subtitles { langs, autoLoad, style { scale, fontSize, font, bold,
color, outlineColor, outlineSize, borderStyle, backColor, shadowOffset, shadowColor, pos,
marginY, alignX, alignY, assOverride } }` (style values are mpv's, checked by the
player's property rules; every style field `null` by default), `audio { langs,
animeLangs, normalize, volumeMax }` (`animeLangs` defaults to `["ja"]`, `normalize`
off, `volumeMax` 130, 100–200) and `skip { showButton, autoSkipIntro, autoSkipRecap,
autoSkipCredits, onlineLookup }` (auto-skip off, the rest on), plus the top-level
`bingeWatching` (on) and `upNextSeconds` (15, 5–60).

Reserved key map — names are fixed now; each key is added only when its feature ships:

| Scope | Keys (existing **bold**) |
| --- | --- |
| Per device | **`mpvPath`**, **`engineListenPort`**, **`engineUpnp`**, **`uploadLimitKibps`**, **`downloadLimitKibps`**, **`deleteOnStop`**, **`autoUpdate`**, **`playerBackend`** (`auto`/`embedded`/`external`), **`marketplaceEnabled`**, **`marketplaceSources{}`** (`stremioAddonsNet`, `stremioCatalog`), **`stremioDeepLink`** |
| Per profile | **`theme`**, **`instantPlay`**, **`hideAnime`**, **`hideLiveTv`**, **`showAdult`** (the single adult flag and age gate, also used by the marketplace; matched by `zeiler_addon::is_adult_text`), **`hideP2pAddons`**, **`homeRows`** (`{order, hidden, renamed: [{key, name}]}`, keyed without transport URLs), **`subtitles{}`** (languages, auto-load, style), **`audio{}`** (languages, normalize, volume max), **`skip{}`** (skip button, auto-skip, online lookups), **`bingeWatching`**, **`upNextSeconds`** |
| Built-in toggles (scope is an owner decision; device until then) | **`builtinCinemeta`**, **`builtinKitsu`**, **`builtinOpensubtitles`** |

A settings file from before `builtinKitsu` and `builtinOpensubtitles` existed takes them
from `builtinCinemeta` (Kitsu is also off when `hideAnime` is on), so an update never
switches on a third-party host for someone who had switched off the only built-in.
Per-profile keys are kept per profile in `profiles/<id>/prefs.json` (see "Local
profiles"); `settings.json` keeps the last active profile's copy for older builds. There
is no `profile{}` group in Settings.

## Account sync

A profile may sign in to a Stremio account (Settings → Profiles & account): a code
confirmed at link.stremio.com (recommended; the password never reaches Zeiler), or email
and password, which cross IPC once and go only to api.strem.io. The session key is kept
by `account/secrets.rs` in the OS keychain (`io.zeiler.desktop`, account
`stremio-auth:<profileId>`) when the user asks to stay signed in, else in memory; never
in a file or settings, never over IPC, never logged; a key read from the keychain is
kept in memory for the run, so the keychain is asked once per profile. The profile's
`account.json` holds only the user's id, email, avatar URL and premium date (never the
Trakt token or anything else of the user object), the sync switches and the last sync
times. The QR code of a link sign-in is shown only when Stremio sends it as a `data:`
image or an https URL (the latter is fetched from Stremio's chosen host).

- **Library** (on once signed in): stremio-core's plan — `datastoreMeta`, pull what is
  newer there (`datastoreGet`; everything in one request on a first sync), push what is
  newer here (`datastorePut`, 50 per request), per item, newer `_mtime` wins at second
  precision; the library is then marked as that account's (`uid`). A library used
  before signing in is merged into the account; one marked as another account's is kept
  beside it (`library-<uid>.json`) and the profile starts that account's own, so two
  accounts' libraries never mix. Items written are
  stremio-core's exact shape (`zeiler-account`). Runs at sign-in, shortly after launch,
  every 15 minutes, a minute after a local change, and on "Sync now". Progress during
  playback reaches the account with the next sync (no 180 s push of its own yet).
- **Addons** (off until the user answers the sign-in's question: use the account's,
  combine, or don't sync): the account's collection is the source of truth. A pull
  replaces the profile's addons with the account's, leaving out Zeiler's built-ins
  (which stay under Settings → Content), entries Stremio protects (its own, like Local
  Files) and loopback addresses, all of which stay in the account untouched; every local
  add, remove, replace or reorder is queued (in order) and replayed onto a fresh `addonCollectionGet` (`zeiler_sync::sync::rebase_addon_op`) and
  sent with `addonCollectionSet`, keeping the account's own entries, flags and unknown
  fields byte for byte and never adding a built-in. Stremio has no compare-and-swap, so
  an edit made in Stremio at the same moment can be lost. Queued changes go before every
  pull, and a pull leaves the local list alone while any are left, so a change that
  couldn't be sent is never undone (the queue is in memory: quitting before it is sent
  loses it from the account, not from the profile). A failed pull pauses pushes
  (`addonsLocked`) until a pull succeeds. Transport URLs are matched once parsed.
- Error code 1 (session ended) signs the profile out. Signing out keeps the library
  (still marked as that account's) and the addons as they are; deleting a profile also
  forgets its session key. A profile switch waits for a running sync and never overlaps
  one (lock order: `sync_lock`, then `profile_op`).

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
- **In the UI** (`src/lib/library.tsx`). The UI never computes library state: it reads
  views from Rust and re-reads them on `library://changed`. `LibraryProvider` keeps
  Continue Watching (the Home row and the progress bars on posters); hooks read one
  view (the Library page's Continue Watching / Library / History tabs, the library
  tab in stremio-core's sorts) or one title with its episodes' watched marks (Detail,
  the stream picker). A Continue Watching card resumes: the stream picker for the
  video it stopped at. When the picked video has a resume point, the picker asks
  "Resume from 12:34" or start over before it opens the Player (`startAtSec`).

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
- **Embedded** (`embedded` feature; on by default in Windows and macOS releases): libmpv actor +
  event thread (`crates/zeiler-player/src/embedded.rs`), `config=no`, no
  scripts/ytdl/OSC/input, `vo=gpu-next,` (the trailing comma falls back through mpv's
  default VO list instead of failing on RDP, VMs or old GPUs), `hwdec=auto-safe`. On
  Unix it resets `LC_NUMERIC` to `"C"` before creating a core (GTK sets the user's
  locale, and libmpv refuses to start otherwise). `EmbeddedOptions { surface }` says
  where the video goes, fixed at spawn:
  - `Surface::Wid(hwnd)` — a child window of the main window (Windows; with
    `force-window=immediate`);
  - `Surface::RenderApi(host)` — macOS: `vo=libmpv`, `video-timing-offset=0`; the
    host (`RenderHost`, `crates/zeiler-player/src/render.rs`) creates the OpenGL render
    context once the core is up and before anything loads, and frees it before the
    core quits (the actor calls `detach` before `quit`; a `GlRenderContext` also holds
    the core, so the core is never destroyed under it);
  - `Surface::Detached` — mpv's own top-level window;
  - `Surface::Headless` — `vo=null`, `ao=null`, for tests.

  Per-OS surface code: `commands/mpv/windows.rs` (child HWND pinned under the
  transparent WebView2), `macos.rs` (render API into an NSOpenGLView under the
  WKWebView), `linux.rs` (not available: Linux keeps mpv in its own window),
  `stub.rs` (feature off). `player_backends` reports why built-in playback is
  unavailable in a sentence Settings → Player shows as-is (per OS: mpv window on
  Linux, a Windows or macOS build without the feature, or a missing `libmpv-2.dll`).
- **macOS surface** (`commands/mpv/macos.rs`, adapted from Harbor's
  `mpv_render_mac.rs`). On the core's attach, the main thread puts an `NSOpenGLView`
  (OpenGL 3.2 core, opaque surface, Retina-resolution drawable) into the WKWebView's
  superview, ordered below the webview, turns the webview's `drawsBackground` off
  and its layer non-opaque, creates the render context and clears the view to black
  until mpv's first frame. mpv's update callback queues one redraw at a time on the
  main dispatch queue; a redraw renders into the default framebuffer at the view's
  backing size and flushes. `player_set_geometry` gets the page's physical pixels and
  divides them by the window's scale factor (NSView frames are points; Harbor used
  the pixels as points), flipping into AppKit's bottom-up coordinates; a rect that
  fills the window keeps filling it while the window resizes. Same transparency
  policy as Windows: the webview draws its background again when the surface is
  removed (the core's detach, `player_stop`, or a start that ends elsewhere). A
  newer core's install takes the surface of the one it replaces over, so nothing
  flashes between two built-in videos; the replaced core's detach then leaves it
  alone. libmpv is linked normally (it is always in the bundle), so a macOS build
  with the feature reports built-in playback available.
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
  validated commands (subtitles: `player_subtitle_*`, which add only files Zeiler wrote,
  and `player_set_sub_style`, whose fields pass the same property rules).

## Navigation names

The frontend navigates with a frame stack (`src/lib/view.tsx`). Every switch over
`RootView` or `Frame` kinds ends in `unreachable(x)`, so a new kind that is not handled
everywhere fails `tsc`. Nothing is reserved any more:
the frame `browse` (the catalog browser, `src/views/Browse.tsx`) is in use since M8 and
replaced `filter`; the root view `live` (Live TV: the addons' `tv` and `channel` catalogs,
`src/views/Live.tsx`) since M9; the frame `addon-detail` (a marketplace listing, keyed by
manifest id) since M10.
Reserved Settings section id: `playback` (`subtitles` is in use since M6: Settings →
Subtitles & audio; `account` since M11: Settings → Profiles & account).

The Player is not a frame: it is the `#/player?h=<id>` hash route, drawn over the
still-mounted shell (see "Playback entry point"). The frame stack is untouched while it
is up; leaving the player pops the history entry it pushed.

## Security posture

- Strict CSP (no remote script/style; `img-src https: http:` because addon posters are
  arbitrary hosts; loopback wildcards for the engine stream only).
- Least-privilege capabilities; no fs/shell/http plugin — the webview can only invoke our
  typed commands. The dialog plugin is registered for Rust's own subtitle file picker
  only; no capability grants the webview its commands. Player property writes, reads and
  commands pass the allow-list tables in `commands/mpv/validate.rs`; stream URLs,
  headers and tokens reach mpv over its private IPC connection, never argv.
- Skip lookups go only to AniSkip, Kitsu and TheIntroDB (fixed hosts, from Rust, behind
  a setting); the in-window skip prompt sends fixed mpv commands only.
- Subtitle downloads are http(s) only and capped (10 MiB, also unpacked); archives, web
  pages and any text that does not start as a subtitle format (an mpv EDL, a CUE sheet,
  a playlist) are refused, because a saved file is local to mpv; files land under the
  cache dir with random names, and only those are ever handed to `sub-add`. A local
  file must be a regular file and is read no further than the cap.
- Addons are untrusted JSON: stream/external URLs are scheme-allow-listed before reaching
  mpv or the OS (mpv would happily open `file://`); `proxyHeaders` are treated as
  attacker-controlled; infoHashes validated as 40-char hex.
- Addon transport URLs may embed credentials (config-in-URL pattern) — they are secrets;
  never logged, and sent nowhere but the addon host, except to the user's own Stremio
  account while they have addon sync on (as Stremio's apps do; the sync switch and the
  privacy text say so).
- The Stremio session key lives in the OS keychain (or memory), never in a file,
  settings or IPC, and is never logged; the password crosses IPC once in a password
  sign-in and is wiped after the call; only the user's id, email, avatar URL and premium
  date are kept from the user object.
- IPTV sources (`src-tauri/src/iptv.rs`): `iptv.json` holds only names, hosts, user
  agents and favourites; playlist and guide URLs and Xtream accounts go to the OS
  keychain (`src-tauri/src/secrets.rs`, IPTV's memory fallback over M12's
  `account/secrets.rs` `KeyringStore`), or
  for one run to memory when the keychain fails — never to a file. The webview gets
  channel ids, names, logos and groups; a channel's stream URL only from `iptv_resolve`,
  straight into the one-shot player launch (marked `live`). Addresses are fetched over
  https only, also across redirects, unless the user accepted plain http for that
  source when adding it (`allowHttp`: the account would travel unencrypted). Errors never quote a URL,
  redirects send no Referer, and the player logs a stream as scheme and host only
  (`redact_url`; external mpv's own "Playing: <url>" line is silenced).
- Playback goes through the in-memory one-shot handoff, never the location hash: stream
  URLs, headers and transport URLs stay out of the history stack, and release builds
  ignore URL-carrying player params.
- Engine HTTP binds 127.0.0.1 with per-session basic-auth credentials.
- Deep links are untrusted argv input. `stremio://` is opt-in (`stremioDeepLink`,
  registered at runtime on Windows and Linux; tauri.conf.json declares no scheme, so it
  is never claimed while off). On Windows the scheme's previous per-user owner (a
  Stremio app) is saved when Zeiler takes over and put back when it lets go; Zeiler
  never touches the machine-wide key. A lone `stremio://` argument becomes the install
  confirmation's input, parsed in Rust like a pasted URL, never an install. Links are
  ignored while the setting is off or a confirmation is open, a repeat within 2.5 s
  counts once, and links are never logged (they may carry an addon's keys).
- Directory listings are untrusted display data: remote `flags` are dropped, the
  documentation is plain text (never HTML), slugs are checked before they reach a URL,
  and directory errors name the directory, never a URL. The index always includes
  adult listings (so their `nsfw` category marks the same addon from any directory)
  and hides them locally unless `showAdult`. HTTP errors from addons and directories
  are passed on without their URL (reqwest's `without_url`). Transport URLs are masked in the
  UI until revealed.

## Packaging & licensing notes

- `license = "GPL-3.0-or-later"` on every crate; LICENSE at root; THIRD-PARTY notices for
  Apache-2.0 (librqbit) and any MIT-attributed code adapted from stremio-core/Harbor.
- libmpv is **not** statically linked. Windows releases ship `libmpv-2.dll` beside the
  exe via `bundle.resources` (`src-tauri/tauri.embedded.conf.json`, which also installs
  the LGPL-2.1 text from `src-tauri/licenses/` as `licenses/LGPL-2.1.txt` and the fetch
  script's notice — versions, provenance, source links and the written source offer —
  as `licenses/libmpv-NOTICE.txt`). It is the LGPL
  flavour of zhongfly/mpv-winbuild, pinned by tag and archive SHA-256 in
  `scripts/fetch-libmpv.mjs`, which downloads it from Zeiler's own `libmpv-<tag>`
  pre-release (zhongfly prunes old releases; the mirror also holds the build inputs and
  LGPL sources, recorded in `docs/libmpv/`); THIRD-PARTY-NOTICES.md names the exact mpv
  and FFmpeg commits and carries the source offer. The archive has no `.def`, so the script writes
  one from the DLL's export table (`LIBRARY libmpv-2.dll`) and generates `mpv.lib` with
  MSVC `lib /def:`. The app links it DELAYLOAD (`src-tauri/build.rs`) so it starts —
  and falls back to external mpv — when the DLL is missing. macOS releases bundle
  libmpv and the libraries it loads under `Contents/Frameworks`
  (`bundle.macOS.frameworks` in the overlay `src-tauri/libmpv-macos/<arch>/tauri.conf.json`
  that `scripts/fetch-libmpv.mjs` writes, with the notice and licence texts as
  resources); `src-tauri/build.rs` links them with the rpath
  `@executable_path/../Frameworks` (debug builds also the vendor dir). They are built
  from pinned sources by `scripts/build-libmpv-macos.sh` (mpv `-Dgpl=false`, FFmpeg
  without `--enable-gpl`/`--enable-version3`, libplacebo, libass with FreeType,
  HarfBuzz and FriBidi, dav1d; macOS 12.0 deployment target; `@rpath` install names;
  ad-hoc signed) in the `libmpv-macos` workflow, and kept with every source archive
  on Zeiler's `libmpv-macos-<version>` pre-release (`docs/libmpv/`).
  `scripts/check-macos-app.sh` (release.yml, after each macOS build) checks the app
  loads nothing from outside itself and the system, that no library needs a newer
  macOS than the app, and that the signature verifies. Linux keeps the distro mpv. If a
  GPL libmpv/FFmpeg build is ever shipped, the corresponding source offer goes in the
  release notes.
- `deny.toml` allows `LGPL-2.1-only` for the `libmpv2`/`libmpv2-sys` bindings (a
  clarification, since they declare the deprecated id `LGPL-2.1`). CI's `deny` job runs
  `cargo deny --all-features check licenses sources`, `zeiler/embedded` included.
- Every bundle ships `licenses/THIRD-PARTY-LICENSES.txt` (`bundle.resources` in
  `tauri.conf.json`): the licences of the Rust crates and npm packages in that build,
  generated per release target by `scripts/third-party-licenses.mjs` (cargo-about with
  `about.toml`, plus Vite's `build.license` report) in `release.yml`'s `licenses` job.
  The file is git-ignored; `src-tauri/build.rs` writes a placeholder for other builds,
  since `tauri_build` refuses a missing resource.
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
4. **Later** — casting, Live TV/EPG, PiP, theme system. (Optional Stremio account sync,
   isolated and off by default, shipped with M12; see "Account sync".)
