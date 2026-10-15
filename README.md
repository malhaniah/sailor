# Sailor

**A free, open-source desktop client for the Stremio ecosystem — built in Rust.**

Sailor is a self-contained media client that speaks the open [Stremio addon protocol](https://github.com/Stremio/stremio-addon-sdk/blob/master/docs/protocol.md).

> Sailor hosts no media, indexes no media, and ships no content addon.
> You bring your own addons; Sailor is just the boat.

## What it is

- **Self-contained** — native playback via libmpv and a built-in streaming engine; no external server, no browser, no Node runtime required.
- **Bring your own addons** — paste any addon manifest URL. Sailor ships none and endorses none.
- **Rust core** — the addon protocol client, streaming engine, and playback orchestration are Rust crates; the UI is a thin TypeScript layer in a Tauri 2 shell.
- **Cross-platform** — Windows, macOS, Linux.

## Status

Early development (v0.1 milestone): addon install/manage, catalog browsing, search,
metadata/episodes, stream fan-out, built-in torrent engine, and playback through an
external mpv. Embedded libmpv playback (video inside the app window) is the next milestone.

## Architecture

| Component | Role |
| --- | --- |
| `crates/sailor-addon` | Stremio addon-protocol client (manifests, catalogs, meta, streams, subtitles) |
| `crates/sailor-engine` | Built-in BitTorrent streaming engine (librqbit) exposing local HTTP streams |
| `crates/sailor-player` | libmpv playback orchestration |
| `src-tauri` | Tauri 2 shell wiring the core to the UI |
| `ui/` | TypeScript/React frontend |

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

## License

Sailor is licensed under the [GNU General Public License v3.0](LICENSE).

Sailor is an independent project and is not affiliated with or endorsed by Stremio (Smart Code Ltd).
