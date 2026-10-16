/**
 * The bottom control bar of the Player screen, for both backends: seek bar (with the
 * buffered range), play/pause, time, volume and mute, audio/subtitle track menus,
 * playback speed, fullscreen and — for an external mpv window — Stop. Position-driven
 * parts subscribe to the playback clock themselves, so a time-pos tick re-renders only
 * them.
 */
import { Maximize, Pause, Play, RotateCcw, Square, Volume2, VolumeX } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { type MpvTrack, playerCommand, playerSetProperty } from "../../lib/player-ipc";
import { formatTime, type MpvState } from "./mpv-state";
import { type PlaybackClock, useClock } from "./playback-clock";
import { type PlayerControls, VOLUME_MAX } from "./usePlayerControls";

const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;

const chip =
  "flex h-8 items-center gap-1.5 rounded-md bg-canvas/50 px-3 text-sm text-ink-muted hover:bg-elevated hover:text-ink";

export function ControlsBar({
  clock,
  ended,
  mpv,
  controls,
  shown,
  onPoke,
  onToggleFullscreen,
  fullscreenTitle = "Fullscreen (f)",
  onStop,
  stopping = false,
  footnote,
}: {
  clock: PlaybackClock;
  ended: boolean;
  mpv: MpvState;
  controls: PlayerControls;
  shown: boolean;
  onPoke: () => void;
  onToggleFullscreen: () => void;
  fullscreenTitle?: string;
  /** external mpv: a Stop button that closes the mpv window */
  onStop?: () => void;
  stopping?: boolean;
  /** a quiet line under the controls (e.g. what Back does) */
  footnote?: ReactNode;
}) {
  return (
    <div
      className={`absolute inset-x-0 bottom-0 flex flex-col gap-1.5 bg-gradient-to-t from-canvas/90 to-transparent px-4 pb-3 pt-10 transition-opacity duration-300 ${
        shown ? "opacity-100" : "pointer-events-none opacity-0"
      }`}
    >
      <SeekBar clock={clock} onPoke={onPoke} />
      <div className="flex items-center gap-3">
        <PlayPauseButton clock={clock} ended={ended} onClick={controls.togglePauseOrReplay} />
        <TimeReadout clock={clock} />
        <VolumeControl volume={mpv.volume} mute={mpv.mute} onMute={controls.toggleMute} onPoke={onPoke} />
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
        <SpeedMenu speed={mpv.speed} />
        <button
          className={chip}
          onClick={onToggleFullscreen}
          title={fullscreenTitle}
          aria-label={fullscreenTitle}
        >
          <Maximize size={15} />
        </button>
        {onStop && (
          <button
            disabled={stopping}
            className="flex h-8 items-center gap-1.5 rounded-md bg-danger/15 px-3 text-sm font-semibold text-[oklch(0.75_0.12_25)] transition-colors hover:bg-danger/25 disabled:opacity-60"
            onClick={onStop}
            title="Stop and close mpv"
          >
            <Square size={13} fill="currentColor" />
            {stopping ? "Stopping…" : "Stop"}
          </button>
        )}
      </div>
      {footnote && <div className="text-[11.5px] text-ink-subtle">{footnote}</div>}
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
  const label = ended ? "Replay" : pause ? "Play" : "Pause";
  return (
    <button
      className="flex h-8 w-10 items-center justify-center rounded-md bg-canvas/50 text-ink hover:bg-elevated"
      onClick={onClick}
      title={`${label} (Space)`}
      aria-label={label}
    >
      {ended ? (
        <RotateCcw size={16} />
      ) : pause ? (
        <Play size={16} fill="currentColor" />
      ) : (
        <Pause size={16} fill="currentColor" />
      )}
    </button>
  );
}

function TimeReadout({ clock }: { clock: PlaybackClock }) {
  // whole seconds: re-renders once a second, not on every time-pos tick
  const position = useClock(clock, (s) => Math.floor(Math.max(0, s.timePos)));
  const duration = useClock(clock, (s) => Math.floor(Math.max(0, s.duration)));
  return (
    <span className="font-mono text-xs text-ink-muted">
      {formatTime(position)} / {duration > 0 ? formatTime(duration) : "--:--"}
    </span>
  );
}

/** Seek slider over a track that shows what is played and what is buffered. */
function SeekBar({ clock, onPoke }: { clock: PlaybackClock; onPoke: () => void }) {
  const timePos = useClock(clock, (s) => s.timePos);
  const duration = useClock(clock, (s) => s.duration);
  const cachedUntil = useClock(clock, (s) => s.cachedUntil);
  const [scrub, setScrub] = useState<number | null>(null);
  const max = Math.max(duration, 0.1);
  const value = Math.min(scrub ?? timePos, max);
  const pct = (t: number) => `${Math.min(100, Math.max(0, (t / max) * 100))}%`;
  return (
    <div className="relative flex h-3 items-center">
      <div className="absolute inset-x-0 h-1 overflow-hidden rounded-full bg-edge">
        {duration > 0 && cachedUntil > 0 && (
          <div
            className="absolute inset-y-0 left-0 bg-ink-subtle/70"
            style={{ width: pct(cachedUntil) }}
            title="Buffered"
          />
        )}
        <div className="absolute inset-y-0 left-0 bg-accent" style={{ width: pct(value) }} />
      </div>
      <input
        type="range"
        aria-label="Seek"
        data-seek-slider="" // SEEK_SLIDER_ATTR: ←/→ seek 5 s here too
        min={0}
        max={max}
        step={0.1}
        value={value}
        className="absolute inset-0 h-3 w-full cursor-pointer appearance-none bg-transparent disabled:cursor-default [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-accent"
        onInput={(e) => {
          setScrub(Number(e.currentTarget.value));
          onPoke();
        }}
        onChange={(e) => {
          const target = Number(e.currentTarget.value);
          void playerCommand("seek", target, "absolute").catch(() => undefined);
          setScrub(null);
        }}
        disabled={duration <= 0}
      />
    </div>
  );
}

function VolumeControl({
  volume,
  mute,
  onMute,
  onPoke,
}: {
  volume: number;
  mute: boolean;
  onMute: () => void;
  onPoke: () => void;
}) {
  // Normally 0…VOLUME_MAX; external mpv can be louder (the user's mpv.conf or its own
  // keys), so the scale grows to the loudest volume seen instead of pinning it at 100.
  const level = Math.max(0, Math.round(volume));
  const [ceiling, setCeiling] = useState(VOLUME_MAX);
  if (level > ceiling) setCeiling(level);
  return (
    <div className="flex items-center gap-2">
      <button
        className="flex h-8 w-8 items-center justify-center rounded-md bg-canvas/50 text-ink-muted hover:bg-elevated hover:text-ink"
        onClick={onMute}
        title={mute ? "Unmute (m)" : "Mute (m)"}
        aria-label={mute ? "Unmute" : "Mute"}
      >
        {mute ? <VolumeX size={16} /> : <Volume2 size={16} />}
      </button>
      <input
        type="range"
        aria-label="Volume"
        min={0}
        max={Math.max(ceiling, level)}
        step={1}
        value={mute ? 0 : level}
        className="w-24 cursor-pointer accent-(--color-accent)"
        onInput={(e) => {
          void playerSetProperty("volume", Number(e.currentTarget.value)).catch(() => undefined);
          if (mute) void playerSetProperty("mute", false).catch(() => undefined);
          onPoke();
        }}
      />
    </div>
  );
}

/** A button with a small menu above it; closes on a pick, a click elsewhere or Escape
 * (which then does not also leave the player). */
function PopupMenu({
  label,
  title,
  children,
}: {
  label: ReactNode;
  title?: string;
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    // capture phase: runs before the player's window shortcuts, which skip a
    // prevented event
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);
  return (
    <div ref={ref} className="relative">
      <button className={chip} onClick={() => setOpen((o) => !o)} title={title} aria-expanded={open}>
        {label}
      </button>
      {open && (
        <div className="absolute bottom-10 right-0 z-10 max-h-64 w-56 overflow-y-auto rounded-lg border border-edge bg-surface py-1 shadow-xl">
          {children(() => setOpen(false))}
        </div>
      )}
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
  const options = tracks.filter((t) => t.type === type);
  if (options.length === 0 && !allowOff) return null;

  const trackName = (t: MpvTrack) =>
    [t.title, t.lang?.toUpperCase(), t["demux-h"] ? `${t["demux-h"]}p` : null]
      .filter(Boolean)
      .join(" · ") || `Track ${t.id}`;
  const pick = (value: number | string, close: () => void) => {
    void playerSetProperty(property, value).catch(() => undefined);
    close();
  };

  return (
    <PopupMenu label={label}>
      {(close) => (
        <>
          {allowOff && (
            <MenuRow
              label="Off"
              selected={current === "no" || current === "false"}
              onPick={() => pick("no", close)}
            />
          )}
          {options.map((t) => (
            <MenuRow
              key={t.id}
              label={trackName(t)}
              selected={t.selected === true || current === String(t.id)}
              onPick={() => pick(t.id, close)}
            />
          ))}
          {options.length === 0 && (
            <div className="px-3 py-2 text-xs text-ink-subtle">No {label.toLowerCase()} tracks</div>
          )}
        </>
      )}
    </PopupMenu>
  );
}

function SpeedMenu({ speed }: { speed: number }) {
  const label = (s: number) => (s === 1 ? "Normal" : `${s}×`);
  return (
    <PopupMenu label={speed === 1 ? "1×" : `${Math.round(speed * 100) / 100}×`} title="Playback speed">
      {(close) =>
        SPEEDS.map((s) => (
          <MenuRow
            key={s}
            label={label(s)}
            selected={Math.abs(speed - s) < 0.001}
            onPick={() => {
              void playerSetProperty("speed", s).catch(() => undefined);
              close();
            }}
          />
        ))
      }
    </PopupMenu>
  );
}

function MenuRow({
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
