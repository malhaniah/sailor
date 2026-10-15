# Sailor

**A free, open-source desktop client for the Stremio ecosystem — built in Rust.**

Sailor is a self-contained media client that speaks the open [Stremio addon protocol](https://github.com/Stremio/stremio-addon-sdk/blob/master/docs/protocol.md).

> Sailor hosts no media, indexes no media, and ships no content addon.
> You bring your own addons; Sailor is just the boat.

## What it is

- **Self-contained** — a built-in torrent streaming engine and mpv playback; no external streaming server, no browser, no Node runtime required.
- **Bring your own addons** — paste any addon manifest URL. Sailor ships no stream or content addon and endorses none. For browsing out of the box, the Cinemeta *metadata* addon (catalogs, details, episode lists; no streams) is built in and can be turned off in Settings.
- **Updates itself** — signed updates install automatically the next time you open Sailor.
- **Rust core** — the addon protocol client, streaming engine, and playback orchestration are Rust crates; the UI is a thin TypeScript layer in a Tauri 2 shell.
- **Cross-platform** — Windows, macOS, Linux.

## Status

Early development (v0.1 milestone): addon install/manage, catalog browsing, search,
metadata/episodes, stream fan-out, built-in torrent engine, and playback through an
external mpv. Embedded libmpv playback (video inside the app window) is the next milestone.

## Install

Grab the latest build from the [Releases page](https://github.com/malhaniah/sailor/releases).
Playback needs [mpv](https://mpv.io) installed (Sailor finds it automatically in the usual
places, or you can set its path in Settings).

| Platform | Download | mpv |
| --- | --- | --- |
| Windows 10/11 | `Sailor_*_x64-setup.exe` (per-user, no admin) | `winget install shinchiro.mpv` |
| macOS 12+ (Apple Silicon) | `Sailor_*_aarch64.dmg` | `brew install mpv` |
| macOS 12+ (Intel) | `Sailor_*_x64.dmg` | `brew install mpv` |
| Debian / Ubuntu | `Sailor_*_amd64.deb` | pulled in automatically |
| Fedora / openSUSE | `Sailor-*.x86_64.rpm` | from your distro (Fedora: RPM Fusion) |
| Other Linux | `Sailor_*_amd64.AppImage` | from your distro |

Builds are not code-signed yet. On Windows, SmartScreen may warn — choose **More info →
Run anyway**. On macOS, right-click Sailor → **Open** the first time (or allow it under
System Settings → Privacy & Security).

## Architecture

| Component | Role |
| --- | --- |
| `crates/sailor-addon` | Stremio addon-protocol client (manifests, catalogs, meta, streams, subtitles) |
| `crates/sailor-engine` | Built-in BitTorrent streaming engine (librqbit) exposing local HTTP streams |
| `crates/sailor-player` | Playback: external mpv now, embedded libmpv in progress |
| `src-tauri` | Tauri 2 shell wiring the core to the UI |
| `src/` | TypeScript/React frontend |

## Building from source

Prerequisites: Rust (stable, MSVC toolchain on Windows), Node.js ≥ 22, and
[mpv](https://mpv.io) for playback (install it on your PATH, or set its location in
Settings — Sailor works without it, but can't play anything).

```bash
npm install
npm run tauri dev
```

Release bundles: `npm run tauri build`. Architecture notes live in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md); third-party attribution in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) — setup, quality gates, and the one
non-negotiable rule (Sailor ships no content, ever).

## License

Sailor is licensed under the [GNU General Public License v3.0](LICENSE).

Sailor is an independent project and is not affiliated with or endorsed by Stremio (Smart Code Ltd).
