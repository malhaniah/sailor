/**
 * The stats overlay (key i): what mpv is decoding and how well, read once a second
 * through the read allow-list (`player_get_property`, commands/mpv/validate.rs) while it
 * is open, on either backend. The property list follows Harbor's `stats-overlay.tsx`
 * (MIT — see THIRD-PARTY-NOTICES.md).
 */
import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { playerGetProperty } from "../../lib/player-ipc";
import type { MpvState } from "./mpv-state";

const POLL_MS = 1000;

/** The properties read, in the order shown. */
const PROPERTIES = [
  "video-codec",
  "hwdec-current",
  "container-fps",
  "estimated-vf-fps",
  "video-bitrate",
  "audio-codec",
  "audio-bitrate",
  "frame-drop-count",
  "vo-drop-frame-count",
  "demuxer-cache-duration",
  "cache-speed",
  "mpv-version",
] as const;

type Stats = Partial<Record<(typeof PROPERTIES)[number], unknown>>;

export function StatsOverlay({ mpv, onClose }: { mpv: MpvState; onClose: () => void }) {
  const [stats, setStats] = useState<Stats>({});
  useEffect(() => {
    let live = true;
    const poll = async () => {
      const values = await Promise.all(
        PROPERTIES.map((name) => playerGetProperty(name).catch(() => null)),
      );
      if (!live) return;
      setStats(Object.fromEntries(PROPERTIES.map((name, i) => [name, values[i]])));
    };
    void poll();
    const timer = window.setInterval(() => void poll(), POLL_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, []);

  const video = mpv.tracks.find((t) => t.type === "video" && (t.selected || String(t.id) === mpv.vid));
  const rows: [string, string | null][] = [
    ["Video", join(text(stats["video-codec"]), size(video?.["demux-w"], video?.["demux-h"]))],
    ["Hardware decoding", hwdec(stats["hwdec-current"])],
    ["Frame rate", join(fps(stats["container-fps"]), fps(stats["estimated-vf-fps"], "shown"))],
    ["Video bitrate", bitrate(stats["video-bitrate"])],
    ["Audio", join(text(stats["audio-codec"]), bitrate(stats["audio-bitrate"]))],
    ["Dropped frames", drops(stats["frame-drop-count"], stats["vo-drop-frame-count"])],
    ["Buffered", seconds(stats["demuxer-cache-duration"])],
    ["Download speed", speed(stats["cache-speed"])],
    ["Player", text(stats["mpv-version"])],
  ];

  return (
    <div className="pointer-events-auto fixed left-4 top-16 z-20 w-96 rounded-lg border border-edge bg-canvas/85 p-3 font-mono text-[11.5px] text-ink-muted shadow-xl backdrop-blur-sm">
      <div className="mb-1.5 flex items-center justify-between font-sans text-xs font-semibold text-ink">
        Stats
        <button
          className="rounded p-0.5 text-ink-subtle hover:bg-elevated hover:text-ink"
          onClick={onClose}
          aria-label="Close stats (i)"
          title="Close (i)"
        >
          <X size={13} />
        </button>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
        {rows.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-ink-subtle">{label}</dt>
            <dd className="truncate text-ink" title={value ?? undefined}>
              {value ?? "—"}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

const num = (v: unknown) => (typeof v === "number" && Number.isFinite(v) ? v : null);
const text = (v: unknown) => (typeof v === "string" && v.trim() ? v.trim() : null);
const join = (...parts: (string | null)[]) => parts.filter(Boolean).join(" · ") || null;

function size(w: number | undefined, h: number | undefined): string | null {
  return w && h ? `${w}×${h}` : null;
}
function hwdec(v: unknown): string | null {
  const s = text(v);
  return s === null ? null : s === "no" ? "off" : s;
}
function fps(v: unknown, suffix = ""): string | null {
  const n = num(v);
  return n === null ? null : `${n.toFixed(n % 1 ? 3 : 0)} fps${suffix ? ` ${suffix}` : ""}`;
}
function bitrate(v: unknown): string | null {
  const n = num(v);
  if (n === null) return null;
  return n >= 1e6 ? `${(n / 1e6).toFixed(1)} Mb/s` : `${Math.round(n / 1e3)} kb/s`;
}
function drops(decoder: unknown, output: unknown): string | null {
  const a = num(decoder);
  const b = num(output);
  if (a === null && b === null) return null;
  return `${a ?? 0} decoder · ${b ?? 0} output`;
}
function seconds(v: unknown): string | null {
  const n = num(v);
  return n === null ? null : `${n.toFixed(1)} s`;
}
function speed(v: unknown): string | null {
  const n = num(v);
  if (n === null) return null;
  return n >= 1048576 ? `${(n / 1048576).toFixed(1)} MiB/s` : `${Math.round(n / 1024)} KiB/s`;
}
