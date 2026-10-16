/**
 * Playback clock: the fast-changing mpv properties (position ~5×/s, duration, pause,
 * buffering) in a tiny subscribable store, one per player session. Components read
 * exactly the field they draw with `useClock(clock, (s) => s.timePos)`, so a position
 * tick re-renders the seek bar and the time readout — not the whole player page.
 */
import { useSyncExternalStore } from "react";
import type { PlayerEvent } from "../../lib/player-ipc";

export interface ClockState {
  timePos: number;
  duration: number;
  pause: boolean;
  pausedForCache: boolean;
  /** cache-buffering-state, 0-100 */
  buffering: number;
}

const INITIAL_CLOCK: ClockState = {
  timePos: 0,
  duration: 0,
  pause: false,
  pausedForCache: false,
  buffering: 100,
};

export interface PlaybackClock {
  get(): ClockState;
  set(patch: Partial<ClockState>): void;
  subscribe(listener: () => void): () => void;
}

export function createPlaybackClock(): PlaybackClock {
  let state = INITIAL_CLOCK;
  const listeners = new Set<() => void>();
  return {
    get: () => state,
    set(patch) {
      const changed = (Object.keys(patch) as (keyof ClockState)[]).some(
        (key) => patch[key] !== state[key],
      );
      if (!changed) return;
      state = { ...state, ...patch };
      for (const listener of [...listeners]) listener();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** Subscribes to one derived value of the clock; re-renders only when it changes. */
export function useClock<T>(clock: PlaybackClock, select: (state: ClockState) => T): T {
  return useSyncExternalStore(clock.subscribe, () => select(clock.get()));
}

/**
 * Applies a player event to the clock. Returns true when the event concerns only the
 * clock (the page's slower state need not see it). `start-file` resets the clock and
 * returns false: the page resets its own per-file state too.
 */
export function applyClockEvent(clock: PlaybackClock, event: PlayerEvent): boolean {
  if (event.type === "start-file") {
    clock.set({ timePos: 0, duration: 0 });
    return false;
  }
  if (event.type !== "property") return false;
  const v = event.value;
  switch (event.name) {
    case "time-pos":
      if (typeof v === "number") clock.set({ timePos: v });
      return true;
    case "duration":
      if (typeof v === "number") clock.set({ duration: v });
      return true;
    case "pause":
      if (typeof v === "boolean") clock.set({ pause: v });
      return true;
    case "paused-for-cache":
      if (typeof v === "boolean") clock.set({ pausedForCache: v });
      return true;
    case "cache-buffering-state":
      if (typeof v === "number") clock.set({ buffering: v });
      return true;
    default:
      return false;
  }
}
