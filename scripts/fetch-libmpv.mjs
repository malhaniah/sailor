// Fetch a pinned libmpv build for the embedded player. On macOS, see the macOS
// section further down (src-tauri/libmpv-macos/). On Windows, it prepares
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
//                  licenses/libmpv-NOTICE.txt, next to the licence texts committed in
//                  src-tauri/licenses/: LGPL-2.1.txt (mpv), LGPL-3.0.txt and
//                  GPL-3.0.txt (FFmpeg, built with --enable-version3) and
//                  libmpv-THIRD-PARTY.txt (the libraries linked into the DLL)
//
// lib.exe comes from the MSVC build tools: the script finds it on PATH or through
// vswhere, so it works from a plain shell as well as a VS developer prompt.
//
// The archive is an LGPL-flavor libmpv from zhongfly/mpv-winbuild (shinchiro's
// build scripts; plain x86_64 — deliberately NOT the -v3 AVX2 build, which
// crashes on older CPUs). zhongfly deletes releases older than about 30 build
// days, so the script downloads Zeiler's own copy: the pre-release MIRROR_TAG of
// this repo holds the archive unchanged, zhongfly's sha256.txt, the build inputs
// and the LGPL sources, recorded in docs/libmpv/<TAG>.md. The download is pinned
// by SHA-256 — the hash below is from zhongfly's sha256.txt.
//
// To update: mirror the new build the same way (a pre-release, never "latest":
// the updater reads releases/latest), then bump TAG/ASSET/SHA256 and the
// MPV_*/FFMPEG_* versions (as the new DLL reports them) together, add its
// docs/libmpv/ record, and update THIRD-PARTY-NOTICES.md and
// src-tauri/licenses/libmpv-THIRD-PARTY.txt with them (the library list follows
// shinchiro's packages/*.cmake as zhongfly's compile-lgpl-libmpv.patch leaves
// them). Keep older mirrors: the source offer runs three years past the last
// release that shipped them.
//
// Usage: node scripts/fetch-libmpv.mjs [--force] [--arch arm64|x86_64 (macOS)]
// Env:   ZEILER_LIBMPV_URL=...  override the archive URL (hash still enforced
//        unless ZEILER_LIBMPV_SHA256 is set too)
//        ZEILER_LIBMPV_ARCHIVE=...  macOS: a local archive instead of the download
//        (e.g. one scripts/build-libmpv-macos.sh just built; hash enforced the same way)

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

const ZEILER_REPO = "https://github.com/malhaniah/zeiler";
// zhongfly/mpv-winbuild's release, and Zeiler's copy of it
const TAG = "2026-10-15-5757d69708";
const MIRROR_TAG = `libmpv-${TAG}`;
const MIRROR = `${ZEILER_REPO}/releases/tag/${MIRROR_TAG}`;
const ASSET = "mpv-dev-lgpl-x86_64-20261015-git-5757d69708.7z";
const SHA256 = process.env.ZEILER_LIBMPV_SHA256 ??
  "5417be2641c31e2c5e676bb7cce90244edb43758936193c6c36dfcf5ea35eabd";
const URL =
  process.env.ZEILER_LIBMPV_URL ??
  `${ZEILER_REPO}/releases/download/${MIRROR_TAG}/${ASSET}`;
// the versions the pinned DLL reports, and the commits they name
const MPV_VERSION = "v0.41.0-1184-g5757d6970";
const MPV_COMMIT = "5757d6970";
const FFMPEG_VERSION = "N-127570-gaf25036ac";
const FFMPEG_COMMIT = "af25036ac";

/**
 * The text of NOTICE.txt (installed as licenses/libmpv-NOTICE.txt): it travels with
 * the DLL, so it must stand on its own — the licence, where the exact sources are,
 * and the written offer. The installer puts the licence texts and
 * libmpv-THIRD-PARTY.txt next to it.
 */
export function noticeText({ dllSha256, dllSize }) {
  return [
    "libmpv-2.dll — mpv and FFmpeg, as shipped with Zeiler",
    `(${ZEILER_REPO})`,
    "",
    "LICENCE",
    "libmpv-2.dll is mpv, built in its LGPL configuration (-Dgpl=false), together",
    "with FFmpeg and the libraries both link statically.",
    "- mpv is free software under the GNU Lesser General Public License, version 2.1",
    "  or (at your option) any later version: LGPL-2.1.txt.",
    "- FFmpeg is built with --enable-version3 (and without --enable-gpl), so it is",
    "  under the GNU Lesser General Public License, version 3 or (at your option) any",
    "  later version: LGPL-3.0.txt, which builds on the GNU General Public License,",
    "  version 3: GPL-3.0.txt.",
    "- libmpv-2.dll as a whole is therefore distributed under the GNU Lesser General",
    "  Public License, version 3 or any later version.",
    "The other libraries inside the DLL (FreeType, HarfBuzz, libass, OpenSSL, dav1d,",
    "libvpx and more) keep their own licences. Their copyright notices and licence",
    "texts are in libmpv-THIRD-PARTY.txt. All of these files are in the same folder",
    "as this one.",
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
    "The complete corresponding source code of this build, kept by Zeiler:",
    `  ${MIRROR}`,
    "It holds the source of mpv, FFmpeg and every other LGPL library inside the DLL,",
    "at the exact commits built; the build scripts (zhongfly/mpv-winbuild, which",
    "drives shinchiro/mpv-winbuild-cmake: the source, options and patches of every",
    "bundled library); and BUILD-INPUTS.md, the commit of every library and the",
    "toolchain used.",
    "Upstream:",
    `  mpv:     https://github.com/mpv-player/mpv/tree/${MPV_COMMIT}`,
    `  FFmpeg:  https://github.com/FFmpeg/FFmpeg/tree/${FFMPEG_COMMIT}`,
    `  build:   zhongfly/mpv-winbuild release ${TAG}`,
    "",
    "Written offer: for at least three years after the last Zeiler release that",
    "ships this build, Zeiler's maintainers will give anyone a copy of that complete",
    "corresponding source, for no more than the cost of providing it. Ask by opening",
    `an issue at ${ZEILER_REPO}/issues.`,
    "",
    "PROVENANCE",
    `archive:             ${URL}`,
    ...(process.env.ZEILER_LIBMPV_URL
      ? []
      : [`                     (zhongfly/mpv-winbuild ${TAG}, unchanged)`]),
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

  if (process.platform === "darwin") {
    await fetchMacos({ root, force, sha256, log, fail });
    return;
  }
  if (process.platform !== "win32") {
    log("not Windows or macOS — nothing to do (Linux links the system libmpv)");
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

// ---- macOS -------------------------------------------------------------------------------
//
// The macOS app bundles libmpv and the libraries it loads under Contents/Frameworks.
// They are built from pinned sources by scripts/build-libmpv-macos.sh, in the
// libmpv-macos workflow (once per architecture, for macOS 12.0+), in mpv's LGPL
// configuration with an LGPL FFmpeg. Zeiler keeps each build on its own pre-release,
// MAC_MIRROR_TAG, together with every source archive it was built from, the build
// script and its record (docs/libmpv/macos-<MAC_MPV_VERSION>.md).
//
// For an architecture (the machine's own, or --arch), this writes
// src-tauri/libmpv-macos/<arch>/:
//   lib/*.dylib       the libraries (install names @rpath/<file>, ad-hoc signed), plus
//                     libmpv.dylib -> libmpv.2.dylib for the linker
//   licenses/         each component's licence files, as shipped in the archive
//   BUILD-INPUTS.md   the build's record, from the archive
//   NOTICE.txt        licence, versions, provenance and the written source offer;
//                     installed as Contents/Resources/licenses/libmpv-NOTICE.txt
//   tauri.conf.json   the overlay for `tauri build --features embedded --config <it>`:
//                     the libraries as bundle.macOS.frameworks, NOTICE.txt and the
//                     licence texts as resources
//
// To update: run the libmpv-macos workflow, mirror its output as a new pre-release
// (never "latest"), and bump MAC_MIRROR_TAG, the assets and their hashes, and the
// versions together; see scripts/build-libmpv-macos.sh.

const MAC_MPV_VERSION = "0.41.0";
const MAC_FFMPEG_VERSION = "9.0.2";
const MAC_MIRROR_TAG = `libmpv-macos-${MAC_MPV_VERSION}`;
const MAC_MIRROR = `${ZEILER_REPO}/releases/tag/${MAC_MIRROR_TAG}`;
// the archive per architecture, and its SHA-256 (from the workflow's SHA256SUMS-<arch>;
// docs/libmpv/macos-0.41.0.md records the run)
const MAC_ASSETS = {
  arm64: {
    asset: `libmpv-macos-${MAC_MPV_VERSION}-arm64.tar.gz`,
    sha256: "6150075c216dfe8c868ff0a4e60cd7d6f91e6f9df369019a83cf058889449d75",
  },
  x86_64: {
    asset: `libmpv-macos-${MAC_MPV_VERSION}-x86_64.tar.gz`,
    sha256: "938835375e1f39792a0d46e5b0c9fa62b6fbb690521486366c39cbac54275dc0",
  },
};

/** NOTICE.txt for the macOS bundle; like the Windows one, it must stand on its own. */
export function macNoticeText({ arch, archiveSha256, archiveUrl, libraries }) {
  return [
    "libmpv and the libraries it loads — as shipped with Zeiler for macOS",
    `(${ZEILER_REPO})`,
    "",
    "LICENCE",
    "Zeiler.app/Contents/Frameworks holds libmpv (mpv, built in its LGPL configuration,",
    "-Dgpl=false), FFmpeg (built without --enable-gpl and without --enable-version3),",
    "and the libraries they load, each as its own dynamic library.",
    "- mpv, FFmpeg, libplacebo and FriBidi are free software under the GNU Lesser General",
    "  Public License, version 2.1 or (at your option) any later version: LGPL-2.1.txt.",
    "- libass (ISC), FreeType (FreeType License), HarfBuzz (MIT) and dav1d (BSD-2-Clause)",
    "  keep their own licences; libplacebo contains fast_float (MIT), an OpenGL loader",
    "  generated by glad ((WTFPL OR CC0-1.0) AND Apache-2.0) and code built against the",
    "  Vulkan headers (MIT).",
    "Their copyright notices and licence texts are in libmpv-THIRD-PARTY.txt, in the",
    "same folder as this file.",
    "",
    "Zeiler links these libraries dynamically: you may replace them, in",
    "Zeiler.app/Contents/Frameworks, with compatible builds, including ones you built",
    "yourself from modified sources (then re-sign the app, e.g. `codesign --force",
    "--deep --sign - Zeiler.app`).",
    "",
    "VERSIONS",
    `mpv     ${MAC_MPV_VERSION}`,
    `FFmpeg  ${MAC_FFMPEG_VERSION}`,
    `built for ${arch}, macOS 12.0 and later; the version of every component is in`,
    "BUILD-INPUTS.md in the source release below.",
    ...(process.env.ZEILER_LIBMPV_URL || process.env.ZEILER_LIBMPV_ARCHIVE
      ? ["(this copy came from an overridden archive; the versions and sources listed",
         " here are those of the pinned build and may not match it)"]
      : []),
    "",
    "SOURCE CODE",
    "The complete corresponding source code of this build, kept by Zeiler:",
    `  ${MAC_MIRROR}`,
    "It holds the unchanged source archive of every component, the script that",
    "built them (scripts/build-libmpv-macos.sh: every option used) and the build's",
    "record (BUILD-INPUTS-<arch>.md: toolchain and the hash of every library).",
    "",
    "Written offer: for at least three years after the last Zeiler release that",
    "ships this build, Zeiler's maintainers will give anyone a copy of that complete",
    "corresponding source, for no more than the cost of providing it. Ask by opening",
    `an issue at ${ZEILER_REPO}/issues.`,
    "",
    "PROVENANCE",
    `archive:        ${archiveUrl}`,
    `archive sha256: ${archiveSha256}`,
    ...libraries.map(({ name, sha256: sum }) => `${name}: ${sum}`),
    "Fetched and verified by scripts/fetch-libmpv.mjs.",
    "",
    `All third-party notices: ${ZEILER_REPO}/blob/main/THIRD-PARTY-NOTICES.md`,
    "",
  ].join("\n");
}

/** The bundle overlay: the libraries into Contents/Frameworks, the notices into Resources. */
export function macTauriConf({ arch, libraries }) {
  const dir = `libmpv-macos/${arch}`;
  return {
    $schema: "https://schema.tauri.app/config/2",
    bundle: {
      macOS: { frameworks: libraries.map(({ name }) => `${dir}/lib/${name}`) },
      resources: {
        [`${dir}/NOTICE.txt`]: "licenses/libmpv-NOTICE.txt",
        "licenses/LGPL-2.1.txt": "licenses/LGPL-2.1.txt",
        "licenses/libmpv-macos-THIRD-PARTY.txt": "licenses/libmpv-THIRD-PARTY.txt",
      },
    },
  };
}

async function fetchMacos({ root, force, sha256, log, fail }) {
  const argArch = process.argv.includes("--arch")
    ? process.argv[process.argv.indexOf("--arch") + 1]
    : process.arch === "arm64" ? "arm64" : "x86_64";
  const pin = MAC_ASSETS[argArch];
  if (!pin) fail(`unknown architecture ${argArch} (arm64 or x86_64)`);
  const arch = argArch;
  const expected = process.env.ZEILER_LIBMPV_SHA256 ?? pin.sha256;
  const local = process.env.ZEILER_LIBMPV_ARCHIVE;
  const url = process.env.ZEILER_LIBMPV_URL ??
    `${ZEILER_REPO}/releases/download/${MAC_MIRROR_TAG}/${pin.asset}`;
  const dest = join(root, "src-tauri", "libmpv-macos", arch);
  const stamp = join(dest, "NOTICE.txt");

  const libraries = () =>
    readdirSync(join(dest, "lib"), { withFileTypes: true })
      .filter((e) => e.isFile() && e.name.endsWith(".dylib"))
      .map((e) => e.name)
      .sort()
      .map((name) => ({ name, sha256: sha256(readFileSync(join(dest, "lib", name))) }));
  const writeOverlay = (archiveSha256) => {
    const libs = libraries();
    writeFileSync(stamp, macNoticeText({ arch, archiveSha256, archiveUrl: local ?? url, libraries: libs }));
    writeFileSync(join(dest, "tauri.conf.json"), JSON.stringify(macTauriConf({ arch, libraries: libs }), null, 2) + "\n");
  };

  if (!force && existsSync(join(dest, "lib", "libmpv.2.dylib")) && existsSync(stamp) &&
      readFileSync(stamp, "utf8").includes(`archive sha256: ${expected}`)) {
    writeOverlay(expected);
    log(`macOS libmpv (${arch}) already vendored at this pin — notice refreshed`);
    return;
  }

  // untrusted input: its own fresh temp directory, hash-verified before it is unpacked
  const work = mkdtempSync(join(tmpdir(), "zeiler-libmpv-macos-"));
  try {
    let buf;
    if (local) {
      log(`reading ${local}`);
      buf = readFileSync(local);
    } else {
      log(`fetching ${url}`);
      const res = await fetch(url, { redirect: "follow" });
      if (!res.ok) fail(`download failed: ${res.status} ${res.statusText}`);
      buf = Buffer.from(await res.arrayBuffer());
    }
    const got = sha256(buf);
    if (got !== expected) fail(`checksum mismatch!\n  expected ${expected}\n  got      ${got}`);
    log(`verified ${(buf.length / 1048576).toFixed(1)} MiB (sha256 ok)`);

    const archive = join(work, "libmpv.tar.gz");
    writeFileSync(archive, buf);
    const x = join(work, "x");
    mkdirSync(x);
    const r = spawnSync("tar", ["-xzf", archive, "-C", x], { stdio: "pipe" });
    if (r.status !== 0) fail(`could not unpack the archive: ${r.stderr}`);
    if (!existsSync(join(x, "lib", "libmpv.2.dylib"))) fail("lib/libmpv.2.dylib not in the archive");
    // every library may load only the bundle's own (@rpath) and the system's, and
    // search no other place; the only link is the one the linker uses
    for (const entry of readdirSync(join(x, "lib"), { withFileTypes: true })) {
      const name = entry.name;
      if (entry.isSymbolicLink()) {
        if (name !== "libmpv.dylib") fail(`unexpected link lib/${name} in the archive`);
        continue;
      }
      const otool = spawnSync("otool", ["-L", join(x, "lib", name)], { stdio: "pipe" });
      const deps = otool.stdout.toString().split("\n").slice(1).map((l) => l.trim().split(" ")[0]).filter(Boolean);
      const stray = deps.filter((d) => !/^(@rpath\/|\/usr\/lib\/|\/System\/Library\/)/.test(d));
      if (stray.length) fail(`${name} loads libraries from outside the bundle: ${stray.join(", ")}`);
      const load = spawnSync("otool", ["-l", join(x, "lib", name)], { stdio: "pipe" }).stdout.toString();
      const rpaths = [...load.matchAll(/cmd LC_RPATH\n\s+cmdsize \d+\n\s+path (\S+)/g)].map((m) => m[1]);
      const strayRpaths = rpaths.filter((r) => r !== "/usr/lib/swift");
      if (strayRpaths.length) fail(`${name} searches outside the bundle: ${strayRpaths.join(", ")}`);
    }

    rmSync(dest, { recursive: true, force: true });
    mkdirSync(dest, { recursive: true });
    cpSync(x, dest, { recursive: true, verbatimSymlinks: true });
    writeOverlay(expected);
    log(`done — vendored into ${dest}`);
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
