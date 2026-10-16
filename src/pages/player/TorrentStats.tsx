import { useEffect, useState } from "react";
import { engineStats } from "../../lib/player-ipc";

/** Live torrent line (speed · peers · progress), polled from the engine every 2 s. */
export function TorrentStats({ infoHash, compact = false }: { infoHash: string; compact?: boolean }) {
  const [line, setLine] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        // raw librqbit TorrentStats JSON (snake_case)
        const stats = (await engineStats(infoHash)) as {
          progress_bytes?: number;
          total_bytes?: number;
          live?: {
            download_speed?: { human_readable?: string };
            snapshot?: { peer_stats?: { live?: number } };
          } | null;
        };
        if (cancelled) return;
        const parts: string[] = [];
        const speed = stats.live?.download_speed?.human_readable;
        if (speed) parts.push(`↓ ${speed}`);
        const peers = stats.live?.snapshot?.peer_stats?.live;
        if (typeof peers === "number") parts.push(`${peers} peers`);
        if (
          typeof stats.progress_bytes === "number" &&
          typeof stats.total_bytes === "number" &&
          stats.total_bytes > 0
        ) {
          parts.push(`${Math.round((stats.progress_bytes / stats.total_bytes) * 100)}%`);
        }
        setLine(parts.join(" · ") || null);
      } catch {
        if (!cancelled) setLine(null);
      }
    };
    void tick();
    const id = window.setInterval(() => void tick(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [infoHash]);
  if (!line) return null;
  return (
    <span className={`font-mono text-xs text-ink-subtle ${compact ? "" : "mt-1"}`}>{line}</span>
  );
}
