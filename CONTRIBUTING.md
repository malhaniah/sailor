# Contributing to Sailor

Thanks for considering a contribution! Sailor is a free, open-source (GPL-3.0-or-later)
desktop client for the Stremio addon ecosystem, and it stays healthy through small,
well-tested changes.

## The one rule that is not negotiable

**Sailor hosts no media, indexes no media, and ships no content addon.**

Pull requests that add bundled/seeded addons, hardcoded addon or tracker hosts, default
content sources, scrapers, or anything that turns Sailor itself into a content source
will be declined regardless of code quality. Features that help users manage addons
*they* choose to install are always welcome.

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

On Linux you also need the Tauri system packages (webkit2gtk 4.1, libayatana-appindicator,
librsvg, patchelf — see `.github/workflows/ci.yml` for the exact list).

## Where things live

| Path | What it is |
| --- | --- |
| `crates/sailor-addon` | Stremio addon-protocol client (types, URL building, fan-out) |
| `crates/sailor-engine` | Embedded librqbit torrent engine + loopback stream endpoint |
| `crates/sailor-player` | Playback (external mpv backend; embedded libmpv behind the `embedded` feature) |
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

Network integration tests against live public addons are opt-in:

```bash
cargo test -p sailor-addon --test live -- --ignored
```

## Guidelines

- **Small PRs.** One change per PR; refactors separate from behavior changes.
- **Tests ride along.** Protocol/engine changes need unit tests; parsing changes need a
  fixture that mirrors real addon output (see `crates/sailor-addon/src/*` test modules).
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
