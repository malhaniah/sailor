/**
 * Player status screens: the small overlay cards drawn above the video, the
 * "Playing in mpv" screen of non-embedded builds, and "nothing to play" for a launch id
 * that is unknown or already used (a reload, a stale history entry).
 */
import { ChevronLeft } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { EmptyState } from "../../components/ui";
import type { PlayerLaunch } from "../../lib/playback-handoff";
import { TorrentStats } from "./TorrentStats";

export function CenterCard({ children }: { children: ReactNode }) {
  return (
    <div className="absolute inset-0 flex items-center justify-center">
      <div className="flex flex-col items-center gap-3 rounded-xl bg-canvas/80 px-8 py-6 text-center">
        {children}
      </div>
    </div>
  );
}

export function Spinner() {
  return (
    <div className="h-6 w-6 animate-spin rounded-full border-2 border-edge border-t-accent" />
  );
}

export function BackButton({ onClick }: { onClick: () => void }) {
  return (
    <button className="rounded-md bg-elevated px-4 py-2 text-sm hover:bg-raised" onClick={onClick}>
      ← Back
    </button>
  );
}

/** Escape leaves the screen (the app shell underneath ignores keys while the player is up). */
function useEscape(onEscape: () => void) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onEscape();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onEscape]);
}

/** Full-window frame for the screens that have no video behind them. */
function Screen({
  backdrop,
  onBack,
  children,
}: {
  backdrop?: string | null;
  onBack: () => void;
  children: ReactNode;
}) {
  return (
    <main className="fixed inset-0 flex items-center justify-center overflow-hidden bg-canvas px-6">
      {backdrop && (
        <div className="pointer-events-none absolute inset-0">
          <img src={backdrop} alt="" className="h-full w-full scale-105 object-cover opacity-30" />
          <div className="absolute inset-0 bg-gradient-to-b from-canvas/40 via-canvas/85 to-canvas" />
        </div>
      )}
      <div data-tauri-drag-region className="absolute inset-x-0 top-0 h-20" />
      <button
        onClick={onBack}
        className="fixed left-5 top-5 z-10 flex h-11 w-11 items-center justify-center rounded-full border border-edge bg-canvas/80 text-ink backdrop-blur-md transition-colors hover:bg-elevated"
        aria-label="Back"
      >
        <ChevronLeft size={20} strokeWidth={2.2} />
      </button>
      <div className="relative w-full max-w-lg">{children}</div>
    </main>
  );
}

export function NothingToPlay({ onBack }: { onBack: () => void }) {
  useEscape(onBack);
  return (
    <Screen onBack={onBack}>
      <EmptyState title="Nothing to play">
        Player links work only once, so this one has expired (for example after a reload). Go
        back and pick a stream again.
        <div className="mt-5">
          <BackButton onClick={onBack} />
        </div>
      </EmptyState>
    </Screen>
  );
}

/**
 * The picked stream's label without the addon name: addons usually put their own name on
 * the first line of `stream.name` and the quality on the next ("Torrentio\n4k"), so every
 * line is kept except one that repeats the addon. Falls back to the stream's filename.
 */
function streamLabel(launch: PlayerLaunch): string | null {
  const addon = launch.addonName?.trim();
  const label = (launch.stream?.name ?? "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && line !== addon)
    .join(" ");
  return label || launch.stream?.behaviorHints.filename || null;
}

/**
 * Non-embedded builds: playback runs in its own mpv window. Zeiler has no remote control
 * over that window yet (M2), so this screen offers Stop and Back; Back keeps mpv playing.
 */
export function ExternalPlayback({
  launch,
  infoHash,
  onBack,
  onStop,
}: {
  launch: PlayerLaunch;
  infoHash: string | null;
  onBack: () => void;
  onStop: () => Promise<void>;
}) {
  const [stopping, setStopping] = useState(false);
  useEscape(onBack);

  const { meta, video } = launch;
  const eyebrow = video
    ? `${meta?.name ?? ""} · Season ${video.season ?? "?"} · Episode ${String(video.episode ?? "?").padStart(2, "0")}`
    : null;
  const heading = video
    ? (video.title ?? `Episode ${video.episode ?? "?"}`)
    : (meta?.name ?? launch.title ?? "Now playing");
  const kind = launch.source.kind === "torrent" ? "Torrent" : "Direct stream";
  const sourceLine = [launch.addonName ?? kind, streamLabel(launch)].filter(Boolean).join(" · ");

  return (
    <Screen backdrop={video?.thumbnail ?? meta?.poster ?? null} onBack={onBack}>
      <div className="flex flex-col gap-6 rounded-[24px] border border-edge-soft bg-elevated/95 px-8 py-8 shadow-[0_30px_80px_-25px_rgba(0,0,0,0.85)]">
        <div className="flex flex-col gap-2">
          <div className="text-[10.5px] font-semibold uppercase tracking-[0.32em] text-accent">
            Playing in mpv
          </div>
          {eyebrow && (
            <div className="text-[11px] font-semibold uppercase tracking-[0.2em] text-ink-subtle">
              {eyebrow}
            </div>
          )}
          <h1 className="font-display text-[32px] font-medium leading-tight tracking-tight text-ink">
            {heading}
          </h1>
          <div className="text-[13px] text-ink-muted">
            <span className="text-ink-subtle">Source: </span>
            {sourceLine}
          </div>
          {infoHash && <TorrentStats infoHash={infoHash} compact />}
        </div>
        <p className="text-[13.5px] leading-relaxed text-ink-muted">
          The video is playing in its own mpv window. Go back to keep browsing; playback
          continues until you stop it here or close mpv.
        </p>
        <div className="flex gap-2">
          <button
            disabled={stopping}
            className="h-10 rounded-xl bg-danger/15 px-4 text-[13px] font-semibold text-[oklch(0.75_0.12_25)] transition-colors hover:bg-danger/25 disabled:opacity-60"
            onClick={() => {
              setStopping(true);
              void onStop().finally(() => setStopping(false));
            }}
          >
            {stopping ? "Stopping…" : "Stop"}
          </button>
          <button
            className="h-10 rounded-xl bg-raised px-4 text-[13px] font-semibold text-ink transition-colors hover:bg-edge"
            onClick={onBack}
          >
            ← Back
          </button>
        </div>
      </div>
    </Screen>
  );
}
