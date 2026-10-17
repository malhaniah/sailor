<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/zeiler-logo-white.png">
    <img src="assets/brand/zeiler-logo-black.png" alt="" width="160" height="160">
  </picture>
</p>

<h1 align="center">Zeiler</h1>

<p align="center">A free, open-source desktop client for the Stremio addon ecosystem, built in Rust.</p>

<p align="center">
  <a href="https://github.com/malhaniah/zeiler/releases/latest"><img src="https://img.shields.io/github/v/release/malhaniah/zeiler" alt="Latest release"></a>
  <a href="https://github.com/malhaniah/zeiler/actions/workflows/ci.yml"><img src="https://github.com/malhaniah/zeiler/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI status"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/malhaniah/zeiler" alt="License: GPL-3.0"></a>
  <img src="https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux-555" alt="Platforms: Windows, macOS, Linux">
</p>

Zeiler is a self-contained media client that speaks the open
[Stremio addon protocol](https://github.com/Stremio/stremio-addon-sdk/blob/master/docs/protocol.md):
a built-in torrent streaming engine, mpv-based playback and its own player screen, with no
external streaming server, no browser and no Node runtime required. It is in early development.

> Zeiler hosts no media, indexes no media, and ships no content addon.
> You bring your own addons; Zeiler is just the boat.

Paste any addon manifest URL. Zeiler ships no stream or content addon and endorses none. For
browsing out of the box, a few *metadata and subtitle* addons are built in — Cinemeta
(catalogs, details, episode lists), Anime Kitsu (anime catalogs and details) and
OpenSubtitles v3 (subtitles) — none serves streams, and each can be turned off in Settings.

## Download

Get the latest build from the [Releases page](https://github.com/malhaniah/zeiler/releases/latest).
Playback is built in on Windows and macOS. On Linux it needs [mpv](https://mpv.io) installed
(Zeiler finds it automatically in the usual places, or you can set its path in Settings).

| Platform | Download | mpv |
| --- | --- | --- |
| Windows 10/11 | `Zeiler_*_x64-setup.exe` (per-user, no admin) | not needed — optional, for playing in your own mpv window: `winget install shinchiro.mpv` |
| macOS 12+ (Apple Silicon) | `Zeiler_*_aarch64.dmg` | not needed — optional, for playing in your own mpv window: `brew install mpv` |
| macOS 12+ (Intel) | `Zeiler_*_x64.dmg` | not needed — optional: `brew install mpv` |
| Debian / Ubuntu | `Zeiler_*_amd64.deb` | pulled in automatically |
| Fedora / openSUSE | `Zeiler-*.x86_64.rpm` | from your distro (Fedora: RPM Fusion) |
| Other Linux | `Zeiler_*_amd64.AppImage` | from your distro |

Builds are not code-signed or notarized yet. On Windows, SmartScreen may warn — choose
**More info → Run anyway**. On macOS, right-click Zeiler → **Open** the first time (or allow
it under System Settings → Privacy & Security). Once installed, Zeiler updates itself:
signed updates install automatically the next time you open it.

**Upgrading from Sailor.** Zeiler was called Sailor up to v0.1.2, and Sailor installs update
to Zeiler on their own. Your addons and settings come along. On Windows the update also
removes Sailor. On macOS the updated app keeps its old name in Applications: rename
`Sailor.app` to `Zeiler.app`, or replace it with the new `.dmg`.

## Features

- **Its own player.** Every video opens Zeiler's player screen with the full set of mpv controls: seek, speed, chapters on the seek bar, a stats overlay, a quality menu for HLS variants, keyboard shortcuts and fullscreen.
- **Built-in playback on Windows and macOS.** libmpv draws the video inside the Zeiler window. On Linux, videos play in an mpv window that Zeiler controls with the same full set of controls.
- **Library and Continue Watching.** Zeiler remembers where you stopped: resume prompts, watched marks for titles, seasons and episodes, a history, and progress bars on posters.
- **Skip and next episode.** Intros, recaps and credits can be skipped (by themselves too, with an Undo), and the next episode plays from the same source when one ends. Skip times come from AniSkip and TheIntroDB, which then learn which title and episode you play; Settings → Player & quality turns that off.
- **Subtitles and audio.** Subtitles in your languages from the video, your subtitle addons or a file, picked automatically, with delay, encoding and style controls, per-show memory and a second subtitle track. Audio tracks and delay, volume normalization, and volume above 100%.
- **Catalogs for every type.** Movies, Shows and Anime rooms with genre rows from your addons' manifests, every other type on Home, a Browse screen for any catalog and its filters, and Home rows you can move, hide and rename. Adult catalogs stay hidden unless you turn them on.
- **Live TV.** Channels from your addons' tv and channel catalogs, plus your own IPTV: M3U and Xtream Codes playlists with an XMLTV guide for now and next. Playlist credentials go to your system's keychain, never to a file.
- **Addon marketplace.** Paste an addon URL, or browse an opt-in community directory in which every listing is labelled "not reviewed by Zeiler". Every install goes through a confirmation dialog, and installed addons can be updated, reconfigured and reordered. `stremio://` links can open in Zeiler on Windows and Linux (opt-in).
- **Local profiles.** Everyone sharing the computer gets their own addons, library and preferences, with an optional PIN.
- **Optional Stremio account sync.** Sign a profile in to a Stremio account, with a code you confirm in the browser or with email and password, to sync its library and, if you choose, its addons with Stremio's own apps.
- **Updates itself.** Signed updates from GitHub releases install automatically the next time you open Zeiler.
- **Rust core.** The addon protocol client, streaming engine, playback orchestration and sync rules are Rust crates; the UI is a thin TypeScript layer in a Tauri 2 shell. Windows, macOS and Linux.

## Privacy

Zeiler has no telemetry and no account of its own. Signed out, it talks only to the
addons you install (and the built-ins you leave on), to GitHub for updates, to AniSkip,
Kitsu and TheIntroDB for skip times unless you turn that off, to the addon directories
once you turn the directory on, and to any IPTV provider you add yourself. If you sign a profile in to a
Stremio account, Zeiler sends Stremio's servers (api.strem.io, link.stremio.com) your
sign-in (a code you confirm in the browser, or your email and password), your library
items (titles, progress, watched marks) and, if you turn on addon sync, your addons'
addresses, including any keys inside configured ones. The session key is kept in your
system's keychain if you ask to stay signed in (otherwise only in memory until Zeiler
quits); nothing else goes to Stremio, and nothing is sent there once you sign out.

## Architecture

| Component | Role |
| --- | --- |
| `crates/zeiler-addon` | Stremio addon-protocol client (manifests, catalogs, meta, streams, subtitles) |
| `crates/zeiler-engine` | Built-in BitTorrent streaming engine (librqbit) exposing local HTTP streams |
| `crates/zeiler-player` | Playback: embedded libmpv (Windows, macOS), or the user's mpv remote-controlled over JSON IPC |
| `crates/zeiler-account` | Library items, playback-progress rules and the watched bitfield (Stremio-compatible) |
| `crates/zeiler-skip` | Skip segments from AniSkip and TheIntroDB |
| `crates/zeiler-iptv` | M3U, Xtream Codes and XMLTV parsing and fetching |
| `crates/zeiler-sync` | Opt-in Stremio account sync: the account API client and the library/addon sync rules |
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

Built-in playback on Windows and macOS is the `embedded` cargo feature and needs the
pinned libmpv first — see [CONTRIBUTING.md](CONTRIBUTING.md#built-in-playback-windows-macos). Release bundles:
`npm run tauri build`. Architecture notes live in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md); third-party attribution in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) — setup, quality gates, and the one
non-negotiable rule (Zeiler ships no content, ever). Bug reports and feature requests
have [issue forms](https://github.com/malhaniah/zeiler/issues/new/choose).

## Security

Found a vulnerability? Please report it privately through
[GitHub's security advisories](https://github.com/malhaniah/zeiler/security/advisories/new),
not as a public issue. [SECURITY.md](SECURITY.md) has the supported versions, what to
include, the response targets and what is in scope.

## License

Zeiler is licensed under the [GNU General Public License v3.0](LICENSE).

Zeiler is an independent project and is not affiliated with or endorsed by Stremio (Smart Code Ltd).

## Third-party notices

Zeiler builds on stremio-core's protocol semantics and Harbor (both MIT), on mpv and libmpv
and FFmpeg (LGPL, bundled on Windows and macOS), and on many Rust crates and npm packages.
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) lists every attribution, and each bundle
ships the licence texts of what it contains.
