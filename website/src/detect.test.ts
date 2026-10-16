import assert from "node:assert/strict";
import { test } from "node:test";
import { archFromRenderer, detectFromHints } from "./detect.ts";

const UA = {
  winChrome: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36",
  macSafari: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Safari/605.1.15",
  ubuntuFirefox: "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0",
  fedoraFirefox: "Mozilla/5.0 (X11; Fedora; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0",
  linuxArm: "Mozilla/5.0 (X11; Linux aarch64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36",
  android: "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Mobile Safari/537.36",
  iphone: "Mozilla/5.0 (iPhone; CPU iPhone OS 19_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Mobile/15E148 Safari/604.1",
  chromebook: "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.0.0 Safari/537.36",
};

test("operating systems", () => {
  assert.equal(detectFromHints({ userAgent: UA.winChrome }).os, "windows");
  assert.equal(detectFromHints({ userAgent: UA.macSafari }).os, "macos");
  assert.equal(detectFromHints({ userAgent: UA.ubuntuFirefox }).os, "linux");
  assert.equal(detectFromHints({ userAgent: UA.android }).os, "android");
  assert.equal(detectFromHints({ userAgent: UA.iphone }).os, "ios");
  assert.equal(detectFromHints({ userAgent: UA.chromebook }).os, "chromeos");
  assert.equal(detectFromHints({ userAgent: "" }).os, "unknown");
});

test("an iPad asking for the desktop site is not a Mac", () => {
  assert.equal(detectFromHints({ userAgent: UA.macSafari, maxTouchPoints: 5 }).os, "ios");
});

test("client hints name the platform when the user agent is frozen", () => {
  assert.equal(detectFromHints({ userAgent: "Mozilla/5.0", uaPlatform: "macOS" }).os, "macos");
  assert.equal(detectFromHints({ userAgent: "Mozilla/5.0", uaPlatform: "Windows" }).os, "windows");
});

test("a Mac's user agent says Intel on Apple Silicon too, so it isn't believed", () => {
  assert.equal(detectFromHints({ userAgent: UA.macSafari }).arch, "unknown");
});

test("Linux distro and arch", () => {
  assert.deepEqual(detectFromHints({ userAgent: UA.ubuntuFirefox }), { os: "linux", arch: "x86", distro: "debian" });
  assert.equal(detectFromHints({ userAgent: UA.fedoraFirefox }).distro, "fedora");
  assert.equal(detectFromHints({ userAgent: UA.linuxArm }).arch, "arm");
});

test("WebGL renderer names", () => {
  assert.equal(archFromRenderer("ANGLE (Apple, ANGLE Metal Renderer: Apple M3 Pro, Unspecified Version)"), "arm");
  assert.equal(archFromRenderer("Apple M1, or similar"), "arm");
  assert.equal(archFromRenderer("ANGLE (Intel Inc., Intel(R) Iris(TM) Plus Graphics 655, OpenGL 4.1)"), "x86");
  assert.equal(archFromRenderer("AMD Radeon Pro 5500M"), "x86");
  assert.equal(archFromRenderer("Apple GPU"), "unknown");
  assert.equal(archFromRenderer(""), "unknown");
});
