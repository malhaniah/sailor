/**
 * The player's actions and keyboard shortcuts, shared by both screens: the embedded
 * chrome (video under the webview) and the external-mpv remote (mpv in its own
 * window). Every action goes through the backend-neutral, allow-listed player API, so
 * the same code drives either backend; only fullscreen differs (the caller passes it).
 *
 * Keys work while the Zeiler window has focus: Space play/pause (replay once ended),
 * ←/→ seek 5 s, ↑/↓ volume ±5, m mute, f fullscreen, s next subtitle, c next second
 * subtitle, z/x subtitle delay −/+0.1 s, [ / ] slower/faster, Escape back. A focused text field
 * keeps its own keys, and so does the focused volume slider for the arrows; the seek
 * bar does not (its native step is a fraction of a second), so ←/→ still seek 5 s after
 * a click on it. An open menu takes Escape first (it closes, see ControlsBar).
 */
import { useCallback, useEffect, useMemo, useRef } from "react";
import { playerCommand, playerSetProperty } from "../../lib/player-ipc";
import type { MpvState } from "./mpv-state";
import type { PlaybackClock } from "./playback-clock";

export const SEEK_STEP_SEC = 5;
/** The z/x subtitle delay step, in seconds. */
export const SUB_DELAY_STEP = 0.1;
/** The speeds the menu offers and [ / ] step through. */
export const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;
/** How long a z/x or [ / ] step builds on the previous one rather than on mpv's value. */
const CHAIN_MS = 1000;

interface Asked {
  value: number;
  at: number;
}
export const VOLUME_STEP = 5;
/** The volume slider's and the ↑ key's ceiling until the settings say otherwise
 * (`audio.volumeMax`, which Rust also gives mpv as `volume-max`). */
export const VOLUME_MAX = 100;
/** Marks the seek bar's range input, whose arrow keys the window shortcuts take over. */
export const SEEK_SLIDER_ATTR = "data-seek-slider";

/**
 * The volume after a ↑/↓ step. Clamped to 0…`max`, except that a volume already above
 * it (external mpv: its own keys) is never pulled down by ↑ and steps down from where
 * it is with ↓.
 */
export function nextVolume(current: number, delta: number, max = VOLUME_MAX): number {
  const now = Math.round(current);
  const ceiling = Math.max(max, now);
  return Math.min(ceiling, Math.max(0, now + delta));
}

export interface PlayerControls {
  togglePauseOrReplay: () => void;
  seekBy: (deltaSec: number) => void;
  changeVolume: (delta: number) => void;
  toggleMute: () => void;
  /** the next subtitle track (and Off), as mpv orders them */
  cycleSubtitles: () => void;
  /** the next second subtitle (and none) */
  cycleSecondarySubtitles: () => void;
  stepSubDelay: (deltaSec: number) => void;
  /** the next speed of SPEEDS up (+1) or down (-1) */
  stepSpeed: (direction: 1 | -1) => void;
}

/** The speed after a [ or ] from `current`. */
export function nextSpeed(current: number, direction: 1 | -1): number {
  const faster = SPEEDS.find((s) => s > current + 1e-3);
  const slower = [...SPEEDS].reverse().find((s) => s < current - 1e-3);
  return (direction > 0 ? faster : slower) ?? current;
}

export function usePlayerControls({
  clock,
  mpv,
  ended,
  volumeMax = VOLUME_MAX,
  poke,
}: {
  clock: PlaybackClock;
  mpv: MpvState;
  ended: boolean;
  /** the volume's ceiling (`audio.volumeMax`) */
  volumeMax?: number;
  /** called after every action (shows auto-hiding controls) */
  poke: () => void;
}): PlayerControls {
  // read at call time, so the callbacks stay stable across volume/mute changes
  const latest = useRef(mpv);
  latest.current = mpv;
  const ceiling = useRef(volumeMax);
  ceiling.current = volumeMax;

  const togglePauseOrReplay = useCallback(() => {
    if (ended) {
      void playerCommand("seek", 0, "absolute").catch(() => undefined);
      void playerSetProperty("pause", false).catch(() => undefined);
    } else {
      void playerSetProperty("pause", !clock.get().pause).catch(() => undefined);
    }
    poke();
  }, [ended, clock, poke]);

  const seekBy = useCallback(
    (deltaSec: number) => {
      void playerCommand("seek", deltaSec, "relative").catch(() => undefined);
      poke();
    },
    [poke],
  );

  const changeVolume = useCallback(
    (delta: number) => {
      const next = nextVolume(latest.current.volume, delta, ceiling.current);
      void playerSetProperty("volume", next).catch(() => undefined);
      if (latest.current.mute && delta > 0) {
        void playerSetProperty("mute", false).catch(() => undefined);
      }
      poke();
    },
    [poke],
  );

  const toggleMute = useCallback(() => {
    void playerSetProperty("mute", !latest.current.mute).catch(() => undefined);
    poke();
  }, [poke]);

  // Steps pressed faster than mpv reports them build on the last one asked for, not on
  // a value mpv has not caught up with yet; after a pause mpv's own value counts again
  // (it may have changed elsewhere, e.g. keys in the external mpv window).
  const asked = useRef<{ subDelay?: Asked; speed?: Asked }>({});
  const base = (name: "subDelay" | "speed") => {
    const last = asked.current[name];
    return last && Date.now() - last.at < CHAIN_MS ? last.value : latest.current[name];
  };
  const ask = (name: "subDelay" | "speed", value: number) => {
    asked.current[name] = { value, at: Date.now() };
  };

  const cycleSubtitles = useCallback(() => {
    void playerCommand("cycle", "sid").catch(() => undefined);
    poke();
  }, [poke]);

  const cycleSecondarySubtitles = useCallback(() => {
    void playerCommand("cycle", "secondary-sid").catch(() => undefined);
    poke();
  }, [poke]);

  const stepSubDelay = useCallback(
    (deltaSec: number) => {
      const next = Math.min(600, Math.max(-600, Math.round((base("subDelay") + deltaSec) * 1000) / 1000));
      ask("subDelay", next);
      void playerSetProperty("sub-delay", next).catch(() => undefined);
      poke();
    },
    [poke],
  );

  const stepSpeed = useCallback(
    (direction: 1 | -1) => {
      const next = nextSpeed(base("speed"), direction);
      ask("speed", next);
      void playerSetProperty("speed", next).catch(() => undefined);
      poke();
    },
    [poke],
  );

  return useMemo(
    () => ({
      togglePauseOrReplay,
      seekBy,
      changeVolume,
      toggleMute,
      cycleSubtitles,
      cycleSecondarySubtitles,
      stepSubDelay,
      stepSpeed,
    }),
    [
      togglePauseOrReplay,
      seekBy,
      changeVolume,
      toggleMute,
      cycleSubtitles,
      cycleSecondarySubtitles,
      stepSubDelay,
      stepSpeed,
    ],
  );
}

/**
 * Window-level shortcuts. `controls` is null while there is nothing to control (loading,
 * an error, mpv closed): then only `f` (when `onFullscreen` is given) and Escape work.
 * Escape calls `onBack` (the embedded screen's leaves fullscreen first, PlayerChrome).
 */
export function usePlayerKeys({
  controls,
  onFullscreen,
  onBack,
}: {
  controls: PlayerControls | null;
  onFullscreen: (() => void) | null;
  onBack: () => void;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
      const target = e.target;
      const ownsArrows =
        target instanceof HTMLInputElement &&
        target.type === "range" &&
        !target.hasAttribute(SEEK_SLIDER_ATTR);
      const typing =
        target instanceof HTMLTextAreaElement ||
        (target instanceof HTMLInputElement && target.type !== "range" && target.type !== "button");
      if (e.key === "Escape") {
        onBack();
        return;
      }
      // the focused volume slider moves itself with the arrow keys (and applies it);
      // on the seek bar, preventDefault below keeps its 0.1 s native step from running
      if (typing || (ownsArrows && e.key.startsWith("Arrow"))) return;
      if (e.key === "f" && onFullscreen) {
        onFullscreen();
        return;
      }
      if (!controls) return;
      switch (e.key) {
        case " ":
          // no page scroll, and a button focused by an earlier click must not click again
          e.preventDefault();
          if (target instanceof HTMLButtonElement) target.blur();
          controls.togglePauseOrReplay();
          break;
        case "ArrowLeft":
          e.preventDefault();
          controls.seekBy(-SEEK_STEP_SEC);
          break;
        case "ArrowRight":
          e.preventDefault();
          controls.seekBy(SEEK_STEP_SEC);
          break;
        case "ArrowUp":
          e.preventDefault();
          controls.changeVolume(VOLUME_STEP);
          break;
        case "ArrowDown":
          e.preventDefault();
          controls.changeVolume(-VOLUME_STEP);
          break;
        case "m":
          controls.toggleMute();
          break;
        case "s":
          controls.cycleSubtitles();
          break;
        case "c":
          controls.cycleSecondarySubtitles();
          break;
        case "z":
          controls.stepSubDelay(-SUB_DELAY_STEP);
          break;
        case "x":
          controls.stepSubDelay(SUB_DELAY_STEP);
          break;
        case "[":
          controls.stepSpeed(-1);
          break;
        case "]":
          controls.stepSpeed(1);
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [controls, onFullscreen, onBack]);
}
