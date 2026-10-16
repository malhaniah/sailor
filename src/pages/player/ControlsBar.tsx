/**
 * The bottom control bar of the Player screen, for both backends: seek bar (with the
 * buffered range), play/pause, time, volume and mute, the audio and subtitle menus
 * (AudioMenu, SubtitleMenu: tracks, sources, delays, quick style), playback speed,
 * fullscreen and — for an external mpv window — Stop. Position-driven
 * parts subscribe to the playback clock themselves, so a time-pos tick re-renders only
 * them.
 */
import { Maximize, Pause, Play, RotateCcw, Square, Volume2, VolumeX } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { playerCommand, playerSetProperty } from "../../lib/player-ipc";
import { AudioMenu } from "./AudioMenu";
import { chip, MenuRow, PopupMenu } from "./menu";
import { formatTime, type MpvState } from "./mpv-state";
import { type PlaybackClock, useClock } from "./playback-clock";
import { SubtitleMenu } from "./SubtitleMenu";
import { SPEEDS, type PlayerControls, VOLUME_MAX } from "./usePlayerControls";
import type { Tracks } from "./useTracks";

export function ControlsBar({
  clock,
  ended,
  mpv,
  tracks,
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
  tracks: Tracks;
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
      {tracks.notice && <Notice text={tracks.notice} onDismiss={tracks.dismissNotice} />}
      <SeekBar clock={clock} onPoke={onPoke} />
      <div className="flex items-center gap-3">
        <PlayPauseButton clock={clock} ended={ended} onClick={controls.togglePauseOrReplay} />
        <TimeReadout clock={clock} />
        <VolumeControl volume={mpv.volume} mute={mpv.mute} onMute={controls.toggleMute} onPoke={onPoke} />
        <div className="flex-1" />
        <AudioMenu tracks={tracks} mpv={mpv} />
        <SubtitleMenu tracks={tracks} mpv={mpv} />
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

function SpeedMenu({ speed }: { speed: number }) {
  const label = (s: number) => (s === 1 ? "Normal" : `${s}×`);
  return (
    <PopupMenu
      label={speed === 1 ? "1×" : `${Math.round(speed * 100) / 100}×`}
      title="Playback speed ([ / ])"
    >
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

/** A passing message from the menus (a dropped file was added, a load failed). */
function Notice({ text, onDismiss }: { text: string; onDismiss: () => void }) {
  useEffect(() => {
    const timer = window.setTimeout(onDismiss, 6000);
    return () => window.clearTimeout(timer);
  }, [text, onDismiss]);
  return (
    <button
      onClick={onDismiss}
      className="max-w-[60%] self-start truncate rounded-md bg-surface/90 px-3 py-1.5 text-xs text-ink-muted shadow-lg"
      title={text}
    >
      {text}
    </button>
  );
}
