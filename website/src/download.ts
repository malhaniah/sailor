import "./style.css";
import { enableCopyButtons, hasFocusWithin, showReleaseLine } from "./common.ts";
import { detectNow } from "./detect.ts";
import { builtRelease, isNewer, liveRelease } from "./live.ts";
import { renderDownloads } from "./render.ts";

const root = document.getElementById("downloads");
const { os } = detectNow();

/** Badges the visitor's own OS and puts it first. */
function markOwnOs(): void {
  const card = root?.querySelector<HTMLElement>(`.os-card[data-os="${os}"]`);
  if (!card) return;
  card.classList.add("is-yours");
  card.querySelector<HTMLElement>(".badge")?.removeAttribute("hidden");
  card.parentElement?.prepend(card);
}

markOwnOs();
enableCopyButtons();
if (os === "ios" || os === "android") document.getElementById("device-note")?.removeAttribute("hidden");

void liveRelease().then((live) => {
  const shown = builtRelease();
  if (!root || !live || !shown || !isNewer(live, shown) || hasFocusWithin(root)) return;
  root.innerHTML = renderDownloads(live);
  showReleaseLine(live);
  markOwnOs();
  enableCopyButtons(root);
});
