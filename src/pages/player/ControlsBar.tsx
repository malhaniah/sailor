/**
 * The bottom control bar of the embedded player: seek bar, play/pause, time, volume,
 * audio/subtitle track menus and fullscreen. Position-driven parts subscribe to the
 * playback clock themselves, so a time-pos tick re-renders only them.
 */
import { useState } from "react";
import {
  type MpvTrack,
  playerCommand,
  playerSetProperty,
} from "../../lib/player-ipc";
import { formatTime, type MpvState } from "./mpv-state";
import { type PlaybackClock, useClock } from "./playback-clock";

export function ControlsBar({
  clock,
  ended,
  mpv,
  shown,
  onPoke,
  onTogglePause,
  onToggleFullscreen,
}: {
  clock: PlaybackClock;
  ended: boolean;
  mpv: MpvState;
  shown: boolean;
  onPoke: () => void;
  onTogglePause: () => void;
  onToggleFullscreen: () => void;
}) {
  return (
    <div
      className={`absolute inset-x-0 bottom-0 flex flex-col gap-1.5 bg-gradient-to-t from-canvas/90 to-transparent px-4 pb-3 pt-10 transition-opacity duration-300 ${
        shown ? "opacity-100" : "pointer-events-none opacity-0"
      }`}
    >
      <SeekBar clock={clock} onPoke={onPoke} />
      <div className="flex items-center gap-4">
        <PlayPauseButton clock={clock} ended={ended} onClick={onTogglePause} />
        <TimeReadout clock={clock} />
        <VolumeControl volume={mpv.volume} mute={mpv.mute} onPoke={onPoke} />
        <div className="flex-1" />
        <TrackMenu label="Audio" type="audio" tracks={mpv.tracks} current={mpv.aid} property="aid" />
        <TrackMenu
          label="Subtitles"
          type="sub"
          tracks={mpv.tracks}
          current={mpv.sid}
          property="sid"
          allowOff
        />
        <button
          className="rounded-md bg-canvas/50 px-3 py-1 text-sm text-ink-muted hover:bg-elevated hover:text-ink"
          onClick={onToggleFullscreen}
          title="Fullscreen (f)"
        >
          ⛶
        </button>
      </div>
    </div>
  );
}

function PlayPauseButton({
  clock,
  ended,
  onClick,
}: {
  clock: PlaybackClock;
  ended: boolean;
  onClick: () => void;
}) {
  const pause = useClock(clock, (s) => s.pause);
  return (
    <button
      className="w-10 rounded-md bg-canvas/50 py-1 text-lg hover:bg-elevated"
      onClick={onClick}
      title="Play/Pause (Space)"
    >
      {ended ? "↺" : pause ? "▶" : "⏸"}
    </button>
  );
}

function TimeReadout({ clock }: { clock: PlaybackClock }) {
  // whole seconds: re-renders once a second, not on every time-pos tick
  const position = useClock(clock, (s) => Math.floor(Math.max(0, s.timePos)));
  const duration = useClock(clock, (s) => Math.floor(Math.max(0, s.duration)));
  return (
    <span className="font-mono text-xs text-ink-muted">
      {formatTime(position)} / {formatTime(duration)}
    </span>
  );
}

function SeekBar({ clock, onPoke }: { clock: PlaybackClock; onPoke: () => void }) {
  const timePos = useClock(clock, (s) => s.timePos);
  const duration = useClock(clock, (s) => s.duration);
  const [scrub, setScrub] = useState<number | null>(null);
  const value = scrub ?? timePos;
  return (
    <input
      type="range"
      min={0}
      max={Math.max(duration, 0.1)}
      step={0.1}
      value={Math.min(value, Math.max(duration, 0.1))}
      className="h-1.5 w-full cursor-pointer accent-(--color-accent)"
      onInput={(e) => {
        setScrub(Number(e.currentTarget.value));
        onPoke();
      }}
      onChange={(e) => {
        const target = Number(e.currentTarget.value);
        void playerCommand("seek", target, "absolute");
        setScrub(null);
      }}
      disabled={duration <= 0}
    />
  );
}

function VolumeControl({
  volume,
  mute,
  onPoke,
}: {
  volume: number;
  mute: boolean;
  onPoke: () => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <button
        className="w-8 rounded-md bg-canvas/50 py-1 text-sm hover:bg-elevated"
        onClick={() => {
          void playerSetProperty("mute", !mute);
          onPoke();
        }}
        title="Mute (m)"
      >
        {mute ? "🔇" : "🔊"}
      </button>
      <input
        type="range"
        min={0}
        max={100}
        step={1}
        value={mute ? 0 : Math.round(volume)}
        className="w-24 cursor-pointer accent-(--color-accent)"
        onInput={(e) => {
          void playerSetProperty("volume", Number(e.currentTarget.value));
          onPoke();
        }}
      />
    </div>
  );
}

function TrackMenu({
  label,
  type,
  tracks,
  current,
  property,
  allowOff = false,
}: {
  label: string;
  type: MpvTrack["type"];
  tracks: MpvTrack[];
  current: string;
  property: "aid" | "sid" | "vid";
  allowOff?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const options = tracks.filter((t) => t.type === type);
  if (options.length === 0 && !allowOff) return null;

  const trackName = (t: MpvTrack) =>
    [t.title, t.lang?.toUpperCase(), t["demux-h"] ? `${t["demux-h"]}p` : null]
      .filter(Boolean)
      .join(" · ") || `Track ${t.id}`;

  return (
    <div className="relative">
      <button
        className="rounded-md bg-canvas/50 px-3 py-1 text-sm text-ink-muted hover:bg-elevated hover:text-ink"
        onClick={() => setOpen((o) => !o)}
      >
        {label}
      </button>
      {open && (
        <div className="absolute bottom-9 right-0 z-10 max-h-64 w-56 overflow-y-auto rounded-lg border border-edge bg-surface py-1 shadow-xl">
          {allowOff && (
            <TrackRow
              label="Off"
              selected={current === "no" || current === "false"}
              onPick={() => {
                void playerSetProperty(property, "no");
                setOpen(false);
              }}
            />
          )}
          {options.map((t) => (
            <TrackRow
              key={t.id}
              label={trackName(t)}
              selected={t.selected === true || current === String(t.id)}
              onPick={() => {
                void playerSetProperty(property, t.id);
                setOpen(false);
              }}
            />
          ))}
          {options.length === 0 && (
            <div className="px-3 py-2 text-xs text-ink-subtle">No {label.toLowerCase()} tracks</div>
          )}
        </div>
      )}
    </div>
  );
}

function TrackRow({
  label,
  selected,
  onPick,
}: {
  label: string;
  selected: boolean;
  onPick: () => void;
}) {
  return (
    <button
      className={`block w-full truncate px-3 py-1.5 text-left text-sm hover:bg-elevated ${
        selected ? "text-accent" : "text-ink-muted"
      }`}
      onClick={onPick}
    >
      {selected ? "✓ " : ""}
      {label}
    </button>
  );
}
