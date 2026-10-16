import "./style.css";
import { enableCopyButtons, hasFocusWithin, showReleaseLine } from "./common.ts";
import { type Platform, detectArch, detectNow } from "./detect.ts";
import { builtRelease, isNewer, liveRelease } from "./live.ts";
import { renderCta } from "./render.ts";

const cta = document.getElementById("cta");
let release = builtRelease();
let platform: Platform = detectNow();

function draw(): void {
  if (!cta || !release) return;
  cta.innerHTML = renderCta(release, platform, { downloadsHref: "download/" });
  cta.dataset.os = platform.os;
  enableCopyButtons(cta);
}

draw();

void detectArch(platform).then((arch) => {
  if (arch === platform.arch) return;
  platform = { ...platform, arch };
  if (cta && !hasFocusWithin(cta)) draw();
});

void liveRelease().then((live) => {
  if (!live || !release || !isNewer(live, release)) return;
  release = live;
  showReleaseLine(live);
  // the links already there still work: they point at the release the page was built with
  if (cta && !hasFocusWithin(cta)) draw();
});
