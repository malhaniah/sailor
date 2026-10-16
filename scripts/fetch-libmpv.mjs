// Fetch a pinned libmpv build for the Windows embedded player and prepare
// src-tauri/libmpv/ for `cargo build --features embedded`:
//
//   libmpv-2.dll   runtime library (bundled beside zeiler.exe, see
//                  src-tauri/tauri.embedded.conf.json — the Windows release
//                  overlay: npm run tauri build -- --features embedded
//                    --config src-tauri/tauri.embedded.conf.json)
//   mpv.def        module-definition file, written here from the DLL's own
//                  export table (the archive ships only a MinGW libmpv.dll.a)
//   mpv.lib        MSVC import library, generated here via `lib /def:`
//   NOTICE.txt     licence notice, provenance (URL + hashes) and the written source
//                  offer for the vendored binaries; shipped in the installer as
//                  licenses/libmpv-NOTICE.txt, next to licenses/LGPL-2.1.txt (the
//                  licence text, committed at src-tauri/licenses/LGPL-2.1.txt)
//
// lib.exe comes from the MSVC build tools: the script finds it on PATH or through
// vswhere, so it works from a plain shell as well as a VS developer prompt.
//
// The archive is an LGPL-flavor libmpv from zhongfly/mpv-winbuild (shinchiro's
// build scripts; plain x86_64 — deliberately NOT the -v3 AVX2 build, which
// crashes on older CPUs). The download is pinned by SHA-256 — the hash below
// is from the release's own sha256.txt manifest; bump TAG/ASSET/SHA256 and the
// MPV_*/FFMPEG_* versions (as the new DLL reports them) together when updating,
// and THIRD-PARTY-NOTICES.md with them.
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
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const TAG = "2026-10-15-5757d69708";
const ASSET = "mpv-dev-lgpl-x86_64-20261015-git-5757d69708.7z";
const SHA256 = process.env.ZEILER_LIBMPV_SHA256 ??
  "5417be2641c31e2c5e676bb7cce90244edb43758936193c6c36dfcf5ea35eabd";
const URL =
  process.env.ZEILER_LIBMPV_URL ??
  `https://github.com/zhongfly/mpv-winbuild/releases/download/${TAG}/${ASSET}`;
// the versions the pinned DLL reports, and the commits they name
const MPV_VERSION = "v0.41.0-1184-g5757d6970";
const MPV_COMMIT = "5757d6970";
const FFMPEG_VERSION = "N-127570-gaf25036ac";
const FFMPEG_COMMIT = "af25036ac";
const ZEILER_REPO = "https://github.com/malhaniah/zeiler";

/**
 * The text of NOTICE.txt (installed as licenses/libmpv-NOTICE.txt): it travels with
 * the DLL, so it must stand on its own — the licence, where the exact sources are,
 * and the written offer. The installer puts the LGPL-2.1 text next to it.
 */
export function noticeText({ dllSha256, dllSize }) {
  return [
    "libmpv-2.dll — mpv and FFmpeg, as shipped with Zeiler",
    `(${ZEILER_REPO})`,
    "",
    "LICENCE",
    "libmpv-2.dll is mpv built in its LGPL configuration (-Dgpl=false) together with",
    "FFmpeg. Both are free software under the GNU Lesser General Public License,",
    "version 2.1 or (at your option) any later version. The licence text is in",
    "LGPL-2.1.txt, in the same folder as this file. The DLL also contains libraries",
    "these builds link statically (for example libass, libplacebo, FreeType,",
    "HarfBuzz, FriBidi, libbluray, libarchive, libssh); each keeps its own licence,",
    "as listed by the build scripts below.",
    "",
    "Zeiler links libmpv-2.dll dynamically: you may replace it, beside zeiler.exe,",
    "with any compatible libmpv-2.dll, including one you built yourself from",
    "modified sources.",
    "",
    "VERSIONS",
    `mpv     ${MPV_VERSION}`,
    `FFmpeg  ${FFMPEG_VERSION}`,
    ...(process.env.ZEILER_LIBMPV_URL
      ? ["(this copy came from an overridden archive URL; the versions and sources",
         " listed here are those of the pinned build and may not match it)"]
      : []),
    "",
    "SOURCE CODE",
    "The complete corresponding source code of this build:",
    `  mpv:     https://github.com/mpv-player/mpv/tree/${MPV_COMMIT}`,
    `  FFmpeg:  https://github.com/FFmpeg/FFmpeg/tree/${FFMPEG_COMMIT}`,
    `  build:   https://github.com/zhongfly/mpv-winbuild/tree/${TAG}`,
    "           (CI and package versions of this release), which drives",
    "           https://github.com/shinchiro/mpv-winbuild-cmake",
    "           (the source URL and patches of every bundled library)",
    "",
    "Written offer: for at least three years after the last Zeiler release that",
    "ships this build, Zeiler's maintainers will give anyone a copy of that complete",
    "corresponding source, for no more than the cost of providing it. Ask by opening",
    `an issue at ${ZEILER_REPO}/issues.`,
    "",
    "PROVENANCE",
    `archive:             ${URL}`,
    `archive sha256:      ${SHA256}`,
    `libmpv-2.dll sha256: ${dllSha256} (${dllSize} bytes)`,
    "Fetched and verified by scripts/fetch-libmpv.mjs. mpv.lib (build-time only, not",
    "shipped) is generated from the DLL's export table and contains no mpv code.",
    "",
    `All third-party notices: ${ZEILER_REPO}/blob/main/THIRD-PARTY-NOTICES.md`,
    "",
  ].join("\n");
}

async function main() {
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
    return;
  }

  // ---- skip when already vendored at this exact pin --------------------------
  const stampPath = join(destDir, "NOTICE.txt");
  const dllPath = join(destDir, "libmpv-2.dll");
  const libPath = join(destDir, "mpv.lib");
  const writeNotice = () => {
    const dll = readFileSync(dllPath);
    writeFileSync(stampPath, noticeText({ dllSha256: sha256(dll), dllSize: dll.length }));
  };
  if (
    !force &&
    existsSync(dllPath) &&
    existsSync(libPath) &&
    existsSync(stampPath) &&
    readFileSync(stampPath, "utf8").includes(SHA256)
  ) {
    // the notice text may have changed since it was written: always write it afresh
    writeNotice();
    log("libmpv-2.dll + mpv.lib already vendored at this pin — notice refreshed");
    return;
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
    if (!dllSrc) fail("libmpv-2.dll not found in the archive");

    mkdirSync(destDir, { recursive: true });
    cpSync(dllSrc, dllPath);
    // The archive has no .def (only a MinGW import library), so write one from the
    // DLL's export table. Its LIBRARY line names the DLL the import library points
    // at — without it `lib /def:` would point at "mpv.dll".
    let def;
    try {
      def = defFromDll(readFileSync(dllPath));
    } catch (e) {
      fail(`cannot read the exports of libmpv-2.dll: ${e.message}`);
    }
    writeFileSync(join(destDir, "mpv.def"), def);

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

    writeNotice();
    log(`done — vendored into ${destDir}`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
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

/**
 * A module-definition file for a PE/COFF DLL's `mpv_*` exports:
 *
 *   LIBRARY libmpv-2.dll
 *   EXPORTS
 *     mpv_abort_async_command
 *     …
 *
 * Reads the export directory straight from the file (no dumpbin needed). The DLL
 * also exports unrelated symbols of its bundled libraries (libbluray's JNI entry
 * points); only libmpv's own API goes into the import library.
 */
export function defFromDll(buf) {
  const u16 = (o) => buf.readUInt16LE(o);
  const u32 = (o) => buf.readUInt32LE(o);
  const cstr = (o) => {
    const end = buf.indexOf(0, o);
    if (end < 0) throw new Error("unterminated string");
    return buf.toString("latin1", o, end);
  };
  if (buf.toString("latin1", 0, 2) !== "MZ") throw new Error("not a PE file (no MZ header)");
  const pe = u32(0x3c);
  if (buf.toString("latin1", pe, pe + 4) !== "PE\0\0") throw new Error("no PE signature");
  const sections = u16(pe + 6);
  const optSize = u16(pe + 20);
  const opt = pe + 24;
  const magic = u16(opt);
  // data directory 0 = export table; its offset depends on PE32 vs PE32+
  const dirs = magic === 0x20b ? opt + 112 : magic === 0x10b ? opt + 96 : -1;
  if (dirs < 0) throw new Error(`unknown optional header magic 0x${magic.toString(16)}`);
  const exportRva = u32(dirs);
  if (!exportRva) throw new Error("the DLL has no export table");
  const table = opt + optSize;
  const offset = (rva) => {
    for (let i = 0; i < sections; i++) {
      const sec = table + i * 40;
      const va = u32(sec + 12);
      const size = Math.max(u32(sec + 8), u32(sec + 16));
      if (rva >= va && rva < va + size) return rva - va + u32(sec + 20);
    }
    throw new Error(`RVA 0x${rva.toString(16)} is in no section`);
  };
  const dir = offset(exportRva);
  const dllName = cstr(offset(u32(dir + 12)));
  const count = u32(dir + 24);
  const namesAt = offset(u32(dir + 32));
  const names = [];
  for (let i = 0; i < count; i++) {
    const name = cstr(offset(u32(namesAt + i * 4)));
    if (name.startsWith("mpv_")) names.push(name);
  }
  if (!names.includes("mpv_create") || !names.includes("mpv_client_api_version")) {
    throw new Error(`${dllName} does not export the libmpv client API`);
  }
  return ["LIBRARY " + dllName, "EXPORTS", ...names.map((n) => "    " + n), ""].join("\r\n");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
