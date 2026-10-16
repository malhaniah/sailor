import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { type Plugin, defineConfig } from "vite";
import { type Release, fetchLatestRelease, normalizeRelease, toApiShape } from "./src/release.ts";
import { releaseLine, renderCta, renderDownloads } from "./src/render.ts";

const root = import.meta.dirname;

// The release the pages are built with: the latest one from the GitHub API, or, when
// the build can't reach it, the committed release-snapshot.json. Either way the pages
// work without JavaScript, and in the browser they swap in the live release.
async function buildRelease(): Promise<Release> {
  if (process.env.ZEILER_OFFLINE !== "1") {
    try {
      return await fetchLatestRelease({ token: process.env.GITHUB_TOKEN });
    } catch (e) {
      console.warn(`[release] GitHub API unavailable (${(e as Error).message}); using release-snapshot.json`);
    }
  }
  return normalizeRelease(JSON.parse(readFileSync(resolve(root, "release-snapshot.json"), "utf8")));
}

// The release as JSON inside the page, in the API's shape; "<" escaped so it can't close the <script>
const dataScript = (r: Release) =>
  `<script type="application/json" id="release-data">${JSON.stringify(toApiShape(r)).replace(/</g, "\\u003c")}</script>`;

// Only GitHub's API is ever fetched; no inline scripts, no third parties
const CSP = [
  "default-src 'self'",
  "connect-src 'self' https://api.github.com",
  "img-src 'self' data:",
  "style-src 'self'",
  "script-src 'self'",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'none'",
].join("; ");

function release(): Plugin {
  let data: Promise<Release> | undefined;
  let isBuild = false;
  return {
    name: "zeiler-release",
    configResolved(config) {
      isBuild = config.command === "build";
    },
    transformIndexHtml: {
      order: "pre",
      async handler(html, ctx) {
        const r = await (data ??= buildRelease());
        // relative, so the site works at a domain's root and under a path alike
        const downloadsHref = ctx.path.startsWith("/download/") ? "./" : "download/";
        const out = html
          .replace("<!--release:cta-->", renderCta(r, null, { downloadsHref }))
          .replace("<!--release:line-->", releaseLine(r))
          .replace("<!--release:downloads-->", renderDownloads(r))
          .replace("<!--release:data-->", dataScript(r));
        return isBuild
          ? out.replace("<!--csp-->", `<meta http-equiv="Content-Security-Policy" content="${CSP}">`)
          : out;
      },
    },
  };
}

export default defineConfig({
  // "/" for zeiler.io; SITE_BASE=/zeiler/ for a GitHub Pages project URL
  base: process.env.SITE_BASE ?? "/",
  plugins: [release()],
  server: { port: 5180 },
  preview: { port: 5181 },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(root, "index.html"),
        download: resolve(root, "download/index.html"),
      },
    },
  },
});
