# Zeiler

**A free, open-source desktop client for the Stremio ecosystem — built in Rust.**

Zeiler is a self-contained media client that speaks the open [Stremio addon protocol](https://github.com/Stremio/stremio-addon-sdk/blob/master/docs/protocol.md).

> Zeiler hosts no media, indexes no media, and ships no content addon.
> You bring your own addons; Zeiler is just the boat.

## What it is

- **Self-contained** — a built-in torrent streaming engine and mpv-based playback (built into the window on Windows); no external streaming server, no browser, no Node runtime required.
- **Bring your own addons** — paste any addon manifest URL. Zeiler ships no stream or content addon and endorses none. For browsing out of the box, a few *metadata and subtitle* addons are built in — Cinemeta (catalogs, details, episode lists), Anime Kitsu (anime catalogs and details) and OpenSubtitles v3 (subtitles) — none serves streams, and each can be turned off in Settings.
- **Updates itself** — signed updates install automatically the next time you open Zeiler.
- **Rust core** — the addon protocol client, streaming engine, and playback orchestration are Rust crates; the UI is a thin TypeScript layer in a Tauri 2 shell.
- **Cross-platform** — Windows, macOS, Linux.

## Status

Early development: addon install/manage, catalog browsing, search, metadata/episodes,
stream fan-out, a built-in torrent engine, Zeiler's own player screen, a local library
that remembers where you stopped (Continue Watching, resume, watched marks), and
subtitles in your languages from the video, your subtitle addons or a file — picked
automatically, with delay, encoding and style controls and per-show memory. On Windows,
videos play inside the Zeiler window (built-in libmpv). On macOS and Linux they play in an
mpv window that Zeiler controls with the same full set of controls; built-in playback for
macOS comes next.

## Install

Grab the latest build from the [Releases page](https://github.com/malhaniah/zeiler/releases).
On Windows, playback is built in. On macOS and Linux, playback needs
[mpv](https://mpv.io) installed (Zeiler finds it automatically in the usual places, or you
can set its path in Settings).

| Platform | Download | mpv |
| --- | --- | --- |
| Windows 10/11 | `Zeiler_*_x64-setup.exe` (per-user, no admin) | not needed — optional, for playing in your own mpv window: `winget install shinchiro.mpv` |
| macOS 12+ (Apple Silicon) | `Zeiler_*_aarch64.dmg` | `brew install mpv` |
| macOS 12+ (Intel) | `Zeiler_*_x64.dmg` | `brew install mpv` |
| Debian / Ubuntu | `Zeiler_*_amd64.deb` | pulled in automatically |
| Fedora / openSUSE | `Zeiler-*.x86_64.rpm` | from your distro (Fedora: RPM Fusion) |
| Other Linux | `Zeiler_*_amd64.AppImage` | from your distro |

Builds are not code-signed yet. On Windows, SmartScreen may warn — choose **More info →
Run anyway**. On macOS, right-click Zeiler → **Open** the first time (or allow it under
System Settings → Privacy & Security).

**Upgrading from Sailor.** Zeiler was called Sailor up to v0.1.2, and Sailor installs update
to Zeiler on their own. Your addons and settings come along. On Windows the update also
removes Sailor. On macOS the updated app keeps its old name in Applications: rename
`Sailor.app` to `Zeiler.app`, or replace it with the new `.dmg`.

## Architecture

| Component | Role |
| --- | --- |
| `crates/zeiler-addon` | Stremio addon-protocol client (manifests, catalogs, meta, streams, subtitles) |
| `crates/zeiler-engine` | Built-in BitTorrent streaming engine (librqbit) exposing local HTTP streams |
| `crates/zeiler-player` | Playback: embedded libmpv (Windows), or the user's mpv remote-controlled over JSON IPC |
| `src-tauri` | Tauri 2 shell wiring the core to the UI |
| `src/` | TypeScript/React frontend |
| `website/` | The download website for zeiler.io (static, Vite) — see [website/README.md](website/README.md) |

## Building from source

Prerequisites: Rust (stable, MSVC toolchain on Windows), Node.js ≥ 22, and
[mpv](https://mpv.io) for playback (install it on your PATH, or set its location in
Settings — Zeiler works without it, but can't play anything).

```bash
npm install
npm run tauri dev
```

Built-in playback on Windows is the `embedded` cargo feature and needs the pinned libmpv
first — see [CONTRIBUTING.md](CONTRIBUTING.md#built-in-playback-windows). Release bundles:
`npm run tauri build`. Architecture notes live in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md); third-party attribution in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) — setup, quality gates, and the one
non-negotiable rule (Zeiler ships no content, ever).

## License

Zeiler is licensed under the [GNU General Public License v3.0](LICENSE).

Zeiler is an independent project and is not affiliated with or endorsed by Stremio (Smart Code Ltd).
