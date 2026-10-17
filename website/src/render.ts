// HTML for the release-dependent parts of the pages, as strings: vite.config.ts writes
// them into the built pages (so they work without JavaScript), and the browser renders
// them again when it has the live release or knows the visitor's OS.

import type { DetectedArch, Platform } from "./detect.ts";
import { escapeHtml as esc, renderMarkdown } from "./markdown.ts";
import { installNotes, withoutInstallNotes } from "./notes.ts";
import {
  type Download,
  type Os,
  type Release,
  RELEASES_URL,
  downloads,
  find,
  formatDate,
  formatSize,
  installersFor,
  version,
} from "./release.ts";

const ICON_DOWNLOAD =
  '<svg class="icon" aria-hidden="true" viewBox="0 0 24 24" width="20" height="20"><path d="M12 3v12m0 0 5-5m-5 5-5-5M5 21h14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>';

export const OS_NAME: Record<Os, string> = { windows: "Windows", macos: "macOS", linux: "Linux" };

export const REQUIREMENTS: Record<Os, string> = {
  windows: "Windows 10 or 11, 64-bit. Installs for your user only, no admin needed.",
  macos: "macOS 12 Monterey or later, on Apple Silicon or Intel.",
  linux: "64-bit x86 (x86_64) with glibc 2.35 or later (Ubuntu 22.04 and newer).",
};

export function releaseLine(r: Release): string {
  const date = formatDate(r.publishedAt);
  return `Version ${esc(version(r))}${date ? ` · released <time datetime="${esc(r.publishedAt)}">${esc(date)}</time>` : ""}`;
}

function button(d: Download, text: string, primary: boolean, detail?: string): string {
  return `<a class="btn ${primary ? "btn-primary" : "btn-secondary"}" href="${esc(d.asset.url)}" data-file="${esc(d.asset.name)}">${ICON_DOWNLOAD}<span class="btn-text"><span class="btn-label">${text}</span><span class="btn-detail">${esc(detail ?? `${d.ext} · ${formatSize(d.asset.size)}`)}</span></span></a>`;
}

// ---- The hero's download box -------------------------------------------------------

export interface CtaOptions {
  /** where the downloads page is, relative to this page */
  downloadsHref: string;
}

const missing = (os: string, o: CtaOptions) =>
  `<p class="cta-note">This release has no ${os} download yet. See <a href="${o.downloadsHref}">all downloads</a> or <a href="${RELEASES_URL}">every release on GitHub</a>.</p>`;

function ctaWindows(r: Release, o: CtaOptions): string {
  const exe = find(r, "setup", "x64") ?? find(r, "setup");
  if (!exe) return missing("Windows", o);
  return `${button(exe, "Download for Windows", true, `Windows 10 and 11 · 64-bit · ${formatSize(exe.asset.size)}`)}
<p class="cta-note">Builds aren’t code-signed yet: if SmartScreen warns, choose <strong>More info → Run anyway</strong>. Playback is built in.</p>`;
}

const MAC_NOTE =
  '<p class="cta-note">The first time, right-click Zeiler → <strong>Open</strong> (builds aren’t notarized yet). Playback needs <a href="https://mpv.io">mpv</a>: <code>brew install mpv</code>.</p>';

function ctaMac(r: Release, arch: DetectedArch, o: CtaOptions): string {
  const arm = find(r, "dmg", "arm64");
  const intel = find(r, "dmg", "x64");
  if (!arm && !intel) return missing("macOS", o);
  const armBtn = (primary: boolean) => arm && button(arm, "Download for Apple Silicon", primary, `M1 and later · .dmg · ${formatSize(arm.asset.size)}`);
  const intelBtn = (primary: boolean) => intel && button(intel, "Download for Intel Mac", primary, `.dmg · ${formatSize(intel.asset.size)}`);
  if (arch === "arm" && arm) {
    return `${armBtn(true)}
<p class="cta-alt">Got an Intel Mac? ${intel ? `<a href="${esc(intel.asset.url)}">Download the Intel version</a>` : "No Intel build in this release."}</p>${MAC_NOTE}`;
  }
  if (arch === "x86" && intel) {
    return `${intelBtn(true)}
<p class="cta-alt">Got an Apple Silicon Mac? ${arm ? `<a href="${esc(arm.asset.url)}">Download the Apple Silicon version</a>` : "No Apple Silicon build in this release."}</p>${MAC_NOTE}`;
  }
  return `<div class="btn-row">${armBtn(true) || ""}${intelBtn(true) || ""}</div>
<p class="cta-alt">Not sure which Mac you have? Apple menu → <strong>About This Mac</strong>: a “Chip” line means Apple Silicon, a “Processor” line means Intel.</p>${MAC_NOTE}`;
}

function ctaLinux(r: Release, p: Platform, o: CtaOptions): string {
  const list = installersFor(r, "linux").filter((d) => d.arch !== "arm64");
  if (!list.length) return missing("Linux", o);
  const preferred = p.distro === "debian" ? "deb" : p.distro === "fedora" || p.distro === "suse" ? "rpm" : null;
  const buttons = list.map((d) => button(d, d.label, d.kind === preferred || !preferred)).join("");
  const armNote =
    p.arch === "arm" ? '<p class="cta-note">Zeiler’s Linux builds are for 64-bit x86 only so far; there is no ARM build yet.</p>' : "";
  return `<div class="btn-row">${buttons}</div>${armNote}
<p class="cta-note">Playback needs <a href="https://mpv.io">mpv</a>: the <code>.deb</code> installs it for you; otherwise install it from your distro.</p>`;
}

function ctaOther(p: Platform, o: CtaOptions): string {
  const where =
    p.os === "ios" || p.os === "android"
      ? "Zeiler is a desktop app for Windows, macOS and Linux: open this page on your computer to get it."
      : p.os === "chromeos"
        ? "Zeiler has no ChromeOS build. With Linux turned on in ChromeOS, the Linux .deb may work."
        : "Zeiler runs on Windows, macOS and Linux.";
  return `<a class="btn btn-primary" href="${o.downloadsHref}">${ICON_DOWNLOAD}<span class="btn-text"><span class="btn-label">See all downloads</span><span class="btn-detail">Windows · macOS · Linux</span></span></a>
<p class="cta-note">${esc(where)}</p>`;
}

/** The download box for one platform; `null` for the page as built, before detection. */
export function renderCta(r: Release, p: Platform | null, o: CtaOptions): string {
  let body: string;
  if (!p) body = ctaOther({ os: "unknown", arch: "unknown", distro: null }, o);
  else if (p.os === "windows") body = ctaWindows(r, o);
  else if (p.os === "macos") body = ctaMac(r, p.arch, o);
  else if (p.os === "linux") body = ctaLinux(r, p, o);
  else body = ctaOther(p, o);
  const others = p && ["windows", "macos", "linux"].includes(p.os) ? `<p class="cta-more"><a href="${o.downloadsHref}">Other platforms and all files</a></p>` : "";
  return `${body}
<p class="cta-version">${releaseLine(r)} · Free and open source (GPL-3.0)</p>${others}`;
}

// ---- The downloads page -----------------------------------------------------------

function osSection(r: Release, os: Os, notes: string): string {
  const list = installersFor(r, os);
  const files = list.length
    ? `<div class="btn-row">${list.map((d) => button(d, os === "macos" ? `${d.label}` : os === "windows" ? "64-bit installer" : d.label, true)).join("")}</div>`
    : `<p>This release has no ${OS_NAME[os]} download. See <a href="${RELEASES_URL}">every release on GitHub</a>.</p>`;
  return `<section class="os-card" id="${os}" aria-labelledby="${os}-title" data-os="${os}">
<div class="os-card-head"><h2 id="${os}-title">${OS_NAME[os]}</h2><span class="badge" hidden>Your system</span></div>
<p class="os-req">${esc(REQUIREMENTS[os])}</p>
${files}
<div class="os-notes">
<h3>Installing</h3>
${renderMarkdown(notes, { headingShift: 2 })}
</div>
</section>`;
}

const FOR: Record<string, string> = {
  setup: "Windows installer",
  dmg: "macOS disk image",
  deb: "Debian / Ubuntu package",
  rpm: "Fedora / openSUSE package",
  appimage: "Linux AppImage",
};

function fileRow(d: Download): string {
  const what =
    d.kind === "updater"
      ? "Used by auto-update"
      : `${FOR[d.kind] ?? "Other"}${d.arch ? ` (${d.arch === "arm64" ? (d.os === "macos" ? "Apple Silicon" : "ARM64") : d.os === "macos" ? "Intel" : "x64"})` : ""}`;
  const hash = d.asset.digest?.slice("sha256:".length);
  return `<tr>
<td data-label="File"><a href="${esc(d.asset.url)}">${esc(d.asset.name)}</a></td>
<td data-label="For">${esc(what)}</td>
<td data-label="Size" class="num">${esc(formatSize(d.asset.size))}</td>
<td data-label="SHA-256">${hash ? `<code class="hash" title="${hash}">${hash}</code><button type="button" class="copy" data-copy="${hash}" hidden>Copy<span class="visually-hidden"> SHA-256 of ${esc(d.asset.name)}</span></button>` : "—"}</td>
</tr>`;
}

function fileTable(rows: Download[], caption: string): string {
  return `<table class="files">
<caption>${caption}</caption>
<thead><tr><th scope="col">File</th><th scope="col">For</th><th scope="col" class="num">Size</th><th scope="col">SHA-256</th></tr></thead>
<tbody>${rows.map(fileRow).join("")}</tbody>
</table>`;
}

export function renderDownloads(r: Release): string {
  const notes = installNotes(r.body);
  const all = downloads(r);
  const installers = all.filter((d) => d.kind !== "updater");
  const updater = all.filter((d) => d.kind === "updater");
  return `<div class="os-grid">
${(["windows", "macos", "linux"] as const).map((os) => osSection(r, os, notes[os])).join("\n")}
</div>
<section class="all-files" aria-labelledby="all-files-title">
<h2 id="all-files-title">All files in ${esc(r.name)}</h2>
${fileTable(installers, "Installers")}
${updater.length ? `<details class="updater-files"><summary>Auto-update files (${updater.length})</summary><p>Installed copies of Zeiler fetch these by themselves. The <code>.sig</code> files are signatures that Zeiler checks before it installs an update.</p>${fileTable(updater, "Auto-update files")}</details>` : ""}
<p><a href="${esc(r.htmlUrl)}">${esc(r.name)} on GitHub</a> · <a href="${RELEASES_URL}">All releases</a></p>
</section>
<section class="release-notes" aria-labelledby="release-notes-title">
<h2 id="release-notes-title">Release notes</h2>
<div class="prose">${renderMarkdown(withoutInstallNotes(r.body), { headingShift: 1 })}</div>
</section>`;
}
