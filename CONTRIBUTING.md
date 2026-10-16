# Contributing to Zeiler

Thanks for considering a contribution! Zeiler is a free, open-source (GPL-3.0-or-later)
desktop client for the Stremio addon ecosystem, and it stays healthy through small,
well-tested changes.

## The one rule that is not negotiable

**Zeiler hosts no media, indexes no media, and ships no content addon.**

Pull requests that add bundled/seeded stream addons, hardcoded stream or tracker hosts,
default content sources, scrapers, or anything that turns Zeiler itself into a content
source will be declined regardless of code quality. Features that help users manage
addons *they* choose to install are always welcome.

The single exception is metadata: Zeiler builds in the Cinemeta metadata addon
(catalogs, details, episode lists — it serves no streams), and users can turn it off in
Settings. Any further built-in source must likewise be metadata-only and switchable.

## Getting set up

Prerequisites:

- **Rust** stable (MSVC toolchain on Windows) — the version is pinned in
  [rust-toolchain.toml](rust-toolchain.toml)
- **Node.js** ≥ 22
- **mpv** on your PATH for playback (<https://mpv.io>) — optional; the app runs without it

```bash
npm install
npm run tauri dev
```

### Built-in playback (Windows)

Built-in playback (libmpv in-process, video under the webview) is the `embedded` cargo
feature. Windows release builds have it on; plain `npm run tauri dev` builds leave it off
and play through external mpv. To work on it on Windows:

```powershell
node scripts/fetch-libmpv.mjs      # once: pinned libmpv-2.dll + mpv.lib into src-tauri/libmpv
npm run tauri dev -- --features embedded
npm run tauri build -- --features embedded --config src-tauri/tauri.embedded.conf.json
```

The fetch script downloads the hash-pinned LGPL libmpv build (see THIRD-PARTY-NOTICES.md),
writes `mpv.def` from the DLL's exports and generates the `mpv.lib` import library with
MSVC's `lib.exe` (found on PATH or through vswhere). It does nothing on other systems.
`src-tauri/tauri.embedded.conf.json` bundles the DLL and its NOTICE into the installer.

`cargo check`/`clippy` with the feature never link, so `ZEILER_SKIP_LIBMPV_CHECK=1` lets
them run before the fetch. Tests of the player crate do link libmpv:

```bash
# Linux: needs libmpv-dev (mpv ≥ 0.35, client API 2.x)
cargo test -p zeiler-player --features embedded
```

```powershell
# Windows: put the vendored libmpv on the linker path (LIB) and the DLL on PATH
$env:LIB = "$PWD\src-tauri\libmpv;$env:LIB"; $env:PATH = "$PWD\src-tauri\libmpv;$env:PATH"
cargo test -p zeiler-player --features embedded
```

These run headless (no window, no audio device). macOS and Linux keep external mpv for
now (`commands/mpv/{macos,linux}.rs`).

On Linux you also need the Tauri system packages (webkit2gtk 4.1, libayatana-appindicator,
librsvg, patchelf — see `.github/workflows/ci.yml` for the exact list).

## Where things live

| Path | What it is |
| --- | --- |
| `crates/zeiler-account` | Library items, playback-progress rules and the watched bitfield (Stremio-compatible; pure, no I/O) |
| `crates/zeiler-addon` | Stremio addon-protocol client (types, URL building, fan-out) |
| `crates/zeiler-engine` | Embedded librqbit torrent engine + loopback stream endpoint |
| `crates/zeiler-player` | Playback session: external mpv over JSON IPC; embedded libmpv behind the `embedded` feature |
| `src-tauri` | Tauri shell: typed IPC commands, state, capabilities, config |
| `src/` | React/TypeScript UI (design system mirrors Harbor's tokens — see `docs/ARCHITECTURE.md`) |
| `docs/ARCHITECTURE.md` | Load-bearing decisions; read this first |

Domain crates never depend on `tauri` and are tested with plain `cargo test`.

## Quality gates (CI enforces all of these)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run typecheck
npm run build
```

CI also lints and tests the `embedded` feature (Linux with the distro libmpv; Windows
with the vendored one, including the shell's Windows code) and builds a Windows installer
with built-in playback for every pull request (a downloadable workflow artifact).

Network integration tests against live public addons are opt-in:

```bash
cargo test -p zeiler-addon --test live -- --ignored
```

## Guidelines

- **Small PRs.** One change per PR; refactors separate from behavior changes.
- **Tests ride along.** Protocol/engine changes need unit tests; parsing changes need a
  fixture that mirrors real addon output (see `crates/zeiler-addon/src/*` test modules).
- **Lenient in, strict out.** Addon responses are untrusted, messy JSON — parsing must
  degrade gracefully, never panic, and unknown fields must be ignored. Anything we emit
  (IPC payloads, URLs) is exact.
- **Security posture holds.** All addon HTTP stays in Rust; URLs handed to the player or
  OS are scheme-allow-listed; addon transport URLs may embed credentials and must never
  be logged.
- **UI changes** follow the existing design tokens (`src/index.css`) and component
  recipes; keep the Harbor-derived class vocabulary rather than inventing new styles.
- **Commit messages**: imperative mood, explain *why* in the body when it isn't obvious.

## Licensing

By contributing you agree that your contribution is licensed under
**GPL-3.0-or-later**, the project license. If you adapt code from elsewhere it must be
GPL-compatible and attributed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)
(e.g. MIT code keeps its copyright notice in the adapted file).

## Conduct

Be kind, be constructive, assume good faith. Maintainers may edit or decline
contributions that don't fit the project's principles or scope.
