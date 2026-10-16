# Third-party notices

Zeiler is licensed under GPL-3.0-or-later (see [LICENSE](LICENSE)). It builds on the
following third-party work:

## Protocol semantics

`crates/zeiler-addon` re-implements the Stremio addon protocol. Its lenient parsing
semantics (field aliases, null handling, URL construction, resource-routing rules) were
written against the behavior of **stremio-core**
(<https://github.com/Stremio/stremio-core>), MIT License, Copyright (c) 2019 SmartCode OOD,
and the **stremio-addon-sdk** protocol documentation
(<https://github.com/Stremio/stremio-addon-sdk>), MIT License.

## stremio-core and stremio-watched-bitfield (MIT)

`crates/zeiler-account` adapts the library model of **stremio-core**
(<https://github.com/Stremio/stremio-core>): the `libraryItem` schema
(`types/library/library_item.rs`), the player's progress rules (`models/player.rs`),
the meta's video order and next video (`types/resource/meta_item.rs`), marking videos
watched (`models/meta_details.rs`), the library update and view rules
(`models/ctx/update_library.rs`, `models/library_with_filters.rs`), and ports its **stremio-watched-bitfield** codec. The
adapted files (`src/types.rs`, `src/library.rs`, `src/watched.rs`) keep a pointer to
this notice. The player's next episode (`src/lib/next-video.ts`) follows the same video
order and next-video rules, and keeps the pointer too.

```text
MIT License

Copyright (c) 2019 SmartCode OOD

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Harbor (MIT)

Zeiler's built-in player follows the Tauri + libmpv technique shipped by **Harbor**
(<https://github.com/harborstremio/harbor>), and parts of its UI follow Harbor's design
system. Code adapted from Harbor keeps a pointer to this notice in the adapted file.

Adapted files:

- `crates/zeiler-player/src/embedded.rs` — libmpv embedding approach: pre-init options,
  the `gpu-next,` video-output fallback list, the `LC_NUMERIC` reset on Unix
- `src-tauri/src/commands/mpv/windows.rs` — mpv child window pinned under the WebView2
  (`HWND_BOTTOM`, hit-test transparency), the WebView2 transparent background and its
  re-apply after window changes
- `src/pages/player/surface-sync.ts` — re-asserting the video surface on a kick
  schedule after fullscreen and window changes
- `crates/zeiler-skip/src/{lib,aniskip,introdb}.rs` — the AniSkip and TheIntroDB
  lookups (`src/lib/skip-intro/{aniskip,theintrodb}.ts`): the Kitsu → MyAnimeList
  mapping, request parameters, skip-type mapping, open start/end handling
- `src/lib/skip.ts` — the chapter-title classifier, source priority and the active
  segment's end guard (`src/lib/skip-intro/{chapters,index}.ts`)
- `src/pages/player/SkipLayer.tsx` — the skip pill and the Up Next card
  (`skip-pill.tsx`, `skip-pill-container.tsx`), and `useNextEpisode.ts` — the
  end-of-episode rule (`use-auto-next-episode.ts`)
- `src/pages/player/StatsOverlay.tsx` — the stats overlay's property list
  (`stats-overlay.tsx`), and `src-tauri/src/commands/mpv/audio.rs` — the volume
  normalization filter's values (`mpv.ts`)
- `src/index.css`, `src/lib/theme.ts` — design tokens and theme presets
- `src/lib/view.tsx`, `src/components/{ui,cards,Row,Hero,Poster,GenreTiles,LibraryCard}.tsx`,
  `src/views/{Discover,Browse}.tsx` — navigation model and UI recipes
- `crates/zeiler-addon/src/adult.rs` — the adult-content term lists (`adult-filter.ts`)
- `src/lib/home-customization.ts` — Home row order, hiding and renaming
- `src/lib/rooms.ts`, `src/lib/catalogs.ts` (`isAnimeCatalog`) — which rows go to Home
  and which to the Anime room
- `crates/zeiler-addon/src/request.rs` (`normalize_transport_input`) — the rules for the
  addon links people paste (`parseAddonUrl`)
- `crates/zeiler-addon/src/directory.rs` — the directory endpoints and the merge by
  manifest id (`src/lib/providers/stremio-addons.ts`, `src/lib/addons-store/community.ts`)
- `src-tauri/src/deeplink.rs` — runtime `stremio://` registration
  (`deeplink_set_stremio`); `src/views/addons/*` — install match kinds, the facts table
  and masked addresses (`install-modal.tsx`, `addon-detail.tsx`)
- `src/views/profiles/*` — the "Who's watching?" picker layout and the avatar resize to
  a small WebP (`picker-modal`, `avatar-utils.ts`)

```text
MIT License

Copyright (c) 2026 Harbor

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Bundled/linked libraries

- **librqbit** (<https://github.com/ikatson/rqbit>) — Apache License 2.0,
  Copyright 2021 Igor Katson. Used as the embedded BitTorrent engine.
- **Tauri** (<https://tauri.app>) — MIT or Apache-2.0.
- **libmpv2 / libmpv2-sys** (<https://github.com/kohsine/libmpv-rs>) — LGPL-2.1. Rust
  bindings to libmpv, compiled in with the `embedded` feature (Windows releases).
- **mpv / libmpv and FFmpeg** — see the next section for the Windows build Zeiler ships.
  macOS and Linux builds do not bundle libmpv; they use the user's own mpv program.

## libmpv and FFmpeg in the Windows installer

The Windows installer ships `libmpv-2.dll` beside `zeiler.exe` for built-in playback. It
is dynamically linked (never statically) and can be replaced by any compatible libmpv
build. Next to it, in `licenses\`, the installer puts:
- the licence texts (`LGPL-2.1.txt`, `LGPL-3.0.txt`, `GPL-3.0.txt`, from
  `src-tauri/licenses/`);
- `libmpv-THIRD-PARTY.txt`, with the copyright notices and licence texts of the
  libraries linked into the DLL;
- `libmpv-NOTICE.txt`, with the versions, the provenance and the source offer below
  (written by `scripts/fetch-libmpv.mjs`).

- **Build**: the LGPL flavour of zhongfly/mpv-winbuild, release `2026-10-15-5757d69708`,
  archive `mpv-dev-lgpl-x86_64-20261015-git-5757d69708.7z`, SHA-256
  `5417be2641c31e2c5e676bb7cce90244edb43758936193c6c36dfcf5ea35eabd` (pinned and checked
  by `scripts/fetch-libmpv.mjs`). zhongfly prunes old releases, so Zeiler keeps the
  archive unchanged on its own pre-release
  [`libmpv-2026-10-15-5757d69708`](https://github.com/malhaniah/zeiler/releases/tag/libmpv-2026-10-15-5757d69708),
  which is where the script downloads it from. The build inputs (workflow run,
  toolchain, the commit of every bundled library) are recorded in
  [`docs/libmpv/2026-10-15-5757d69708.md`](docs/libmpv/2026-10-15-5757d69708.md).
- **Versions** (as reported by the DLL): mpv `v0.41.0-1184-g5757d6970`, built with
  `-Dgpl=false`; FFmpeg `N-127570-gaf25036ac`.
- **License**:
  - mpv, in its LGPL configuration, is under the GNU Lesser General Public License,
    version 2.1 or later.
  - FFmpeg is built with `--enable-version3` and without `--enable-gpl` (zhongfly's
    `compile-lgpl-libmpv.patch` keeps version3 for libraries such as OpenSSL 3), so it is
    under the GNU Lesser General Public License, version 3 or later.
  - `libmpv-2.dll` as a whole is therefore distributed under the LGPL, version 3 or
    later.
  - The libraries those builds link statically (FreeType, HarfBuzz, libass, libplacebo,
    OpenSSL, dav1d, libvpx and more) keep their own licences. Their notices are in
    `src-tauri/licenses/libmpv-THIRD-PARTY.txt`, which the installer ships.
- **Source offer.** The complete corresponding source is kept by Zeiler at
  <https://github.com/malhaniah/zeiler/releases/tag/libmpv-2026-10-15-5757d69708>:
  - source archives of mpv, FFmpeg and every other LGPL library inside the DLL
    (libplacebo, FriBidi, libbluray, libudfread, libiconv, LAME, libsoxr, OpenAL Soft,
    libssh, uchardet, the VapourSynth headers), at the exact commits built;
  - the build scripts: zhongfly/mpv-winbuild at `8a39c51` (with its
    `compile-lgpl-libmpv.patch`), which drives shinchiro/mpv-winbuild-cmake at
    `1f8d2e7` (the per-library source URLs, options and patches);
  - `BUILD-INPUTS.md`, the same record as `docs/libmpv/2026-10-15-5757d69708.md`.

  Upstream: mpv <https://github.com/mpv-player/mpv/tree/5757d6970>, FFmpeg
  <https://github.com/FFmpeg/FFmpeg/tree/af25036ac>.

  Written offer: for at least three years after the last Zeiler release that ships
  this build, Zeiler's maintainers will give anyone a copy of that complete
  corresponding source, for no more than the cost of providing it — ask by opening an
  issue at <https://github.com/malhaniah/zeiler/issues>.

## Rust crates and npm packages

Besides the libraries above, Zeiler compiles in several hundred Rust crates (mostly MIT
or Apache-2.0) and bundles a few npm packages into its interface (React, lucide-react,
Tauri's JavaScript API and Tailwind CSS's base styles). Every release, on every platform,
ships their licence texts and copyright notices in `THIRD-PARTY-LICENSES.txt`, in the
`licenses` folder next to `zeiler.exe` on Windows, in
`Zeiler.app/Contents/Resources/licenses/` on macOS, and in `/usr/lib/Zeiler/licenses/`
on Linux.

`scripts/third-party-licenses.mjs` writes that file for each release target, in
`release.yml`'s `licenses` job. The crates come from `Cargo.lock` through cargo-about
(`about.toml`), limited to the crates built for that target with its features. The npm
packages come from Vite's report of what the interface build bundles. To write it
yourself (with cargo-about 0.9 installed):

```bash
npm run build && node scripts/third-party-licenses.mjs
```

CI (the `deny` job in `ci.yml`) runs `cargo deny --all-features check licenses sources`,
which checks every crate's licence and source against `deny.toml`, with
`zeiler/embedded` included.

## Trademarks

"Stremio" is a trademark of its respective owner (Smart Code Ltd). Zeiler is an
independent project, not affiliated with or endorsed by Stremio.
