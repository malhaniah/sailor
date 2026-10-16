// The latest GitHub release of Zeiler: fetching it, checking it, and sorting its files.
// Shared by the browser (live data) and by vite.config.ts (the build-time fallback), so
// it touches no DOM.

export const REPO = "malhaniah/zeiler";
export const REPO_URL = `https://github.com/${REPO}`;
export const RELEASES_URL = `${REPO_URL}/releases`;
export const LATEST_API = `https://api.github.com/repos/${REPO}/releases/latest`;

// Every download link must point here, whatever the API returns
const DOWNLOAD_PREFIX = `${REPO_URL}/releases/download/`;

export interface Asset {
  name: string;
  size: number;
  url: string;
  /** "sha256:<hex>", when GitHub has computed one */
  digest: string | null;
}

export interface Release {
  tag: string;
  name: string;
  publishedAt: string;
  htmlUrl: string;
  body: string;
  assets: Asset[];
}

type Json = Record<string, unknown>;

const isObject = (v: unknown): v is Json => typeof v === "object" && v !== null;
const str = (v: unknown): string | null => (typeof v === "string" ? v : null);

/** The parts of a GitHub API release this site uses; throws if it isn't one. */
export function normalizeRelease(raw: unknown): Release {
  if (!isObject(raw)) throw new Error("release: not an object");
  const tag = str(raw.tag_name);
  const htmlUrl = str(raw.html_url);
  if (!tag || !htmlUrl || !htmlUrl.startsWith(`${RELEASES_URL}/`)) {
    throw new Error("release: missing tag_name or html_url");
  }
  if (raw.draft === true || raw.prerelease === true) throw new Error("release: not a published release");
  const assets: Asset[] = [];
  for (const a of Array.isArray(raw.assets) ? raw.assets : []) {
    if (!isObject(a)) continue;
    const name = str(a.name);
    const url = str(a.browser_download_url);
    if (!name || !url || !url.startsWith(DOWNLOAD_PREFIX)) continue;
    const digest = str(a.digest);
    assets.push({
      name,
      size: typeof a.size === "number" ? a.size : 0,
      url,
      digest: digest && /^sha256:[0-9a-f]{64}$/.test(digest) ? digest : null,
    });
  }
  return {
    tag,
    name: str(raw.name) || `Zeiler ${tag}`,
    publishedAt: str(raw.published_at) ?? "",
    htmlUrl,
    body: str(raw.body) ?? "",
    assets,
  };
}

/** The same shape back as the API's, for the committed snapshot. */
export function toApiShape(r: Release): Json {
  return {
    tag_name: r.tag,
    name: r.name,
    published_at: r.publishedAt,
    html_url: r.htmlUrl,
    draft: false,
    prerelease: false,
    body: r.body,
    assets: r.assets.map((a) => ({
      name: a.name,
      size: a.size,
      browser_download_url: a.url,
      digest: a.digest,
    })),
  };
}

export async function fetchLatestRelease(opts: { token?: string; timeoutMs?: number } = {}): Promise<Release> {
  const headers: Record<string, string> = {
    accept: "application/vnd.github+json",
    "x-github-api-version": "2022-11-28",
  };
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;
  const res = await fetch(LATEST_API, { headers, signal: AbortSignal.timeout(opts.timeoutMs ?? 8000) });
  if (!res.ok) throw new Error(`GitHub API: ${res.status} ${res.statusText}`);
  return normalizeRelease(await res.json());
}

// ---- Sorting the files ------------------------------------------------------------

export type Os = "windows" | "macos" | "linux";
export type Arch = "x64" | "arm64";
export type Kind = "setup" | "dmg" | "deb" | "rpm" | "appimage" | "updater" | "other";

export interface Download {
  asset: Asset;
  os: Os | null;
  kind: Kind;
  arch: Arch | null;
  /** Short, human name: "Apple Silicon", "Debian / Ubuntu", … */
  label: string;
  /** The file type shown next to the label: ".dmg", ".deb", … */
  ext: string;
}

function archOf(name: string): Arch | null {
  if (/(?:^|[_.-])(?:aarch64|arm64)(?:[_.-]|$)/i.test(name)) return "arm64";
  if (/(?:^|[_.-])(?:x64|x86_64|amd64)(?:[_.-]|$)/i.test(name)) return "x64";
  return null;
}

export function classify(asset: Asset): Download {
  const { name } = asset;
  const arch = archOf(name);
  const d = (os: Os | null, kind: Kind, label: string, ext: string): Download => ({ asset, os, kind, arch, label, ext });
  // updater packages and their signatures first: "*.app.tar.gz", "*.sig", latest.json
  if (/\.sig$/i.test(name) || /\.app\.tar\.gz$/i.test(name) || /\.(nsis|msi)\.zip$/i.test(name) || name === "latest.json") {
    return d(null, "updater", "Auto-update file", "");
  }
  if (/-setup\.exe$/i.test(name)) {
    return d("windows", "setup", arch === "arm64" ? "Windows on Arm" : "Windows", ".exe");
  }
  if (/\.dmg$/i.test(name)) {
    return d("macos", "dmg", arch === "arm64" ? "Apple Silicon" : arch === "x64" ? "Intel" : "Mac", ".dmg");
  }
  if (/\.deb$/i.test(name)) return d("linux", "deb", "Debian / Ubuntu", ".deb");
  if (/\.rpm$/i.test(name)) return d("linux", "rpm", "Fedora / openSUSE", ".rpm");
  if (/\.AppImage$/i.test(name)) return d("linux", "appimage", "AppImage (any distro)", ".AppImage");
  return d(null, "other", "Other", "");
}

const KIND_ORDER: Kind[] = ["setup", "dmg", "deb", "rpm", "appimage", "other", "updater"];

export function downloads(release: Release): Download[] {
  return release.assets
    .map(classify)
    .sort(
      (a, b) =>
        KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind) ||
        (a.arch === "arm64" ? 0 : 1) - (b.arch === "arm64" ? 0 : 1) ||
        a.asset.name.localeCompare(b.asset.name),
    );
}

/** The installers for one OS, in the order to offer them. */
export function installersFor(release: Release, os: Os): Download[] {
  return downloads(release).filter((d) => d.os === os);
}

export function find(release: Release, kind: Kind, arch?: Arch): Download | undefined {
  return downloads(release).find((d) => d.kind === kind && (arch === undefined || d.arch === arch));
}

// ---- Formatting -------------------------------------------------------------------

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(bytes < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}

export function formatDate(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString("en", { year: "numeric", month: "long", day: "numeric", timeZone: "UTC" });
}

/** "0.2.0" from "v0.2.0" */
export const version = (r: Release): string => r.tag.replace(/^v/, "");
