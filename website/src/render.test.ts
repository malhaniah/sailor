import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import type { Platform } from "./detect.ts";
import { normalizeRelease } from "./release.ts";
import { renderCta, renderDownloads } from "./render.ts";

const release = normalizeRelease(JSON.parse(readFileSync(new URL("../release-snapshot.json", import.meta.url), "utf8")));
const o = { downloadsHref: "download/" };
const cta = (p: Partial<Platform>) => renderCta(release, { os: "unknown", arch: "unknown", distro: null, ...p }, o);
const files = (html: string) => [...html.matchAll(/data-file="([^"]+)"/g)].map((m) => m[1]);

test("Windows gets the setup.exe", () => {
  assert.deepEqual(files(cta({ os: "windows" })).map((f) => /-setup\.exe$/.test(f)), [true]);
  assert.match(cta({ os: "windows" }), /Run anyway/);
});

test("a Mac of unknown CPU gets both disk images; a known one gets its own", () => {
  assert.deepEqual(files(cta({ os: "macos" })).map((f) => f.match(/_(aarch64|x64)\.dmg$/)?.[1]), ["aarch64", "x64"]);
  assert.deepEqual(files(cta({ os: "macos", arch: "arm" })).map((f) => f.match(/_(aarch64|x64)\.dmg$/)?.[1]), ["aarch64"]);
  assert.deepEqual(files(cta({ os: "macos", arch: "x86" })).map((f) => f.match(/_(aarch64|x64)\.dmg$/)?.[1]), ["x64"]);
  assert.match(cta({ os: "macos", arch: "arm" }), /Download the Intel version/);
});

test("Linux gets .deb, .rpm and .AppImage, its distro's first-class", () => {
  assert.deepEqual(files(cta({ os: "linux" })).map((f) => f.split(".").pop()), ["deb", "rpm", "AppImage"]);
  const fedora = cta({ os: "linux", distro: "fedora" });
  assert.match(fedora, /btn-primary" href="[^"]+\.rpm"/);
  assert.match(fedora, /btn-secondary" href="[^"]+\.deb"/);
});

test("phones and the page as built point at all downloads", () => {
  for (const html of [cta({ os: "android" }), cta({ os: "ios" }), renderCta(release, null, o)]) {
    assert.deepEqual(files(html), []);
    assert.match(html, /href="download\/"/);
  }
});

test("the downloads page lists every file once and every OS's notes", () => {
  const html = renderDownloads(release);
  for (const a of release.assets) assert.ok(html.includes(`>${a.name}</a>`), a.name);
  for (const id of ["windows", "macos", "linux"]) assert.match(html, new RegExp(`id="${id}"`));
  assert.match(html, /SmartScreen/);
  assert.match(html, /brew install mpv/);
});
