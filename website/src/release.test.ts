import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { classify, downloads, find, formatSize, normalizeRelease } from "./release.ts";

const snapshot = JSON.parse(readFileSync(new URL("../release-snapshot.json", import.meta.url), "utf8"));
const asset = (name: string) => ({ name, size: 1, url: `https://github.com/malhaniah/zeiler/releases/download/v1/${name}`, digest: null });

test("the snapshot is a release with every installer", () => {
  const r = normalizeRelease(snapshot);
  assert.ok(find(r, "setup", "x64"), "Windows setup");
  assert.ok(find(r, "dmg", "arm64"), "Apple Silicon dmg");
  assert.ok(find(r, "dmg", "x64"), "Intel dmg");
  assert.ok(find(r, "deb"), ".deb");
  assert.ok(find(r, "rpm"), ".rpm");
  assert.ok(find(r, "appimage"), ".AppImage");
});

test("classify sorts Tauri's file names", () => {
  const cases: [string, string, string | null, string | null][] = [
    ["Zeiler_0.2.1_x64-setup.exe", "setup", "x64", "windows"],
    ["Zeiler_0.2.1_aarch64.dmg", "dmg", "arm64", "macos"],
    ["Zeiler_0.2.1_x64.dmg", "dmg", "x64", "macos"],
    ["Zeiler_0.2.1_amd64.deb", "deb", "x64", "linux"],
    ["Zeiler-0.2.1-1.x86_64.rpm", "rpm", "x64", "linux"],
    ["Zeiler_0.2.1_amd64.AppImage", "appimage", "x64", "linux"],
    ["Zeiler_0.2.1_aarch64.app.tar.gz", "updater", "arm64", null],
    ["Zeiler_0.2.1_x64-setup.exe.sig", "updater", "x64", null],
    ["latest.json", "updater", null, null],
    ["notes.txt", "other", null, null],
  ];
  for (const [name, kind, arch, os] of cases) {
    const d = classify(asset(name));
    assert.deepEqual([d.kind, d.arch, d.os], [kind, arch, os], name);
  }
});

test("installers come before the updater files", () => {
  const r = normalizeRelease(snapshot);
  const kinds = downloads(r).map((d) => d.kind);
  assert.equal(kinds.indexOf("updater"), kinds.length - kinds.filter((k) => k === "updater").length);
});

test("links that don't point at the repo's releases are dropped", () => {
  const r = normalizeRelease({
    ...snapshot,
    assets: [
      { name: "Zeiler_9_x64-setup.exe", size: 1, browser_download_url: "https://evil.example/Zeiler_9_x64-setup.exe" },
      { name: "a.dmg", size: 1, browser_download_url: "javascript:alert(1)" },
      { name: "b.deb", size: 1, browser_download_url: "https://github.com/malhaniah/zeiler/releases/download/v9/b.deb", digest: "sha256:nothex" },
    ],
  });
  assert.deepEqual(r.assets.map((a) => [a.name, a.digest]), [["b.deb", null]]);
});

test("drafts and pre-releases are refused", () => {
  assert.throws(() => normalizeRelease({ ...snapshot, draft: true }));
  assert.throws(() => normalizeRelease({ ...snapshot, prerelease: true }));
  assert.throws(() => normalizeRelease({ ...snapshot, html_url: "https://example.com/" }));
});

test("formatSize", () => {
  assert.equal(formatSize(8_391_871), "8.0 MB");
  assert.equal(formatSize(89_766_392), "86 MB");
  assert.equal(formatSize(432), "432 B");
});
