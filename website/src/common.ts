// What every page does: refresh the release line, and wire the copy buttons.

import { releaseLine } from "./render.ts";
import type { Release } from "./release.ts";

export function showReleaseLine(r: Release): void {
  for (const el of document.querySelectorAll<HTMLElement>("[data-release-line]")) el.innerHTML = releaseLine(r);
}

/** Copy buttons render hidden: they show only where the Clipboard API is there to use. */
export function enableCopyButtons(root: ParentNode = document): void {
  if (!navigator.clipboard) return;
  for (const b of root.querySelectorAll<HTMLButtonElement>("button[data-copy]")) b.hidden = false;
}

document.addEventListener("click", async (e) => {
  const b = (e.target as Element | null)?.closest<HTMLButtonElement>("button[data-copy]");
  if (!b?.dataset.copy) return;
  const label = b.firstChild;
  try {
    await navigator.clipboard.writeText(b.dataset.copy);
    if (label) label.textContent = "Copied";
  } catch {
    if (label) label.textContent = "Couldn’t copy";
  }
  setTimeout(() => label && (label.textContent = "Copy"), 2000);
});

/** True while keyboard or pointer focus is inside el: don't pull it out from under them. */
export const hasFocusWithin = (el: Element): boolean => el.contains(document.activeElement) && document.activeElement !== document.body;
