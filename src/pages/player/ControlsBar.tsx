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
import { type MpvChapter, playerCommand, playerSetProperty } from "../../lib/player-ipc";
import { AudioMenu } from "./AudioMenu";
import { chip, MenuRow, PopupMenu } from "./menu";
import { formatTime, type MpvState } from "./mpv-state";
import { type PlaybackClock, useClock } from "./playback-clock";
import { useLive } from "./live";
import { SubtitleMenu } from "./SubtitleMenu";
import { QualityMenu } from "./QualityMenu";
import { SPEEDS, type PlayerControls } from "./usePlayerControls";
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
  // live TV: nothing to seek in or speed up, so no seek bar, time or speed
  const live = useLive();
  return (
    <div
      className={`absolute inset-x-0 bottom-0 flex flex-col gap-1.5 bg-gradient-to-t from-canvas/90 to-transparent px-4 pb-3 pt-10 transition-opacity duration-300 ${
        shown ? "opacity-100" : "pointer-events-none opacity-0"
      }`}
    >
      {tracks.notice && <Notice text={tracks.notice} onDismiss={tracks.dismissNotice} />}
      {!live && <SeekBar clock={clock} chapters={mpv.chapters} onPoke={onPoke} />}
      <div className="flex items-center gap-3">
        <PlayPauseButton clock={clock} ended={ended} onClick={controls.togglePauseOrReplay} />
        {live ? <LiveBadge /> : <TimeReadout clock={clock} />}
        <VolumeControl
          volume={mpv.volume}
          mute={mpv.mute}
          max={tracks.volumeMax}
          onMute={controls.toggleMute}
          onPoke={onPoke}
        />
        <div className="flex-1" />
        <QualityMenu mpv={mpv} />
        <AudioMenu tracks={tracks} mpv={mpv} />
        <SubtitleMenu tracks={tracks} mpv={mpv} />
        {!live && <SpeedMenu speed={mpv.speed} />}
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

function LiveBadge() {
  return (
    <span className="flex items-center gap-1.5 rounded-md bg-danger/15 px-2 py-0.5 text-[11px] font-bold uppercase tracking-[0.18em] text-[oklch(0.75_0.12_25)]">
      <span className="h-1.5 w-1.5 rounded-full bg-[oklch(0.7_0.2_25)]" aria-hidden />
      Live
    </span>
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

/** Seek slider over a track that shows what is played and what is buffered, with a tick
 * at each chapter; hovering shows the time there and its chapter's title. */
function SeekBar({
  clock,
  chapters,
  onPoke,
}: {
  clock: PlaybackClock;
  chapters: MpvChapter[];
  onPoke: () => void;
}) {
  const timePos = useClock(clock, (s) => s.timePos);
  const duration = useClock(clock, (s) => s.duration);
  const cachedUntil = useClock(clock, (s) => s.cachedUntil);
  const [scrub, setScrub] = useState<number | null>(null);
  const max = Math.max(duration, 0.1);
  const value = Math.min(scrub ?? timePos, max);
  const pct = (t: number) => `${Math.min(100, Math.max(0, (t / max) * 100))}%`;
  const [hover, setHover] = useState<number | null>(null);
  const ticks = duration > 0 ? chapters.filter((c) => c.time > 0.5 && c.time < duration - 0.5) : [];
  const hoverChapter =
    hover === null ? null : [...chapters].reverse().find((c) => c.time <= hover)?.title?.trim() || null;
  return (
    <div
      className="relative flex h-3 items-center"
      onMouseMove={(e) => {
        if (duration <= 0) return;
        const rect = e.currentTarget.getBoundingClientRect();
        setHover(Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width)) * duration);
      }}
      onMouseLeave={() => setHover(null)}
    >
      {hover !== null && (
        <div
          className="pointer-events-none absolute bottom-4 -translate-x-1/2 whitespace-nowrap rounded bg-surface/95 px-2 py-0.5 text-[11px] text-ink shadow-lg"
          style={{ left: pct(hover) }}
        >
          {formatTime(hover)}
          {hoverChapter && <span className="text-ink-muted"> · {hoverChapter}</span>}
        </div>
      )}
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
      {ticks.map((c, i) => (
        <div
          key={`${i}-${c.time}`}
          className="pointer-events-none absolute top-1/2 h-2 w-0.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-canvas/80"
          style={{ left: pct(c.time) }}
        />
      ))}
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
  max,
  onMute,
  onPoke,
}: {
  volume: number;
  mute: boolean;
  /** `audio.volumeMax` */
  max: number;
  onMute: () => void;
  onPoke: () => void;
}) {
  // Normally 0…max; external mpv can be louder (its own keys), so the scale grows to
  // the loudest volume seen instead of pinning it.
  const level = Math.max(0, Math.round(volume));
  const [loudest, setLoudest] = useState(0);
  if (level > loudest) setLoudest(level);
  const ceiling = Math.max(max, loudest);
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
