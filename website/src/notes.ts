// The install notes for each OS, taken from the release's own text (the RELEASE_BODY in
// .github/workflows/release.yml writes a "### Windows", "### macOS" and "### Linux"
// section into every release), so they change with each release. The copies below are
// used only when a release has no such section.

import type { Os } from "./release.ts";

export const FALLBACK_NOTES: Record<Os, string> = {
  windows: [
    "Download the `*-setup.exe` installer and run it (installs per-user, no admin). Builds are unsigned, so SmartScreen may warn: choose **More info → Run anyway**. Playback is built in. Optional: to play in your own mpv window instead (Settings → Player → External mpv), install mpv with `winget install shinchiro.mpv`.",
  ].join("\n"),
  macos: [
    "Download the `.dmg` matching your Mac (`aarch64` = Apple Silicon, `x64` = Intel) and drag Zeiler to Applications. Builds are not notarized: the first time, right-click Zeiler → **Open**, or allow it under System Settings → Privacy & Security. Install mpv with `brew install mpv`.",
  ].join("\n"),
  linux: [
    "- Debian/Ubuntu: `sudo apt install ./Zeiler_*_amd64.deb` (pulls in mpv automatically)",
    "- Fedora/openSUSE: install the `.rpm`, plus mpv (Fedora: from RPM Fusion)",
    "- Anything else: make the `.AppImage` executable and run it; install mpv from your distro",
  ].join("\n"),
};

const HEADINGS: Record<string, Os> = { windows: "windows", macos: "macos", "mac os": "macos", mac: "macos", linux: "linux" };

const osOf = (heading: string): Os | null => {
  const key = heading.toLowerCase();
  return Object.hasOwn(HEADINGS, key) ? HEADINGS[key] : null;
};

/** The body of each "### <OS>" section, up to the next heading or rule. */
export function installNotes(body: string): Record<Os, string> {
  const found: Partial<Record<Os, string[]>> = {};
  let current: string[] | null = null;
  for (const line of body.replace(/\r\n?/g, "\n").split("\n")) {
    const heading = /^#{1,6}\s+(.+?)\s*#*$/.exec(line.trim());
    if (heading) {
      const os = osOf(heading[1]);
      current = os && !found[os] ? (found[os] = []) : null;
      continue;
    }
    if (/^(-{3,}|\*{3,}|_{3,})$/.test(line.trim())) {
      current = null;
      continue;
    }
    current?.push(line);
  }
  const pick = (os: Os) => found[os]?.join("\n").trim() || FALLBACK_NOTES[os];
  return { windows: pick("windows"), macos: pick("macos"), linux: pick("linux") };
}

/** The release text without its install sections, which the page shows per OS. */
export function withoutInstallNotes(body: string): string {
  const out: string[] = [];
  let skipping = false;
  for (const line of body.replace(/\r\n?/g, "\n").split("\n")) {
    const heading = /^#{1,6}\s+(.+?)\s*#*$/.exec(line.trim());
    if (heading) skipping = osOf(heading[1]) !== null;
    else if (/^(-{3,}|\*{3,}|_{3,})$/.test(line.trim())) skipping = false;
    if (!skipping) out.push(line);
  }
  return out.join("\n").trim();
}
