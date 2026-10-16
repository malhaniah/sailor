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

## Architecture reference

The Tauri + libmpv integration approach planned for the embedded player follows the
technique shipped by **Harbor** (<https://github.com/9000000/harbor>), MIT License,
Copyright (c) Harbor contributors. Any code adapted from Harbor retains its MIT notice in
the adapted files.

## Bundled/linked libraries

- **librqbit** (<https://github.com/ikatson/rqbit>) — Apache License 2.0,
  Copyright 2021 Igor Katson. Used as the embedded BitTorrent engine.
- **Tauri** (<https://tauri.app>) — MIT or Apache-2.0.
- **mpv / libmpv** (<https://mpv.io>) — LGPL-2.1+ or GPL-2.0+ depending on build flavor.
  Zeiler does not bundle libmpv in development builds; release packaging will state the
  exact build used and provide a corresponding source offer.

A complete dependency license inventory can be generated with
`cargo deny check licenses` / `cargo about generate`.

## Trademarks

"Stremio" is a trademark of its respective owner (Smart Code Ltd). Zeiler is an
independent project, not affiliated with or endorsed by Stremio.
