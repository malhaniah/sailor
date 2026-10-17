<!-- One change per PR; refactors separate from behaviour changes. See CONTRIBUTING.md. -->

## Why

<!-- The problem or need, and why this is the right change. Link the issue if there is one. -->

## What

<!-- What changed, from the user's or the code's point of view. Name any new dependency
(and say that `cargo update` was not run), and any settings key, IPC command or file
format added. -->

## How it was verified

<!-- Tests added or changed. What you tried in the app, on which OS, with built-in or
external mpv. What you could not verify, and why. -->

## Gates run

<!-- Tick what you ran locally. CI runs all of them, plus the `embedded` feature. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `npm run typecheck`
- [ ] `npm run build`

## Checklist

- [ ] Zeiler still ships no content: no bundled or seeded stream addon, hardcoded stream or
      tracker host, default content source or scraper. Any new built-in is metadata or
      subtitles only, switchable, and never serves streams.
- [ ] Addon responses are parsed leniently (no panics, unknown fields ignored); anything
      emitted (IPC payloads, URLs) is exact.
- [ ] No addon transport URL, stream URL or credential is logged, and none reaches the
      webview or the OS without the allow-list checks.
- [ ] Protocol, engine and parsing changes come with unit tests or a fixture that mirrors
      real addon output.
- [ ] UI changes use the existing design tokens and component recipes.
- [ ] Adapted code is GPL-compatible and attributed in `THIRD-PARTY-NOTICES.md`.
- [ ] Docs are updated where behaviour changed (`README.md`, `docs/ARCHITECTURE.md`).
