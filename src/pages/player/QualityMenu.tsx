/**
 * The quality menu: shown only when the stream has more than one video track — an HLS
 * (or DASH) stream's variants, which mpv lists as video tracks with their size and
 * bitrate — and picks one (`vid`). A single-file stream has one video track, so the
 * menu stays away.
 */
import { MonitorPlay } from "lucide-react";
import { playerSetProperty, type MpvTrack } from "../../lib/player-ipc";
import { MenuRow, PopupMenu } from "./menu";
import type { MpvState } from "./mpv-state";

/** The stream's video variants, best first. */
export function videoVariants(tracks: MpvTrack[]): MpvTrack[] {
  return tracks
    .filter((t) => t.type === "video" && !t.albumart && !t.image)
    .sort(
      (a, b) =>
        (b["demux-h"] ?? 0) - (a["demux-h"] ?? 0) ||
        (b["hls-bitrate"] ?? b["demux-bitrate"] ?? 0) - (a["hls-bitrate"] ?? a["demux-bitrate"] ?? 0),
    );
}

/** "1080p · 5.2 Mb/s", or the track's own title. */
export function variantLabel(track: MpvTrack): string {
  const height = track["demux-h"];
  const rate = track["hls-bitrate"] ?? track["demux-bitrate"];
  const parts = [
    height ? `${height}p` : null,
    rate ? (rate >= 1e6 ? `${(rate / 1e6).toFixed(1)} Mb/s` : `${Math.round(rate / 1e3)} kb/s`) : null,
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : (track.title?.trim() || `Track ${track.id}`);
}

export function QualityMenu({ mpv }: { mpv: MpvState }) {
  const variants = videoVariants(mpv.tracks);
  if (variants.length < 2) return null;
  const current = variants.find((t) => String(t.id) === mpv.vid || t.selected);
  return (
    <PopupMenu
      label={
        <>
          <MonitorPlay size={15} />
          {current?.["demux-h"] ? `${current["demux-h"]}p` : "Quality"}
        </>
      }
      title="Quality"
    >
      {(close) =>
        variants.map((t) => (
          <MenuRow
            key={t.id}
            label={variantLabel(t)}
            selected={t === current}
            onPick={() => {
              void playerSetProperty("vid", t.id).catch(() => undefined);
              close();
            }}
          />
        ))
      }
    </PopupMenu>
  );
}
