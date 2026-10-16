// Which installer to offer. Browsers say little about the CPU: Chromium browsers tell
// the architecture through User-Agent Client Hints; elsewhere the WebGL renderer name
// sometimes gives an Apple Silicon Mac away. When neither does, the arch stays
// "unknown" and the page offers both Mac downloads.

export type DetectedOs = "windows" | "macos" | "linux" | "chromeos" | "ios" | "android" | "unknown";
export type DetectedArch = "arm" | "x86" | "unknown";
export type Distro = "debian" | "fedora" | "suse" | null;

export interface Platform {
  os: DetectedOs;
  arch: DetectedArch;
  distro: Distro;
}

export interface Hints {
  userAgent: string;
  /** navigator.userAgentData.platform, where there is one */
  uaPlatform?: string;
  maxTouchPoints?: number;
}

/** Everything that can be told without asking the browser anything asynchronous. */
export function detectFromHints({ userAgent: ua, uaPlatform, maxTouchPoints = 0 }: Hints): Platform {
  const platform = (uaPlatform ?? "").toLowerCase();
  let os: DetectedOs = "unknown";
  if (/android/i.test(ua) || platform === "android") os = "android";
  else if (/iphone|ipad|ipod/i.test(ua) || platform === "ios") os = "ios";
  else if (/cros/i.test(ua) || platform === "chrome os" || platform === "chromeos") os = "chromeos";
  else if (/windows/i.test(ua) || platform === "windows") os = "windows";
  else if (/macintosh|mac os x/i.test(ua) || platform === "macos") {
    // iPadOS asks for the desktop site with a Mac user agent; a Mac has no touch screen
    os = maxTouchPoints > 1 ? "ios" : "macos";
  } else if (/linux|x11/i.test(ua) || platform === "linux") os = "linux";

  let arch: DetectedArch = "unknown";
  if (/\b(aarch64|arm64|armv8)/i.test(ua)) arch = "arm";
  else if (/\b(x86_64|x64|win64|wow64|amd64)\b/i.test(ua)) arch = "x86";
  // every Mac user agent says "Intel Mac OS X", Apple Silicon too: never trust it
  if (os === "macos") arch = "unknown";

  let distro: Distro = null;
  if (os === "linux") {
    if (/ubuntu|debian|mint|pop!?_os/i.test(ua)) distro = "debian";
    else if (/fedora|red hat|centos|rocky/i.test(ua)) distro = "fedora";
    else if (/suse/i.test(ua)) distro = "suse";
  }
  return { os, arch, distro };
}

/** Apple Silicon or Intel, from a WebGL renderer name such as "Apple M2 Pro". */
export function archFromRenderer(renderer: string): DetectedArch {
  if (/apple m\d|apple a\d/i.test(renderer)) return "arm";
  if (/intel|amd|radeon|nvidia|geforce/i.test(renderer)) return "x86";
  // Safari says "Apple GPU" on both
  return "unknown";
}

interface UaData {
  platform?: string;
  getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string }>;
}

function uaData(): UaData | undefined {
  return (navigator as Navigator & { userAgentData?: UaData }).userAgentData;
}

export function detectNow(): Platform {
  return detectFromHints({
    userAgent: navigator.userAgent,
    uaPlatform: uaData()?.platform,
    maxTouchPoints: navigator.maxTouchPoints,
  });
}

function webglRenderer(): string {
  try {
    const gl = document.createElement("canvas").getContext("webgl");
    if (!gl) return "";
    const ext = gl.getExtension("WEBGL_debug_renderer_info");
    const name = gl.getParameter(ext ? ext.UNMASKED_RENDERER_WEBGL : gl.RENDERER);
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    return typeof name === "string" ? name : "";
  } catch {
    return "";
  }
}

/** The CPU, where the browser will say; asks only on a Mac, where it matters. */
export async function detectArch(p: Platform): Promise<DetectedArch> {
  if (p.os !== "macos") return p.arch;
  try {
    const values = await uaData()?.getHighEntropyValues?.(["architecture"]);
    if (values?.architecture === "arm") return "arm";
    if (values?.architecture === "x86") return "x86";
  } catch {
    // not allowed, or not there
  }
  return archFromRenderer(webglRenderer());
}
