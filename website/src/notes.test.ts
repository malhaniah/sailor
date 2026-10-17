import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { escapeHtml, renderMarkdown } from "./markdown.ts";
import { FALLBACK_NOTES, installNotes, withoutInstallNotes } from "./notes.ts";

const body: string = JSON.parse(readFileSync(new URL("../release-snapshot.json", import.meta.url), "utf8")).body;

test("each OS's notes come from the release text", () => {
  const notes = installNotes(body);
  assert.match(notes.windows, /More info → Run anyway/);
  assert.match(notes.macos, /right-click Zeiler → \*\*Open\*\*/);
  assert.match(notes.macos, /brew install mpv/);
  assert.match(notes.linux, /RPM Fusion/);
  assert.notEqual(notes.windows, FALLBACK_NOTES.windows);
});

test("a release without install sections gets the built-in notes", () => {
  assert.deepEqual(installNotes("Just a fix."), FALLBACK_NOTES);
});

test("the remaining release text leaves the install sections out", () => {
  const rest = withoutInstallNotes(body);
  assert.doesNotMatch(rest, /### (Windows|macOS|Linux)/);
  assert.match(rest, /Sailor is now Zeiler/);
});

test("markdown is escaped before it is formatted", () => {
  const html = renderMarkdown('<img src=x onerror=alert(1)> **bold** `<b>` [ok](https://mpv.io) [bad](javascript:alert(1))');
  assert.ok(!html.includes("<img"), html);
  assert.match(html, /<strong>bold<\/strong>/);
  assert.match(html, /<code>&lt;b&gt;<\/code>/);
  assert.match(html, /<a href="https:\/\/mpv.io" rel="noopener">ok<\/a>/);
  assert.ok(!html.includes('href="javascript'), html);
});

test("markdown blocks", () => {
  assert.equal(
    renderMarkdown("## Title\nline one\nline two\n\n- a\n- b\n---"),
    "<h2>Title</h2>\n<p>line one line two</p>\n<ul><li>a</li><li>b</li></ul>\n<hr>",
  );
  assert.equal(renderMarkdown("### x", { headingShift: 1 }), "<h4>x</h4>");
  assert.equal(escapeHtml(`"'&`), "&quot;&#39;&amp;");
});
