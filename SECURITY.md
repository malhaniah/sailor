# Security policy

Zeiler is a desktop client for the Stremio addon ecosystem. It plays media from addons
the user installs, runs a torrent engine and an updater, and keeps profile data, playlist
credentials and account sessions on the user's machine. A bug in any of that can matter,
so please report security problems privately and give us a little time to fix them before
they become public.

## Supported versions

Only the latest release gets security fixes. There are no maintenance branches: a fix
ships as a new release, which installed copies pick up on their own.

| Version | Supported |
| --- | --- |
| v0.2.1 (current) | Yes |
| Older releases, including Sailor v0.1.x | No. Update to the latest release. |

The next release will carry milestones M7 to M13 (skip and next episode, catalogs, Live TV
and IPTV, the addon marketplace, local profiles, Stremio account sync and built-in playback
on macOS). Once it is published it becomes the only supported version.

## Reporting a vulnerability

Please do not open a public issue, pull request or discussion for a security bug.

Report it through GitHub's private vulnerability reporting for this repository:
**[Report a vulnerability](https://github.com/malhaniah/zeiler/security/advisories/new)**
(the repository's Security tab → Advisories → Report a vulnerability). Only the
maintainer can see the report, and the discussion and the fix stay private until an
advisory is published.

### What to include

- The Zeiler version (Settings → About shows it) and your operating system.
- The part affected: the app, the updater, a built-in addon integration, the torrent
  engine, playback, profiles, IPTV, account sync.
- Steps to reproduce, or a proof of concept: a crafted addon manifest or response, a
  playlist or guide, a stream URL, a subtitle file, a sequence of clicks.
- What an attacker gains: which data, which code runs, on whose machine.
- Whether it is already public, and how you would like to be credited.

### Response targets

- An acknowledgement within **72 hours**.
- Within **14 days**, a fix or a status: what was found, what is planned and when. Harder
  problems get an update at least every 14 days until they are resolved.
- The fix ships as a new release, and the advisory is published with it. You are credited
  unless you prefer not to be named.

Zeiler is a volunteer project with no budget, so there is no bug bounty.

## Scope

In scope:

- **The app**: everything in this repository that ships in a release. The Rust crates
  (addon client, torrent engine, player, skip lookups, IPTV, account sync), the Tauri
  shell, the webview UI and the installers.
- **The updater**: `latest.json` on GitHub releases, the signature check, and how an
  update is downloaded and installed.
- **The built-in addons**: the Cinemeta, Anime Kitsu and OpenSubtitles v3 integrations,
  meaning how Zeiler talks to those services and handles their answers. The services
  themselves belong to their operators.

Reports we care about most: a malicious addon, playlist, guide, subtitle or stream that
can run code, read files or reach other hosts from Zeiler; credentials or session keys
ending up in files, logs or IPC payloads; a way around the updater's signature check; the
player, the torrent engine or the local stream endpoint reachable by other programs or
other machines; mistakes in the profile PIN or keychain handling.

Out of scope:

- **Third-party addons.** Zeiler ships none. Addons you install, including anything in
  the opt-in community directory, are labelled "not reviewed by Zeiler" and never are.
  Report a problem in an addon to its author. A problem in how Zeiler *handles* an
  addon's data is in scope.
- Stremio's own services and apps, AniSkip, TheIntroDB, OpenSubtitles, the addon
  directories, and the content behind any stream.
- Denial of service through a huge playlist, guide or addon response, unless it leaves
  the app unable to start.
- Anything that first needs a compromised machine or physical access. A profile PIN is a
  convenience lock, not encryption, and the app says so.
- Vulnerabilities in upstream dependencies (mpv, FFmpeg, librqbit, Tauri, ...) with no
  Zeiler-specific impact. Do tell us if the fix is a version bump we should make.

## Updates

Installed copies check GitHub releases
(`releases/latest/download/latest.json`) at launch and every few hours, and install an
update the next time Zeiler opens. Every update package is signed with Zeiler's updater
key (Tauri's updater, minisign), and the matching public key is compiled into the app, so
a package that is not signed with that key is refused. Drafts and pre-releases, such as
the libmpv mirror releases, are never offered as updates.

Release builds are not yet code-signed on Windows or notarized on macOS, so for a first
install download only from the
[Releases page](https://github.com/malhaniah/zeiler/releases/latest). GitHub records a
SHA-256 digest for every release asset; `gh release view <tag> --json assets` shows them.

## How Zeiler is built to be safe

The full list is the "Security posture" section of
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). In short:

- Addon, skip, directory and account HTTP all run in Rust; the webview has a strict
  content security policy and can only call Zeiler's typed commands.
- Addon answers are untrusted JSON: stream and external URLs are scheme-allow-listed
  before they reach mpv or the operating system, and player properties and commands pass
  allow-lists.
- Addon transport URLs may embed credentials. They are never logged, never sent anywhere
  but the addon host, and masked in the UI until revealed.
- IPTV credentials and Stremio session keys live in the operating system's keychain, or in
  memory only when the keychain is unavailable, and never in a file.
- Subtitle downloads are http(s) only, size-capped, and refused unless the text starts as
  a subtitle format.
- The torrent engine's stream endpoint binds to 127.0.0.1 with per-session credentials.
- Dependency licences and sources are checked in CI with cargo-deny.
