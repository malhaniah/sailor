// Fetch a pinned libmpv build for the Windows embedded player and prepare
// src-tauri/libmpv/ for `cargo build --features embedded`:
//
//   libmpv-2.dll   runtime library (bundled beside zeiler.exe, see
//                  src-tauri/tauri.embedded.conf.json — an opt-in overlay:
//                  npm run tauri build -- --features embedded
//                    --config src-tauri/tauri.embedded.conf.json)
//   mpv.def        export list shipped in the archive
//   mpv.lib        MSVC import library, generated here via `lib /def:`
//   NOTICE.txt     provenance (URL + hashes) of the vendored binaries
//
// The archive is an LGPL-flavor libmpv from zhongfly/mpv-winbuild (shinchiro's
// build scripts; plain x86_64 — deliberately NOT the -v3 AVX2 build, which
// crashes on older CPUs). The download is pinned by SHA-256 — the hash below
// is from the release's own sha256.txt manifest; bump TAG/ASSET/SHA256
// together when updating.
//
// Usage: node scripts/fetch-libmpv.mjs [--force]
// Env:   ZEILER_LIBMPV_URL=...  override the archive URL (hash still enforced
//        unless ZEILER_LIBMPV_SHA256 is set too)

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const TAG = "2026-10-15-5757d69708";
const ASSET = "mpv-dev-lgpl-x86_64-20261015-git-5757d69708.7z";
const SHA256 = process.env.ZEILER_LIBMPV_SHA256 ??
  "5417be2641c31e2c5e676bb7cce90244edb43758936193c6c36dfcf5ea35eabd";
const URL =
  process.env.ZEILER_LIBMPV_URL ??
  `https://github.com/zhongfly/mpv-winbuild/releases/download/${TAG}/${ASSET}`;

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const destDir = join(root, "src-tauri", "libmpv");
const force = process.argv.includes("--force");
const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");
const log = (msg) => console.log(`[fetch-libmpv] ${msg}`);
const fail = (msg) => {
  console.error(`[fetch-libmpv] ERROR: ${msg}`);
  process.exit(1);
};

if (process.platform !== "win32") {
  log("not Windows — nothing to do (other platforms link a system libmpv)");
  process.exit(0);
}

// ---- skip when already vendored at this exact pin --------------------------
const stampPath = join(destDir, "NOTICE.txt");
const dllPath = join(destDir, "libmpv-2.dll");
const libPath = join(destDir, "mpv.lib");
if (
  !force &&
  existsSync(dllPath) &&
  existsSync(libPath) &&
  existsSync(stampPath) &&
  readFileSync(stampPath, "utf8").includes(SHA256)
) {
  log("libmpv-2.dll + mpv.lib already vendored at this pin — nothing to do");
  process.exit(0);
}

// ---- download + verify ------------------------------------------------------
// The archive is untrusted input: it goes into its own fresh temp directory and
// is hash-verified before anything touches it.
const work = mkdtempSync(join(tmpdir(), "zeiler-libmpv-"));
try {
  log(`fetching ${URL}`);
  const res = await fetch(URL, { redirect: "follow" });
  if (!res.ok) fail(`download failed: ${res.status} ${res.statusText}`);
  const buf = Buffer.from(await res.arrayBuffer());
  const got = sha256(buf);
  if (got !== SHA256) {
    fail(`checksum mismatch!\n  expected ${SHA256}\n  got      ${got}`);
  }
  log(`verified ${(buf.length / 1048576).toFixed(1)} MiB (sha256 ok)`);

  const archive = join(work, ASSET);
  writeFileSync(archive, buf);

  // ---- extract (Windows bsdtar reads 7z; fall back to a 7z.exe on PATH) ----
  const extractDir = join(work, "x");
  mkdirSync(extractDir);
  let r = spawnSync("tar", ["-xf", archive, "-C", extractDir], { stdio: "pipe" });
  if (r.status !== 0) {
    r = spawnSync("7z", ["x", "-y", `-o${extractDir}`, archive], { stdio: "pipe" });
    if (r.status !== 0) {
      fail("could not extract the .7z (neither bsdtar nor 7z worked). Install 7-Zip and retry.");
    }
  }

  const findFile = (dir, names) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, entry.name);
      if (entry.isDirectory()) {
        const hit = findFile(p, names);
        if (hit) return hit;
      } else if (names.includes(entry.name.toLowerCase())) {
        return p;
      }
    }
    return null;
  };
  const dllSrc = findFile(extractDir, ["libmpv-2.dll"]);
  const defSrc = findFile(extractDir, ["mpv.def", "libmpv.def"]);
  if (!dllSrc) fail("libmpv-2.dll not found in the archive");
  if (!defSrc) fail("mpv.def not found in the archive");

  mkdirSync(destDir, { recursive: true });
  cpSync(dllSrc, dllPath);
  cpSync(defSrc, join(destDir, "mpv.def"));

  // ---- generate the MSVC import library via `lib /def:` ---------------------
  const libExe = locateLibExe();
  if (!libExe) {
    fail(
      "lib.exe not found. Install the VS Build Tools C++ workload (it is required " +
        "for the Rust MSVC toolchain anyway), or run this from a VS developer shell.",
    );
  }
  log(`generating mpv.lib with ${libExe}`);
  const gen = spawnSync(
    libExe,
    [`/def:${join(destDir, "mpv.def")}`, "/machine:x64", `/out:${libPath}`, "/nologo"],
    { stdio: "pipe" },
  );
  if (gen.status !== 0 || !existsSync(libPath)) {
    fail(`lib.exe failed:\n${gen.stdout}${gen.stderr}`);
  }

  writeFileSync(
    stampPath,
    [
      "Vendored libmpv for the Windows embedded player (fetched by scripts/fetch-libmpv.mjs).",
      "",
      `source:  ${URL}`,
      `archive sha256: ${SHA256}`,
      `libmpv-2.dll sha256: ${sha256(readFileSync(dllPath))} (${statSync(dllPath).size} bytes)`,
      "",
      "libmpv is LGPL-2.1+ (this is zhongfly/mpv-winbuild's LGPL-flavor build of",
      "mpv + FFmpeg). mpv.lib is generated locally from mpv.def and contains no",
      "mpv code. See THIRD-PARTY-NOTICES.md at the repository root.",
      "",
    ].join("\n"),
  );
  log(`done — vendored into ${destDir}`);
} finally {
  rmSync(work, { recursive: true, force: true });
}

function locateLibExe() {
  const onPath = spawnSync("where", ["lib.exe"], { stdio: "pipe" });
  if (onPath.status === 0) {
    const first = onPath.stdout.toString().split(/\r?\n/)[0].trim();
    if (first) return first;
  }
  const vswhere = join(
    process.env["ProgramFiles(x86)"] ?? "C:/Program Files (x86)",
    "Microsoft Visual Studio/Installer/vswhere.exe",
  );
  if (!existsSync(vswhere)) return null;
  const found = spawnSync(
    vswhere,
    [
      "-latest",
      "-products", "*",
      "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
      "-find", "VC/Tools/MSVC/**/bin/Hostx64/x64/lib.exe",
    ],
    { stdio: "pipe" },
  );
  if (found.status !== 0) return null;
  const first = found.stdout.toString().split(/\r?\n/).filter(Boolean).pop();
  return first?.trim() || null;
}
