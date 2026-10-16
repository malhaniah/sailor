/**
 * What the player draws for skipping and the next episode, over either screen (the
 * embedded video's chrome or the external-mpv remote): the Skip pill, the Up Next card,
 * and the "Skipped intro · Undo" toast, bottom right above the control bar. Its keys:
 * Enter takes the pill's offer (when no menu is open and no button has the focus), n
 * plays the next episode and p the previous one (also the media keys).
 *
 * The pill and the card follow Harbor's `skip-pill.tsx` and `skip-pill-container.tsx`
 * (MIT — see THIRD-PARTY-NOTICES.md): same labels, the card's still, episode line,
 * countdown and progress bar, Play now and a close button that cancels autoplay.
 */
import { SkipForward, X } from "lucide-react";
import { useEffect } from "react";
import { segmentLabel } from "../../lib/skip";
import type { NextEpisodeControls } from "./useNextEpisode";
import type { SkipControls } from "./useSkip";

export function SkipLayer({
  skip,
  next,
  upNextSeconds,
  keys,
}: {
  skip: SkipControls;
  next: NextEpisodeControls;
  upNextSeconds: number;
  /** the screen takes keys now (it is not loading or gone) */
  keys: boolean;
}) {
  const { offer } = skip;
  // the card already offers the next episode
  const pill = offer && !(offer.next && next.upNext) ? offer : null;

  useSkipKeys({ enabled: keys, onSkip: pill ? skip.skip : null, next });

  return (
    <div className="pointer-events-none fixed bottom-28 right-7 z-20 flex flex-col items-end gap-3">
      {skip.skipped && (
        <div className="pointer-events-auto flex animate-step-in items-center gap-3 rounded-lg border border-edge bg-surface/95 px-4 py-2.5 text-sm text-ink shadow-xl">
          <span>Skipped {segmentLabel(skip.skipped.kind)}</span>
          <button
            className="font-semibold text-accent hover:underline"
            onClick={() => {
              skip.skipped?.undo();
              skip.dismissSkipped();
            }}
          >
            Undo
          </button>
        </div>
      )}
      {next.upNext && next.next && (
        <UpNextCard next={next} upNextSeconds={upNextSeconds} />
      )}
      {pill && (
        <button
          className="pointer-events-auto flex animate-step-in items-center gap-2 rounded-lg border border-edge bg-surface/95 px-5 py-3 text-sm font-semibold text-ink shadow-xl transition-colors hover:bg-elevated"
          onClick={skip.skip}
          title="Enter"
        >
          {pill.next ? "Next Episode" : `Skip ${capitalize(segmentLabel(pill.segment.kind))}`}
          <SkipForward size={16} />
        </button>
      )}
    </div>
  );
}

function UpNextCard({ next, upNextSeconds }: { next: NextEpisodeControls; upNextSeconds: number }) {
  const episode = next.next;
  if (!episode) return null;
  const { video, stream } = episode;
  const label =
    video.season != null && video.episode != null ? `S${video.season} · E${video.episode}` : "Next";
  const title = video.title ?? (video.episode != null ? `Episode ${video.episode}` : "Next episode");
  const opensPicker = stream.state === "none";
  const countdown = next.countdown;
  const progress = countdown === null ? null : 1 - Math.min(1, countdown / upNextSeconds);
  return (
    <div className="pointer-events-auto w-80 animate-step-in overflow-hidden rounded-xl border border-edge bg-surface/95 shadow-2xl">
      <div className="flex gap-3 p-3">
        <div className="h-[72px] w-32 shrink-0 overflow-hidden rounded-md bg-elevated">
          {video.thumbnail && <img src={video.thumbnail} alt="" className="h-full w-full object-cover" />}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-0.5">
          <div className="flex items-start justify-between gap-2">
            <span className="text-[10.5px] font-semibold uppercase tracking-[0.2em] text-ink-subtle">
              Up next · {label}
            </span>
            <button
              className="-mr-1 -mt-1 rounded p-1 text-ink-subtle hover:bg-elevated hover:text-ink"
              onClick={next.dismiss}
              aria-label="Close — do not play the next episode by itself"
              title="Close — do not play the next episode by itself"
            >
              <X size={14} />
            </button>
          </div>
          <div className="truncate text-sm font-medium text-ink" title={title}>
            {title}
          </div>
          <div className="text-xs text-ink-muted">
            {countdown !== null && countdown > 0
              ? opensPicker
                ? `Choosing a stream in ${countdown}s`
                : `Up next in ${countdown}s`
              : opensPicker
                ? "Choose a stream for it"
                : stream.state === "checking"
                  ? "Looking for the same source…"
                  : "Ready"}
          </div>
        </div>
      </div>
      {progress !== null && (
        <div className="h-0.5 bg-edge">
          <div className="h-full bg-accent transition-[width] duration-1000 ease-linear" style={{ width: `${progress * 100}%` }} />
        </div>
      )}
      <button
        className="flex w-full items-center justify-center gap-2 bg-accent/90 py-2 text-sm font-semibold text-canvas hover:bg-accent"
        onClick={next.playNext}
        title="n"
      >
        {opensPicker ? "Choose a stream" : "Play now"}
      </button>
    </div>
  );
}

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function useSkipKeys({
  enabled,
  onSkip,
  next,
}: {
  enabled: boolean;
  onSkip: (() => void) | null;
  next: NextEpisodeControls;
}) {
  const { next: nextEpisode, playNext, playPrevious } = next;
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
      const target = e.target;
      if (
        target instanceof HTMLTextAreaElement ||
        (target instanceof HTMLInputElement && target.type !== "range" && target.type !== "button")
      ) {
        return;
      }
      if (e.key === "Enter") {
        // a focused button keeps Enter, and an open menu is in the way
        if (!onSkip || target instanceof HTMLButtonElement) return;
        if (document.querySelector('[aria-expanded="true"]')) return;
        e.preventDefault();
        onSkip();
      } else if ((e.key === "n" || e.key === "MediaTrackNext") && nextEpisode) {
        e.preventDefault();
        playNext();
      } else if ((e.key === "p" || e.key === "MediaTrackPrevious") && playPrevious) {
        e.preventDefault();
        playPrevious();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled, onSkip, nextEpisode, playNext, playPrevious]);
}
