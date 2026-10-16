/**
 * Player status screens and shared bits: the small overlay cards drawn above the video,
 * the title/source lines and backdrop of a launch, the passive "Playing in mpv" card
 * (external mpv started without remote control), and "nothing to play" for a launch id
 * that is unknown or already used (a reload, a stale history entry).
 */
import { ChevronLeft, Info } from "lucide-react";
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

/** The player went away on its own (mpv's window was closed, or the core crashed). */
export function ClosedCard({
  title,
  onPlayAgain,
  onBack,
}: {
  title: string;
  onPlayAgain: () => void;
  onBack: () => void;
}) {
  return (
    <CenterCard>
      <div className="text-base font-semibold">{title}</div>
      <div className="flex gap-2">
        <button
          className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-canvas hover:opacity-90"
          onClick={onPlayAgain}
        >
          ▶ Play again
        </button>
        <BackButton onClick={onBack} />
      </div>
    </CenterCard>
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

/** Why built-in playback was not used (it fell back to mpv): an info note, dismissible. */
export function FallbackNotice({ reason, onDismiss }: { reason: string; onDismiss: () => void }) {
  return (
    <button
      className="flex items-start gap-2 rounded-md bg-elevated/90 px-3 py-2 text-left text-xs text-ink-muted ring-1 ring-edge-soft"
      title="Dismiss"
      onClick={onDismiss}
    >
      <Info size={14} className="mt-px shrink-0 text-accent" />
      <span>Built-in playback could not start, so this plays in mpv: {reason}</span>
    </button>
  );
}

/** The launch's title art, dimmed to sit behind the controls (screens without video). */
export function Backdrop({ src }: { src: string | null }) {
  if (!src) return null;
  return (
    <div className="pointer-events-none absolute inset-0">
      <img src={src} alt="" className="h-full w-full scale-105 object-cover opacity-30" />
      <div className="absolute inset-0 bg-gradient-to-b from-canvas/40 via-canvas/85 to-canvas" />
    </div>
  );
}

/** Wide art for a launch: the title's backdrop, else the episode still, else the poster. */
export function launchBackdrop(launch: PlayerLaunch): string | null {
  return launch.meta?.background ?? launch.video?.thumbnail ?? launch.meta?.poster ?? null;
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
      <Backdrop src={backdrop ?? null} />
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

/** What the screens say about a launch: series/episode line, heading, source line. */
export function launchLabels(launch: PlayerLaunch): {
  eyebrow: string | null;
  heading: string;
  sourceLine: string;
} {
  const { meta, video } = launch;
  const eyebrow = video
    ? `${meta?.name ?? ""} · Season ${video.season ?? "?"} · Episode ${String(video.episode ?? "?").padStart(2, "0")}`
    : null;
  const heading = video
    ? (video.title ?? `Episode ${video.episode ?? "?"}`)
    : (meta?.name ?? launch.title ?? "Now playing");
  const kind = launch.source.kind === "torrent" ? "Torrent" : "Direct stream";
  const sourceLine = [launch.addonName ?? kind, streamLabel(launch)].filter(Boolean).join(" · ");
  return { eyebrow, heading, sourceLine };
}

/** A display label for comparison: case, spacing, line breaks and "·" separators aside. */
const labelKey = (label: string | null | undefined) =>
  (label ?? "").replace(/[\s·]+/g, " ").trim().toLowerCase();

/**
 * mpv's media-title, only when it tells the user something the other labels do not
 * (typically the torrent's or stream's file name). Zeiler forces the launch title as
 * media-title, and for a stream without a filename hint that title is just the stream
 * name or "Show · S1E2", already on screen.
 */
export function mediaSubtitle(launch: PlayerLaunch, mediaTitle: string | null): string | null {
  const own = labelKey(mediaTitle);
  if (!own) return null;
  const { eyebrow, heading, sourceLine } = launchLabels(launch);
  const shown = [heading, eyebrow, sourceLine, launch.stream?.name, streamLabel(launch)];
  if (!launch.stream?.behaviorHints.filename) shown.push(launch.title);
  return shown.some((label) => labelKey(label) === own) ? null : mediaTitle;
}

/**
 * External mpv launched without its JSON IPC (the last resort when the IPC never came
 * up): it plays in its own window, but Zeiler can neither control it nor see its state,
 * so this card offers only Stop and Back. Back keeps mpv playing. When built-in playback
 * fell back to mpv, the card says why.
 */
export function UncontrolledPlayback({
  launch,
  infoHash,
  fallbackReason,
  dismissFallback,
  onBack,
  onStop,
}: {
  launch: PlayerLaunch;
  infoHash: string | null;
  fallbackReason: string | null;
  dismissFallback: () => void;
  onBack: () => void;
  onStop: () => Promise<void>;
}) {
  const [stopping, setStopping] = useState(false);
  useEscape(onBack);

  const { eyebrow, heading, sourceLine } = launchLabels(launch);

  return (
    <Screen backdrop={launchBackdrop(launch)} onBack={onBack}>
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
        {fallbackReason && <FallbackNotice reason={fallbackReason} onDismiss={dismissFallback} />}
        <p className="text-[13.5px] leading-relaxed text-ink-muted">
          The video is playing in its own mpv window, but mpv did not open its control
          connection, so Zeiler cannot show its controls here. Use mpv&apos;s own keys in
          its window. Back keeps it playing; Stop closes it.
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
