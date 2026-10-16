# Feature plan handoff

How to pick up the feature plan locally. Written when the cloud session handed over on
2026-10-16.

## Where things stand

- **Released:** v0.1.3, the first release named Zeiler. It has Windows, both macOS builds
  and, for the first time, Linux (.deb, .rpm, .AppImage).
- **Work in progress:** branch `claude/blissful-newton-kbhlcn`, open as draft PR #4. CI is
  green on Windows, macOS and Linux. The `windows-installer` job uploads an installer with
  built-in playback as an artifact you can try. Nothing in PR #4 is released yet.

| Commit | Milestone | What it did |
|---|---|---|
| `445d7c3` | M0 | Foundations: `persist.rs` JsonStore, `builtin.rs` registry with a no-stream test, per-key lenient settings, `settings_patch` + `settings://changed`, IpcError kinds, reserved view names |
| `d213ced` | M1 | Every playback opens Zeiler's own Player screen (one-shot handoff id in the hash), and the app stays mounted underneath |
| `a6c0356` | M2a | Backend-neutral player API (`player_start/load/stop/set_property/command/get_property/backends`), external mpv over JSON IPC, allow-list validation |
| `7838aae` | M2b | Full remote controls for external mpv; Auto / Built-in / External choice in Settings |
| `21887d4` | M3 | Built-in (embedded libmpv) playback is the Windows default; CI covers it; the Windows installer artifact; licensing |

## Decisions already made by the owner

- **Player work:** this plan owns all of it (embedded, skip, subtitles). Embedded runs on
  Windows now and comes to macOS later (M13). Linux keeps mpv in its own window, with full
  controls in Zeiler.
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
- `m4a-wip.patch` holds unreviewed, unfinished M4a work: a new `crates/zeiler-account`
  crate with library types, the watched bitfield and progress logic. Apply it with
  `git apply docs/plan/m4a-wip.patch`, or discard it and redo M4a from the spec.

## Remaining milestones (in order)

| # | Milestone | Scope (see critique.md §4 and the named spec) |
|---|---|---|
| M4a | Library core (Rust) | `crates/zeiler-account` with stremio-core-compatible LibraryItem, the watched bitfield and `apply_playback_progress` (70% rule, live excluded). LibraryStore under `profile_dir()`. `library_*` commands plus `library://changed`. Progress is tracked in Rust from player events, using a playback context passed to `player_start`. account-profile.json steps 1, 3 and 7 (library parts only). |
| M4b | Library UI | Library view tabs (Continue Watching / Library / History), a Continue Watching row on Home, watched marks and add-to-library on Detail, a "Resume from 12:34" prompt, progress bars on posters |
| M5 | Built-ins | OpenSubtitles v3 (subtitles) and anime metadata snapshots in the builtin registry, per-built-in toggles, meta-type fallback for `kitsu:`/`mal:` ids. Update the CONTRIBUTING/ARCHITECTURE policy text to "metadata or subtitles only" (the owner asked for OpenSubtitles). |
| M6 | Subtitles and audio | `subtitles_get` fan-out, subtitle fetch and normalize (gzip, encoding detection), `player_subtitle_add`, pick a local file (tauri-plugin-dialog, Rust-side only), subtitle menu with sources, delay, style and saved preferences, audio delay and speed (player-ui.json steps 4–7) |
| M7 | Skip and next episode | A `zeiler-skip` crate (AniSkip for anime, TheIntroDB, chapter-name fallback), a skip pill, an Up Next card, auto-skip off by default with Undo, bingeGroup next episode via `player_load` (player-ui.json) |
| M8 | Catalogs | Every type and genre, driven by manifest extras; a Browse view; manifest-driven genre tiles; lazy rows; row customization; adult gating (catalogs.json) |
| M9 | Live | A: the addon `tv`/`channel` room and its Sidebar entry. B: M3U/XMLTV/Xtream, with credentials in a SecretStore and the `live` flag in the player (catalogs.json) |
| M10 | Marketplace | URL normalization, replace and reorder, update detection, an opt-in community directory with a detail page and labels; stremio:// handler on Windows and Linux (macOS needs an owner decision) (marketplace.json) |
| M11 | Local profiles | `profiles.json`, migration by copy (not move), Argon2 PIN, avatars, a picker, per-profile addons, library and preferences |
| M12 | Stremio account | Link-code sign-in first, then password; keychain storage; library sync; addon sync with Add/Remove/Reorder; never push built-ins (account-profile.json) |
| M13 | macOS built-in playback | Render-API NSOpenGLView port of Harbor's `mpv_render_mac.rs` (geometry in points), bundled `@rpath` dylibs |
| M14 | Optional | Linux embedded, only if a GtkGLArea prototype works first |

Also still open: the download website. The owner asked for OS detection, a downloads page and a
product tour. It is not started, and the domain is `zeiler.io`.

## How each milestone was built

Each milestone went through the same loop, and each step is worth keeping:

1. Implement against `critique.md` plus the area spec, and keep house conventions
   (`specs/conventions.json`, CONTRIBUTING.md).
2. Smoke-test the UI in a real browser: `npm run dev` plus Playwright with
   `window.__TAURI_INTERNALS__` mocked, and a screenshot of every flow.
3. Two independent reviews: correctness and behaviour preservation, then spec, security and
   cross-platform. Have someone try to refute each finding; fix only the confirmed ones.
4. Run every gate after the last edit, then commit one milestone per commit with the why in
   the body.

## Gates (run all before every commit)

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

## Not yet verified on a real machine

- Windows built-in playback at runtime: WebView2 transparency, re-pinning the video under
  the webview, focus hand-back, and Escape/fullscreen. CI linked the code and ran the
  headless libmpv tests on Windows, but nobody has watched a video yet. Try the
  `windows-installer` artifact from PR #4.
- The Sailor → Zeiler NSIS hook (`src-tauri/windows/hooks.nsh`) runs only when a real
  Sailor v0.1.2 install updates.

## Releasing

1. Bump the version in `Cargo.toml`, `package.json` and both lockfiles (`npm version
   X --no-git-tag-version`, `cargo update --workspace`).
2. Merge to `main`.
3. Push a `vX.Y.Z` tag, or run `release.yml` by hand on `main`. tauri-action then creates
   the tag and release from the version.

Keep the same updater signing key (`TAURI_SIGNING_PRIVATE_KEY`); rotating it would cut
every installed copy off from updates.
