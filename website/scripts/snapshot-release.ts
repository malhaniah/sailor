// Refreshes release-snapshot.json, the copy of the latest release that a build uses
// when it cannot reach the GitHub API. Run it after each release: `npm run snapshot`.
// GITHUB_TOKEN, when set, avoids the API's low anonymous rate limit.

import { writeFileSync } from "node:fs";
import { fetchLatestRelease, toApiShape } from "../src/release.ts";

const out = new URL("../release-snapshot.json", import.meta.url);
const release = await fetchLatestRelease({ token: process.env.GITHUB_TOKEN });
writeFileSync(out, `${JSON.stringify(toApiShape(release), null, 2)}\n`);
console.log(`release-snapshot.json: ${release.tag} with ${release.assets.length} files`);
