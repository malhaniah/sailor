// Generate THIRD-PARTY-LICENSES.txt: the licences of the third-party code in one build
// of Zeiler. Every bundle ships it as licenses/THIRD-PARTY-LICENSES.txt (bundle.resources
// in src-tauri/tauri.conf.json).
//
//   Rust crates   cargo-about (about.toml) over Cargo.lock, filtered to the build's
//                 target and features, so crates of other platforms are left out.
//                 Zeiler's own crates are left out too.
//   npm packages  Vite's report of the packages bundled into the interface
//                 (build.license in vite.config.ts, dist/.vite/license.json), plus
//                 tailwindcss, whose base styles are compiled into the stylesheet and
//                 so are missing from that report (it only covers JavaScript).
//
// release.yml's `licenses` job runs it once per release target and hands each file to
// that target's build. Builds that skip it still compile: src-tauri/build.rs drops in a
// placeholder, since tauri_build refuses to build with a missing resource.
//
// Usage: npm run build && node scripts/third-party-licenses.mjs [options]
//   --target <triple>    default: the host's (rustc -vV)
//   --features <list>    features of the zeiler crate, as the build passes them
//   --out <file>         default: src-tauri/licenses/THIRD-PARTY-LICENSES.txt
// Needs cargo-about 0.9 on PATH: cargo install --locked cargo-about --features cli

import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ZEILER_REPO = "https://github.com/malhaniah/zeiler";
// npm packages whose code reaches the app without going through a JavaScript chunk
const EXTRA_NPM = ["tailwindcss"];

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const log = (msg) => console.log(`[third-party-licenses] ${msg}`);
const fail = (msg) => {
  console.error(`[third-party-licenses] ERROR: ${msg}`);
  process.exit(1);
};

function parseArgs(argv) {
  const opts = {};
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i].replace(/^--/, "");
    if (!["target", "features", "out"].includes(key) || i + 1 >= argv.length) {
      fail(`unknown or incomplete option ${argv[i]}`);
    }
    opts[key] = argv[++i];
  }
  return opts;
}

function run(cmd, args) {
  const res = spawnSync(cmd, args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
  if (res.error) fail(`${cmd}: ${res.error.message}`);
  if (res.status !== 0) fail(`${cmd} ${args.join(" ")} exited with ${res.status}`);
  return res.stdout;
}

function hostTarget() {
  const host = run("rustc", ["-vV"]).match(/^host: (\S+)$/m);
  if (!host) fail("could not read the host target from `rustc -vV`");
  return host[1];
}

/** Licence texts end in exactly one newline and never use CRLF. */
const tidy = (text) => `${text.replace(/\r\n?/g, "\n").replace(/\s+$/, "")}\n`;

/** `prefix` + the words, wrapped at 80 columns with a hanging indent. */
function wrap(prefix, words) {
  const lines = [];
  let line = prefix;
  for (const word of words) {
    if (line.length > prefix.length && line.length + 1 + word.length > 80) {
      lines.push(line);
      line = " ".repeat(prefix.length) + word;
    } else {
      line += (line.length > prefix.length ? " " : "") + word;
    }
  }
  lines.push(line);
  return lines.join("\n");
}
const paragraph = (text) => wrap("", text.split(/\s+/));
/** The items joined by ", " (an item never breaks across lines). */
const wrapList = (prefix, items) =>
  wrap(prefix, items.map((item, i) => (i < items.length - 1 ? `${item},` : item)));

const SOURCE_FILE = /\.(rs|h|c|cc|cpp|inl|S|s|pl|py|go|js)$/;

/**
 * cargo-about also takes notices from source-file headers (ring's ISC files, vendored
 * files in schemars_derive or libdbus-sys) and then gives the whole file as the text:
 * keep the comment block at the top, where the notice is, and drop the code.
 */
function noticeOf(text, sourcePath) {
  if (!SOURCE_FILE.test(sourcePath ?? "")) return text;
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  let inBlock = false;
  let end = lines.length;
  for (const [i, line] of lines.entries()) {
    const t = line.trim();
    if (inBlock) {
      inBlock = !t.includes("*/");
    } else if (t.startsWith("/*")) {
      inBlock = !t.slice(2).includes("*/");
    } else if (t !== "" && !/^\/\/(?![/!])/.test(t) && !/^#( |$)/.test(t)) {
      // code, or a doc comment (//!, ///), which documents the code rather than licensing it
      end = i;
      break;
    }
  }
  const header = lines.slice(0, end).join("\n");
  return /copyright|licen[cs]e|permission/i.test(header) ? header : text;
}

const RULE = "-".repeat(80);
const section = (title, users, text) =>
  [RULE, title, ...(users ? [wrapList("Used by: ", users)] : []), RULE, "", tidy(text)].join("\n");

/** Each distinct licence text once, with the crates that carry it. */
function rustLicences(target, features) {
  // through a file: under PowerShell, cargo-about refuses to write to a pipe
  const tmp = mkdtempSync(join(tmpdir(), "zeiler-about-"));
  const json = join(tmp, "about.json");
  const args = ["about", "generate", "--locked", "--fail", "--format", "json",
    "--manifest-path", join("src-tauri", "Cargo.toml"), "--target", target, "-o", json];
  if (features) args.push("--features", features);
  run("cargo", args);
  const about = JSON.parse(readFileSync(json, "utf8"));
  rmSync(tmp, { recursive: true, force: true });

  const byText = new Map();
  const crates = new Set();
  for (const lic of about.licenses) {
    // path crates (source null) are Zeiler's own, GPL-3.0-or-later
    const users = lic.used_by.filter((u) => u.crate.source != null);
    if (users.length === 0) continue;
    const text = tidy(noticeOf(lic.text, lic.source_path));
    const key = `${lic.id}\0${text}`;
    const entry = byText.get(key) ?? { id: lic.id, name: lic.name, text, users: new Set() };
    for (const { crate } of users) {
      entry.users.add(`${crate.name} ${crate.version}`);
      crates.add(crate.id);
    }
    byText.set(key, entry);
  }
  const entries = [...byText.values()].map((e) => ({ ...e, users: [...e.users].sort() }));
  entries.sort((a, b) => a.id.localeCompare(b.id) || a.users[0].localeCompare(b.users[0]));
  return { entries, crateCount: crates.size };
}

function licenceFile(dir) {
  const file = readdirSync(dir).find((f) => /^(licen[cs]e|copying)/i.test(f));
  return file ? readFileSync(join(dir, file), "utf8") : undefined;
}

/** The npm packages in the built interface, from Vite's report plus EXTRA_NPM. */
function npmLicences() {
  const report = join(root, "dist", ".vite", "license.json");
  if (!existsSync(report)) fail(`${report} is missing: run \`npm run build\` first`);
  const packages = JSON.parse(readFileSync(report, "utf8"));
  for (const name of EXTRA_NPM) {
    if (packages.some((p) => p.name === name)) continue;
    const dir = join(root, "node_modules", name);
    const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    packages.push({ name, version: pkg.version, identifier: pkg.license, text: licenceFile(dir) });
  }
  packages.sort((a, b) => a.name.localeCompare(b.name));
  for (const p of packages) {
    if (!p.text) fail(`npm package ${p.name} ${p.version} ships no licence file`);
  }
  return packages;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const target = opts.target ?? hostTarget();
  const out = resolve(root, opts.out ?? join("src-tauri", "licenses", "THIRD-PARTY-LICENSES.txt"));
  const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;

  const rust = rustLicences(target, opts.features);
  const npm = npmLicences();
  const build = opts.features ? `${target}, features: ${opts.features}` : target;
  // the Windows build with built-in playback also bundles libmpv-2.dll
  const libmpv = target.includes("windows") && /\bembedded\b/.test(opts.features ?? "");

  const text = [
    `Third-party licences in Zeiler ${version} (${build})`,
    "=".repeat(80),
    "",
    paragraph(
      "Zeiler is free software under the GNU General Public License, version 3 or " +
        `later. Its source code is at ${ZEILER_REPO}.`,
    ),
    "",
    paragraph(
      "This file lists the licences of the third-party code built into this copy of " +
        `Zeiler: the ${rust.crateCount} Rust crates in part 1 and the ${npm.length} npm ` +
        "packages of its interface in part 2. Each licence text names the crates or " +
        "the package it covers.",
    ),
    ...(libmpv
      ? [
          "",
          paragraph(
            "libmpv-2.dll (mpv and FFmpeg, for built-in playback) is not in this file. " +
              "See libmpv-NOTICE.txt (its versions, licence and source offer) and " +
              "libmpv-THIRD-PARTY.txt (the libraries inside it), next to this file.",
          ),
        ]
      : []),
    "",
    paragraph(
      "Generated by scripts/third-party-licenses.mjs from Cargo.lock (cargo-about) " +
        "and the interface's build (Vite).",
    ),
    "",
    "",
    `PART 1: RUST CRATES (${rust.crateCount})`,
    "=".repeat(80),
    "",
    ...rust.entries.map((e) => section(`${e.name} (${e.id})`, e.users, e.text)),
    "",
    `PART 2: NPM PACKAGES (${npm.length})`,
    "=".repeat(80),
    "",
    ...npm.map((p) => section(`${p.name} ${p.version} (${p.identifier ?? "see below"})`, null, p.text)),
  ].join("\n");

  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, text);
  log(`${out}: ${rust.crateCount} crates (${rust.entries.length} licence texts), ${npm.length} npm packages, ${build}`);
}

main();
