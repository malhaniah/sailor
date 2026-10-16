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
this notice.

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
- `src/index.css`, `src/lib/theme.ts` — design tokens and theme presets
- `src/lib/view.tsx`, `src/components/{ui,cards,Row,Hero,Poster,GenreTiles,LibraryCard}.tsx`,
  `src/views/{Discover,Filter}.tsx` — navigation model and UI recipes

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
build. The installer puts the LGPL-2.1 text (`licenses\LGPL-2.1.txt`, from
`src-tauri/licenses/`) and a notice with the versions, the provenance and the source
offer below (`licenses\libmpv-NOTICE.txt`, written by `scripts/fetch-libmpv.mjs`) next
to it.

- **Build**: the LGPL flavour of zhongfly/mpv-winbuild, release
  [`2026-10-15-5757d69708`](https://github.com/zhongfly/mpv-winbuild/releases/tag/2026-10-15-5757d69708),
  archive `mpv-dev-lgpl-x86_64-20261015-git-5757d69708.7z`, SHA-256
  `5417be2641c31e2c5e676bb7cce90244edb43758936193c6c36dfcf5ea35eabd` (pinned and checked
  by `scripts/fetch-libmpv.mjs`).
- **Versions** (as reported by the DLL): mpv `v0.41.0-1184-g5757d6970`, built with
  `-Dgpl=false`; FFmpeg `N-127570-gaf25036ac`.
- **License**: mpv (in its LGPL configuration) and FFmpeg are licensed under the GNU
  Lesser General Public License, version 2.1 or later. The DLL also contains libraries
  those builds link statically (for example libass, libplacebo, FreeType, HarfBuzz,
  FriBidi, libbluray, libarchive, libssh); each keeps its own license, as listed by
  the build scripts below.
- **Source offer.** The complete corresponding source is available at:
  - mpv: <https://github.com/mpv-player/mpv/tree/5757d6970>
  - FFmpeg: <https://github.com/FFmpeg/FFmpeg/tree/af25036ac>
  - build scripts: <https://github.com/zhongfly/mpv-winbuild/tree/2026-10-15-5757d69708>
    (CI and package versions for that release), which drive
    <https://github.com/shinchiro/mpv-winbuild-cmake> (the per-library source URLs and
    patches)

  Written offer: for at least three years after the last Zeiler release that ships
  this build, Zeiler's maintainers will give anyone a copy of that complete
  corresponding source, for no more than the cost of providing it — ask by opening an
  issue at <https://github.com/malhaniah/zeiler/issues>.

A complete dependency license inventory can be generated with
`cargo deny check licenses` / `cargo about generate`.

## Trademarks

"Stremio" is a trademark of its respective owner (Smart Code Ltd). Zeiler is an
independent project, not affiliated with or endorsed by Stremio.
