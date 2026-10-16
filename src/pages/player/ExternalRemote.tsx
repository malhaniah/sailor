/**
 * The Player screen for external mpv: the video plays in mpv's own window, and this
 * screen is its remote control. Same parts as the embedded screen (ControlsBar, the
 * shared actions and keys) so both feel like one product: the title's art fills the
 * window, dimmed; title, episode and source sit at the top; the controls are docked at
 * the bottom and always shown (there is no video here to keep clear).
 *
 * Every control goes over mpv's JSON IPC through the backend-neutral, allow-listed
 * player API. `f` makes mpv's window fullscreen (`cycle fullscreen`). Back keeps mpv
 * playing (as it did before Zeiler could control it); Stop closes mpv. Back while mpv is
 * still opening cancels, and after playback ended Back closes mpv; the Back tooltip and
 * the footnote say which (`backKeepsPlaying`). When the user closes the mpv window, the
 * screen says so and offers Play again.
 */
import { ChevronLeft } from "lucide-react";
import { useCallback, useState } from "react";
import { CopyField } from "../../components/ui";
import type { PlayerLaunch } from "../../lib/playback-handoff";
import { playerCommand } from "../../lib/player-ipc";
import { ControlsBar } from "./ControlsBar";
import type { MpvState, Phase } from "./mpv-state";
import type { Tracks } from "./useTracks";
import { type PlaybackClock, useClock } from "./playback-clock";
import {
  BackButton,
  Backdrop,
  CenterCard,
  ClosedCard,
  FallbackNotice,
  launchBackdrop,
  launchLabels,
  mediaSubtitle,
  NextButton,
  Spinner,
} from "./states";
import { TorrentStats } from "./TorrentStats";
import { backKeepsPlaying } from "./usePlaybackSession";
import { usePlayerControls, usePlayerKeys } from "./usePlayerControls";

const noop = () => undefined;

export function ExternalRemote({
  launch,
  phase,
  mpv,
  tracks,
  clock,
  notice,
  dismissNotice,
  fallbackReason,
  dismissFallback,
  infoHash,
  onBack,
  onStop,
  onPlayAgain,
  onNext,
}: {
  launch: PlayerLaunch;
  phase: Phase;
  mpv: MpvState;
  tracks: Tracks;
  clock: PlaybackClock;
  notice: string | null;
  dismissNotice: () => void;
  fallbackReason: string | null;
  dismissFallback: () => void;
  infoHash: string | null;
  onBack: () => void;
  onStop: () => Promise<void>;
  onPlayAgain: () => void;
  /** play the next episode (null: there is none) */
  onNext: (() => void) | null;
}) {
  const [stopping, setStopping] = useState(false);
  const ended = phase.kind === "ended";
  const controllable = phase.kind === "playing" || ended;
  const controls = usePlayerControls({
    clock,
    mpv,
    ended,
    volumeMax: tracks.volumeMax,
    poke: noop,
  });

  const toggleFullscreen = useCallback(() => {
    void playerCommand("cycle", "fullscreen").catch(() => undefined);
  }, []);
  usePlayerKeys({
    controls: controllable ? controls : null,
    onFullscreen: controllable ? toggleFullscreen : null,
    onBack,
  });

  const stop = () => {
    setStopping(true);
    void onStop().finally(() => setStopping(false));
  };

  const { eyebrow, heading, sourceLine } = launchLabels(launch);
  const subtitle = mediaSubtitle(launch, mpv.mediaTitle);
  // what Back does now (the session's cleanup decides by the same rule)
  const keeps = backKeepsPlaying(phase.kind);
  const backTitle = keeps
    ? "Back (Esc) — mpv keeps playing"
    : phase.kind === "loading"
      ? "Back (Esc) — cancels"
      : phase.kind === "ended"
        ? "Back (Esc) — closes mpv"
        : "Back (Esc)";

  return (
    <main className="fixed inset-0 overflow-hidden bg-canvas">
      <Backdrop src={launchBackdrop(launch)} />

      {/* centre: what mpv is doing (drawn first: the cards span the window) */}
      {phase.kind === "loading" && (
        <CenterCard>
          <Spinner />
          <div className="text-sm text-ink-muted">{phase.label}</div>
          {infoHash && <TorrentStats infoHash={infoHash} />}
          <div className="text-[11.5px] text-ink-subtle">Back cancels.</div>
        </CenterCard>
      )}
      {phase.kind === "playing" && <PlayingCard clock={clock} />}
      {phase.kind === "ended" && (
        <CenterCard>
          <div className="text-base font-semibold">Playback finished</div>
          <div className="flex gap-2">
            <button
              className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-canvas hover:opacity-90"
              onClick={controls.togglePauseOrReplay}
            >
              ↺ Replay
            </button>
            {onNext && <NextButton onClick={onNext} />}
            <BackButton onClick={onBack} />
          </div>
        </CenterCard>
      )}
      {phase.kind === "closed" && (
        <ClosedCard title="mpv was closed" onPlayAgain={onPlayAgain} onBack={onBack} />
      )}
      {phase.kind === "error" && (
        <CenterCard>
          <div className="text-base font-semibold text-danger">Could not play</div>
          <p className="max-w-md break-words text-sm text-ink-muted">{phase.message}</p>
          {phase.streamUrl && (
            <div className="flex w-[26rem] max-w-full flex-col gap-2 text-left">
              <div className="text-[12px] text-ink-muted">Stream URL — play it in any player:</div>
              <CopyField value={phase.streamUrl} />
            </div>
          )}
          <BackButton onClick={onBack} />
        </CenterCard>
      )}

      {/* top: back, title, episode, source */}
      <header
        data-tauri-drag-region
        className="absolute inset-x-0 top-0 flex items-start gap-4 px-5 pb-4 pt-5"
      >
        <button
          onClick={onBack}
          className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full border border-edge bg-canvas/80 text-ink backdrop-blur-md transition-colors hover:bg-elevated"
          aria-label="Back"
          title={backTitle}
        >
          <ChevronLeft size={20} strokeWidth={2.2} />
        </button>
        <div className="flex min-w-0 flex-1 flex-col gap-1 pt-0.5">
          {eyebrow && (
            <div className="truncate text-[11px] font-semibold uppercase tracking-[0.2em] text-ink-subtle">
              {eyebrow}
            </div>
          )}
          <h1 className="truncate font-display text-[28px] font-medium leading-tight tracking-tight text-ink">
            {heading}
          </h1>
          <div className="truncate text-[13px] text-ink-muted">
            <span className="text-ink-subtle">Source: </span>
            {sourceLine}
          </div>
          {subtitle && <div className="truncate text-[12px] text-ink-subtle">{subtitle}</div>}
          {infoHash && controllable && <TorrentStats infoHash={infoHash} compact />}
        </div>
        {/* notices: why built-in playback was not used (info), player errors (warning);
            in the header's flow, so the title lines shrink beside them, never under */}
        {(fallbackReason || notice) && (
          <div className="flex max-w-sm shrink-0 flex-col items-end gap-2 pt-0.5">
            {fallbackReason && (
              <FallbackNotice reason={fallbackReason} onDismiss={dismissFallback} />
            )}
            {notice && (
              <button
                className="max-w-sm truncate rounded-md bg-danger/20 px-3 py-2 text-left text-xs text-ink"
                title={notice}
                onClick={dismissNotice}
              >
                ⚠ {notice}
              </button>
            )}
          </div>
        )}
      </header>

      {/* bottom: the same control bar as the embedded player, always shown */}
      {controllable && (
        <ControlsBar
          clock={clock}
          ended={ended}
          mpv={mpv}
          tracks={tracks}
          controls={controls}
          shown
          onPoke={noop}
          onToggleFullscreen={toggleFullscreen}
          fullscreenTitle="Fullscreen mpv window (f)"
          onStop={stop}
          stopping={stopping}
          footnote={
            keeps
              ? "Playing in its own mpv window. Back keeps it playing while you browse; Stop closes mpv."
              : "Playing in its own mpv window. Back or Stop closes mpv."
          }
        />
      )}
    </main>
  );
}

/** The centre while mpv plays: where the video is, and whether it is paused/buffering. */
function PlayingCard({ clock }: { clock: PlaybackClock }) {
  const paused = useClock(clock, (s) => s.pause);
  const pausedForCache = useClock(clock, (s) => s.pausedForCache);
  const buffering = useClock(clock, (s) => s.buffering);
  const status = pausedForCache ? `Buffering… ${buffering}%` : paused ? "Paused" : "Playing";
  return (
    <CenterCard>
      <div className="text-[10.5px] font-semibold uppercase tracking-[0.32em] text-accent">
        In the mpv window
      </div>
      <div className="flex items-center gap-2 text-sm text-ink">
        {pausedForCache && <Spinner />}
        {status}
      </div>
    </CenterCard>
  );
}
