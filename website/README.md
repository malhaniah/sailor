# zeiler.io

The download website for Zeiler: a home page with a download button for the visitor's
system and a product tour, and a downloads page with every file of the latest release.
It is a static site built with [Vite](https://vite.dev) and plain TypeScript: no
framework, no third-party requests except GitHub's API, and nothing to run on a server.

```bash
cd website
npm install
npm run dev        # http://localhost:5180
```

| Command | What it does |
| --- | --- |
| `npm run dev` | Dev server with live reload |
| `npm run build` | Typechecks, then builds the site into `dist/` |
| `npm run preview` | Serves the built `dist/` at http://localhost:5181 |
| `npm test` | Unit tests (Node's test runner): OS detection, file sorting, release notes |
| `npm run snapshot` | Refreshes `release-snapshot.json` from the latest release |

Node 22.18 or newer (the tests and the snapshot script run TypeScript directly).

## How the release data stays current

The pages never hard-code a version or a file name.

1. **At build time**, `vite.config.ts` reads [releases/latest] from the GitHub API and
   writes the download buttons, the file table and the install notes into the HTML, so
   the pages work without JavaScript. If the API can't be reached (offline, rate
   limited), it uses the committed `release-snapshot.json` instead. Set `GITHUB_TOKEN`
   to lift the anonymous rate limit, or `ZEILER_OFFLINE=1` to always use the snapshot.
2. **In the browser**, the page asks the same API again (at most once per tab every 15
   minutes) and redraws if a newer release is out. So a site built before a release
   offers the new version without a rebuild.

`releases/latest` skips drafts and pre-releases, so the site follows exactly what the
app's updater follows. Only links into `github.com/malhaniah/zeiler/releases/download/`
are ever shown.

The install notes for each system are the `### Windows`, `### macOS` and `### Linux`
sections of the release's own text (`RELEASE_BODY` in
[`.github/workflows/release.yml`](../.github/workflows/release.yml)). If a release has
none, the site falls back to the copies in `src/notes.ts`.

After a release, refresh the snapshot and commit it when convenient:

```bash
npm run snapshot
```

[releases/latest]: https://docs.github.com/en/rest/releases/releases#get-the-latest-release

## Which download a visitor gets

`src/detect.ts` works it out from the browser:

- **Windows** gets the `*-setup.exe` installer.
- **macOS** gets the `.dmg` for Apple Silicon (`aarch64`) or Intel (`x64`). Every Mac
  browser claims to be "Intel Mac OS X", so the user agent can't tell. Chromium-based
  browsers say the CPU through User-Agent Client Hints, and the WebGL renderer name
  ("Apple M2") gives it away in some others. Safari tells neither: there, and wherever
  else it's unknown, both are offered side by side, with how to check which Mac you have.
- **Linux** gets `.deb`, `.rpm` and `.AppImage`. When the user agent names the distro
  (Firefox on Ubuntu or Fedora does), the matching package is the main button.
- **Phones, tablets, ChromeOS** and anything unknown get a link to all downloads.

## Layout

```
website/
  index.html            home page and product tour
  download/index.html   downloads page
  release-snapshot.json the build's fallback release
  public/               copied as is: icons, tour screenshots
  scripts/              snapshot-release.ts
  src/
    release.ts          GitHub API, checks, file sorting (shared by build and browser)
    render.ts           the release-dependent HTML (shared by build and browser)
    detect.ts           OS and CPU detection
    notes.ts            install notes from the release text
    markdown.ts         a small, escaping Markdown renderer for release notes
    home.ts, download.ts, live.ts, common.ts   the browser side
    style.css
```

## Screenshots

The tour's screenshots in `public/tour/` come from the app's own interface running in a
browser with Tauri's IPC mocked (the method in `docs/plan/HANDOFF.md`, "How each
milestone was built"), with invented titles and abstract placeholder artwork. Never use
posters or names of real films or shows, and never show a real stream source: Zeiler
ships no content. To retake them, serve a mock page from a git-ignored `smoke.local/`
folder at the repository root and capture the app at 1440×900.

## Deploying

The build output is `website/dist/`: plain files for any static host. The built pages
carry a Content-Security-Policy `<meta>` tag allowing only the site itself and
`api.github.com`. All links are relative, so the site also works under a path; set
`SITE_BASE` (for example `SITE_BASE=/zeiler/`) when it is served from one, so the asset
URLs match.

Nothing is deployed yet and the domain isn't configured. Three good options, all free
for a site like this:

### GitHub Pages

1. Add a workflow such as `.github/workflows/website-pages.yml`:

   ```yaml
   name: website-pages
   on:
     push:
       branches: [main]
       paths: ["website/**"]
     release:
       types: [published]   # refreshes the no-JavaScript fallback after each release
     workflow_dispatch:
   permissions:
     contents: read
     pages: write
     id-token: write
   concurrency:
     group: pages
     cancel-in-progress: true
   jobs:
     deploy:
       runs-on: ubuntu-latest
       environment:
         name: github-pages
         url: ${{ steps.deployment.outputs.page_url }}
       defaults:
         run:
           working-directory: website
       steps:
         - uses: actions/checkout@v7
         - uses: actions/setup-node@v7
           with:
             node-version: 24
             cache: npm
             cache-dependency-path: website/package-lock.json
         - run: npm ci
         - run: npm run build
           env:
             GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
         - uses: actions/upload-pages-artifact@v4
           with:
             path: website/dist
         - id: deployment
           uses: actions/deploy-pages@v4
   ```

2. Repository Settings → Pages → Source: **GitHub Actions**.
3. Settings → Pages → Custom domain: `zeiler.io`, then **Enforce HTTPS**. At the DNS
   provider, point the apex at GitHub Pages (`A` records `185.199.108.153`,
   `185.199.109.153`, `185.199.110.153`, `185.199.111.153`; `AAAA` records
   `2606:50c0:8000::153` to `2606:50c0:8003::153`) and `www` with a `CNAME` to
   `malhaniah.github.io`. Verifying the domain under the account's Pages settings
   protects it from takeover.
4. Until the domain is set, the site lives at `malhaniah.github.io/zeiler/`: build with
   `SITE_BASE: /zeiler/` in the build step's `env` for that.

### Cloudflare Pages

1. Cloudflare dashboard → Workers & Pages → Create → Pages → connect the GitHub
   repository.
2. Build settings: framework preset **None**, root directory `website`, build command
   `npm run build`, output directory `dist`. Environment variable `NODE_VERSION` = `24`.
3. Custom domains → add `zeiler.io` (and `www.zeiler.io`). With the domain's DNS on
   Cloudflare this is one click; otherwise add the `CNAME` it shows.
4. Pull requests get preview deployments automatically. For a rebuild after each
   release, create a deploy hook and call it from a `release: published` workflow.

### Vercel

1. vercel.com → Add New → Project → import the repository.
2. Root Directory `website`; the Vite preset fills in build command `npm run build` and
   output `dist`.
3. Project → Settings → Domains → add `zeiler.io` and follow the DNS records it asks for
   (an `A` record for the apex, a `CNAME` for `www`).
4. Same as Cloudflare: previews per pull request; a deploy hook for rebuilds after
   releases.

Any of them can also send security headers (for example `X-Content-Type-Options:
nosniff` and `Referrer-Policy: strict-origin-when-cross-origin`) through a `_headers`
file (Cloudflare) or `vercel.json` (Vercel); GitHub Pages can't set headers.

## CI

[`.github/workflows/website.yml`](../.github/workflows/website.yml) runs the tests and
the build for changes under `website/` only, separately from the app's CI, and keeps
the built site as an artifact. It deploys nothing.
