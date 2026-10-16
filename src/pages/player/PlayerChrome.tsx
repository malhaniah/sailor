/**
 * The embedded player's screen — everything drawn above the video: status overlays, the
 * top bar and the control bar, plus the keyboard shortcuts (usePlayerControls.ts). The
 * page is transparent and `videoRef` marks the region the native mpv window is glued
 * to. Also drawn while the backend is still unknown (the first moments of loading).
 * External mpv has its own screen, ExternalRemote.tsx, built from the same parts.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { CopyField } from "../../components/ui";
import { ControlsBar } from "./ControlsBar";
import type { Backend, MpvState, Phase } from "./mpv-state";
import { type PlaybackClock, useClock } from "./playback-clock";
import { BackButton, CenterCard, ClosedCard, Spinner } from "./states";
import { TorrentStats } from "./TorrentStats";
import { usePlayerControls, usePlayerKeys } from "./usePlayerControls";

export function PlayerChrome({
  phase,
  backend,
  mpv,
  clock,
  notice,
  dismissNotice,
  title,
  infoHash,
  videoRef,
  onLeave,
  onPlayAgain,
}: {
  phase: Phase;
  backend: Backend;
  mpv: MpvState;
  clock: PlaybackClock;
  notice: string | null;
  dismissNotice: () => void;
  title: string;
  infoHash: string | null;
  videoRef: React.RefObject<HTMLDivElement | null>;
  onLeave: () => void;
  onPlayAgain: () => void;
}) {
  const [visible, setVisible] = useState(true);
  const hideTimer = useRef<number | null>(null);
  const playing = phase.kind === "playing";
  const paused = useClock(clock, (s) => s.pause);
  const embedded = backend === "embedded";

  const poke = useCallback(() => {
    setVisible(true);
    if (hideTimer.current !== null) window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => setVisible(false), 3000);
  }, []);

  useEffect(() => {
    poke();
    return () => {
      if (hideTimer.current !== null) window.clearTimeout(hideTimer.current);
    };
  }, [poke]);

  // Controls stay up whenever playback isn't actively running.
  const controlsShown = visible || !playing || paused;

  const controllable = playing || phase.kind === "ended";
  const controls = usePlayerControls({ clock, mpv, ended: phase.kind === "ended", poke });

  // embedded video has no window of its own: fullscreen is the Zeiler window's
  const toggleFullscreen = useCallback(async () => {
    const w = getCurrentWindow();
    await w.setFullscreen(!(await w.isFullscreen())).catch(() => undefined);
  }, []);
  const onFullscreen = useCallback(() => void toggleFullscreen(), [toggleFullscreen]);

  usePlayerKeys({
    controls: embedded && controllable ? controls : null,
    onFullscreen,
    onBack: onLeave,
  });

  return (
    <div
      className="fixed inset-0 overflow-hidden"
      style={{
        // only the embedded backend has a native video behind the page
        background: embedded ? "transparent" : "var(--color-canvas)",
        cursor: controlsShown ? "default" : "none",
      }}
      onMouseMove={poke}
      onClick={(e) => {
        if (!embedded) return;
        if (!controllable) return;
        if (e.target === e.currentTarget || e.target === videoRef.current) {
          controls.togglePauseOrReplay();
        }
      }}
      onDoubleClick={onFullscreen}
    >
      {/* the transparent region the native video is glued to */}
      <div ref={videoRef} className="absolute inset-0" />

      {/* status overlays */}
      {phase.kind === "loading" && (
        <CenterCard>
          <Spinner />
          <div className="text-sm text-ink-muted">{phase.label}</div>
          {infoHash && <TorrentStats infoHash={infoHash} />}
        </CenterCard>
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
          <BackButton onClick={onLeave} />
        </CenterCard>
      )}
      {phase.kind === "playing" && <BufferingOverlay clock={clock} />}
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
            <BackButton onClick={onLeave} />
          </div>
        </CenterCard>
      )}

      {phase.kind === "closed" && (
        <ClosedCard
          title="The player stopped"
          onPlayAgain={onPlayAgain}
          onBack={onLeave}
        />
      )}

      {notice && (
        <button
          className="absolute right-4 top-16 max-w-sm truncate rounded-md bg-danger/20 px-3 py-2 text-left text-xs text-ink"
          title={notice}
          onClick={dismissNotice}
        >
          ⚠ {notice}
        </button>
      )}

      {/* top bar */}
      <div
        className={`absolute inset-x-0 top-0 flex items-center gap-3 bg-gradient-to-b from-canvas/85 to-transparent px-4 pb-8 pt-3 transition-opacity duration-300 ${
          controlsShown ? "opacity-100" : "pointer-events-none opacity-0"
        }`}
      >
        <button
          className="rounded-md bg-canvas/60 px-3 py-1.5 text-sm text-ink-muted hover:text-ink"
          onClick={onLeave}
        >
          ← Back
        </button>
        <div className="min-w-0 flex-1 truncate text-sm font-medium">{title}</div>
        {infoHash && phase.kind === "playing" && <TorrentStats infoHash={infoHash} compact />}
      </div>

      {/* bottom control bar */}
      {(phase.kind === "playing" || phase.kind === "ended") && (
        <ControlsBar
          clock={clock}
          ended={phase.kind === "ended"}
          mpv={mpv}
          controls={controls}
          shown={controlsShown}
          onPoke={poke}
          onToggleFullscreen={onFullscreen}
        />
      )}
    </div>
  );
}

function BufferingOverlay({ clock }: { clock: PlaybackClock }) {
  const pausedForCache = useClock(clock, (s) => s.pausedForCache);
  const buffering = useClock(clock, (s) => s.buffering);
  if (!pausedForCache) return null;
  return (
    <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
      <div className="flex items-center gap-3 rounded-lg bg-canvas/70 px-4 py-3">
        <Spinner />
        <span className="text-sm text-ink-muted">Buffering… {buffering}%</span>
      </div>
    </div>
  );
}
