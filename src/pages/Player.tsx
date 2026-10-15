/**
 * In-app player page (embedded libmpv).
 *
 * The video is NOT an element on this page: mpv renders into a native child
 * window pinned underneath the (transparent) webview. This page makes the
 * document background transparent while mounted, keeps the native video rect
 * in sync with the layout (`player_set_geometry`, physical px), and draws the
 * controls as overlays.
 *
 * Route: #/player?… with either
 *   url=<http(s) stream>            direct / already-resolved stream
 *   infoHash=<40 hex>&sources=<json array>&fileIdx=<n>&filename=<name>
 *     &fileMustInclude=<json array> torrent — resolved here via engine_resolve
 * plus optional title, headers (JSON object), ua, referrer, startAt (seconds).
 *
 * If the embedded backend is unavailable (feature off / libmpv-2.dll missing),
 * the page falls back to launching the external mpv backend.
 */
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  engineRemove,
  engineResolve,
  engineStats,
  errorMessage,
  type MpvTrack,
  onPlayerEvent,
  type PlaybackRequest,
  playerCommand,
  playerEmbeddedAvailable,
  playerEmbeddedStart,
  playerEmbeddedStop,
  playerExternalPlay,
  type PlayerEvent,
  playerSetGeometry,
  playerSetProperty,
  settingsGet,
} from "../lib/player-ipc";

// --- route params -----------------------------------------------------------------------

interface PlayerParams {
  url: string | null;
  title: string | null;
  infoHash: string | null;
  fileIdx: number | null;
  sources: string[];
  filename: string | null;
  fileMustInclude: string[];
  headers: [string, string][];
  userAgent: string | null;
  referrer: string | null;
  startAt: number | null;
}

function parseParams(): PlayerParams {
  const query = window.location.hash.split("?")[1] ?? "";
  const p = new URLSearchParams(query);
  let headers: [string, string][] = [];
  try {
    const parsed: unknown = JSON.parse(p.get("headers") ?? "{}");
    if (parsed && typeof parsed === "object") {
      headers = Object.entries(parsed as Record<string, string>).map(([k, v]) => [k, String(v)]);
    }
  } catch {
    /* addon data is messy; ignore bad header blobs */
  }
  const stringList = (name: string) => {
    try {
      const parsed: unknown = JSON.parse(p.get(name) ?? "[]");
      return Array.isArray(parsed) ? parsed.map(String) : [];
    } catch {
      return [];
    }
  };
  const num = (name: string) => {
    const v = p.get(name);
    const n = v === null ? NaN : Number(v);
    return Number.isFinite(n) ? n : null;
  };
  return {
    url: p.get("url"),
    title: p.get("title"),
    infoHash: p.get("infoHash"),
    fileIdx: num("fileIdx"),
    sources: stringList("sources"),
    filename: p.get("filename"),
    fileMustInclude: stringList("fileMustInclude"),
    headers,
    userAgent: p.get("ua"),
    referrer: p.get("referrer"),
    startAt: num("startAt"),
  };
}

// --- mpv property state -----------------------------------------------------------------

interface MpvState {
  timePos: number;
  duration: number;
  pause: boolean;
  eofReached: boolean;
  pausedForCache: boolean;
  buffering: number; // cache-buffering-state, 0-100
  volume: number;
  mute: boolean;
  mediaTitle: string | null;
  tracks: MpvTrack[];
  aid: string;
  sid: string;
  vid: string;
}

const INITIAL_MPV: MpvState = {
  timePos: 0,
  duration: 0,
  pause: false,
  eofReached: false,
  pausedForCache: false,
  buffering: 100,
  volume: 100,
  mute: false,
  mediaTitle: null,
  tracks: [],
  aid: "auto",
  sid: "auto",
  vid: "auto",
};

function mpvReducer(state: MpvState, event: PlayerEvent): MpvState {
  if (event.type === "start-file") {
    return { ...state, timePos: 0, duration: 0, eofReached: false, tracks: [] };
  }
  if (event.type !== "property") return state;
  const v = event.value;
  switch (event.name) {
    case "time-pos":
      return typeof v === "number" ? { ...state, timePos: v } : state;
    case "duration":
      return typeof v === "number" ? { ...state, duration: v } : state;
    case "pause":
      return typeof v === "boolean" ? { ...state, pause: v } : state;
    case "eof-reached":
      return typeof v === "boolean" ? { ...state, eofReached: v } : state;
    case "paused-for-cache":
      return typeof v === "boolean" ? { ...state, pausedForCache: v } : state;
    case "cache-buffering-state":
      return typeof v === "number" ? { ...state, buffering: v } : state;
    case "volume":
      return typeof v === "number" ? { ...state, volume: v } : state;
    case "mute":
      return typeof v === "boolean" ? { ...state, mute: v } : state;
    case "media-title":
      return typeof v === "string" ? { ...state, mediaTitle: v } : state;
    case "track-list":
      return Array.isArray(v) ? { ...state, tracks: v as MpvTrack[] } : state;
    case "aid":
      return { ...state, aid: String(v) };
    case "sid":
      return { ...state, sid: String(v) };
    case "vid":
      return { ...state, vid: String(v) };
    default:
      return state;
  }
}

type Phase =
  | { kind: "loading"; label: string }
  | { kind: "playing" }
  | { kind: "external" }
  | { kind: "ended" }
  | { kind: "error"; message: string };

function formatTime(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(sec).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

// --- page -------------------------------------------------------------------------------

export function PlayerPage() {
  const params = useMemo(parseParams, []);
  const [phase, setPhase] = useState<Phase>({ kind: "loading", label: "Preparing player…" });
  const [mpv, dispatch] = useReducer(mpvReducer, INITIAL_MPV);
  const [notice, setNotice] = useState<string | null>(null);
  const videoRef = useRef<HTMLDivElement>(null);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  // While this page is up, the document must not paint a background —
  // the native video window sits directly behind the webview.
  useEffect(() => {
    const targets = [document.documentElement, document.body, document.getElementById("root")]
      .filter((t): t is HTMLElement => t !== null);
    const previous = targets.map((t) => t.style.background);
    for (const t of targets) t.style.background = "transparent";
    return () => targets.forEach((t, i) => (t.style.background = previous[i]));
  }, []);

  const sendGeometry = useCallback(() => {
    const el = videoRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const dpr = window.devicePixelRatio;
    void playerSetGeometry({
      x: rect.left * dpr,
      y: rect.top * dpr,
      width: rect.width * dpr,
      height: rect.height * dpr,
    }).catch(() => undefined);
  }, []);

  // Keep the native video rect glued to the layout.
  useEffect(() => {
    sendGeometry();
    const el = videoRef.current;
    const observer = new ResizeObserver(sendGeometry);
    if (el) observer.observe(el);
    window.addEventListener("resize", sendGeometry);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", sendGeometry);
    };
  }, [sendGeometry]);

  // Resolve (torrents), start playback, pump events.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;

    (async () => {
      const request: PlaybackRequest = {
        url: params.url ?? "",
        title: params.title ?? params.filename,
        headers: params.headers,
        userAgent: params.userAgent,
        referrer: params.referrer,
        startAtSec: params.startAt,
      };

      const embedded = await playerEmbeddedAvailable().catch(() => false);

      if (!request.url && params.infoHash) {
        setPhase({ kind: "loading", label: "Finding peers and fetching metadata…" });
        const resolved = await engineResolve({
          infoHash: params.infoHash,
          fileIdx: params.fileIdx,
          sources: params.sources,
          filename: params.filename,
          fileMustInclude: params.fileMustInclude,
        });
        if (cancelled) return;
        request.url = resolved.streamUrl;
        request.title ??= resolved.fileName ?? null;
      }
      if (!request.url) throw new Error("nothing to play: the route carried no url/infoHash");

      if (!embedded) {
        await playerExternalPlay(request);
        if (!cancelled) setPhase({ kind: "external" });
        return;
      }

      unlisten = await onPlayerEvent((event) => {
        dispatch(event);
        if (event.type === "file-loaded" || event.type === "playback-restart") {
          sendGeometry(); // the mpv child may have just (re)appeared
          if (phaseRef.current.kind === "loading") setPhase({ kind: "playing" });
        } else if (event.type === "end-file") {
          if (event.reason === "eof") setPhase({ kind: "ended" });
          else if (event.reason === "error" && phaseRef.current.kind !== "error") {
            setPhase({ kind: "error", message: "playback failed (stream error)" });
          }
        } else if (event.type === "error") {
          // non-fatal unless nothing ever loaded
          if (phaseRef.current.kind === "loading") {
            setPhase({ kind: "error", message: event.message });
          } else {
            setNotice(event.message);
          }
        } else if (event.type === "property" && event.name === "eof-reached") {
          if (event.value === true) setPhase({ kind: "ended" });
          else if (phaseRef.current.kind === "ended") setPhase({ kind: "playing" });
        }
      });
      if (cancelled) return;

      setPhase({ kind: "loading", label: "Starting playback…" });
      await playerEmbeddedStart(request);
      if (!cancelled) sendGeometry();
    })().catch((e) => {
      if (!cancelled) setPhase({ kind: "error", message: errorMessage(e) });
    });

    return () => {
      cancelled = true;
      unlisten?.();
      void playerEmbeddedStop().catch(() => undefined);
    };
    // params is stable for the lifetime of the page (parsed once from the hash)
  }, [params, sendGeometry]);

  const leave = useCallback(async () => {
    void playerEmbeddedStop().catch(() => undefined);
    if (params.infoHash) {
      try {
        const settings = await settingsGet();
        await engineRemove(params.infoHash, settings.deleteOnStop);
      } catch {
        /* best effort */
      }
    }
    window.history.back();
  }, [params.infoHash]);

  return (
    <PlayerChrome
      phase={phase}
      mpv={mpv}
      notice={notice}
      dismissNotice={() => setNotice(null)}
      title={mpv.mediaTitle ?? params.title ?? params.filename ?? "Now playing"}
      infoHash={params.infoHash}
      videoRef={videoRef}
      onLeave={() => void leave()}
    />
  );
}

// --- chrome (everything drawn above the video) ------------------------------------------

function PlayerChrome({
  phase,
  mpv,
  notice,
  dismissNotice,
  title,
  infoHash,
  videoRef,
  onLeave,
}: {
  phase: Phase;
  mpv: MpvState;
  notice: string | null;
  dismissNotice: () => void;
  title: string;
  infoHash: string | null;
  videoRef: React.RefObject<HTMLDivElement | null>;
  onLeave: () => void;
}) {
  const [visible, setVisible] = useState(true);
  const hideTimer = useRef<number | null>(null);
  const playing = phase.kind === "playing";

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
  const controlsShown = visible || !playing || mpv.pause;

  const togglePauseOrReplay = useCallback(() => {
    if (phase.kind === "ended") {
      void playerCommand("seek", 0, "absolute");
      void playerSetProperty("pause", false);
    } else {
      void playerSetProperty("pause", !mpv.pause);
    }
    poke();
  }, [phase.kind, mpv.pause, poke]);

  const seekBy = useCallback(
    (delta: number) => {
      void playerCommand("seek", delta, "relative");
      poke();
    },
    [poke],
  );

  const toggleFullscreen = useCallback(async () => {
    const w = getCurrentWindow();
    await w.setFullscreen(!(await w.isFullscreen())).catch(() => undefined);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement) return;
      switch (e.key) {
        case " ":
          e.preventDefault();
          togglePauseOrReplay();
          break;
        case "ArrowLeft":
          seekBy(-5);
          break;
        case "ArrowRight":
          seekBy(5);
          break;
        case "ArrowUp":
          void playerSetProperty("volume", Math.min(100, Math.round(mpv.volume) + 5));
          poke();
          break;
        case "ArrowDown":
          void playerSetProperty("volume", Math.max(0, Math.round(mpv.volume) - 5));
          poke();
          break;
        case "m":
          void playerSetProperty("mute", !mpv.mute);
          poke();
          break;
        case "f":
          void toggleFullscreen();
          break;
        case "Escape":
          onLeave();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [togglePauseOrReplay, seekBy, toggleFullscreen, onLeave, mpv.volume, mpv.mute, poke]);

  return (
    <div
      className="fixed inset-0 overflow-hidden"
      style={{ background: "transparent", cursor: controlsShown ? "default" : "none" }}
      onMouseMove={poke}
      onClick={(e) => {
        if (e.target === e.currentTarget || e.target === videoRef.current) togglePauseOrReplay();
      }}
      onDoubleClick={() => void toggleFullscreen()}
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
      {phase.kind === "external" && (
        <CenterCard>
          <div className="text-base font-semibold">Playing in external mpv</div>
          <p className="max-w-sm text-sm text-ink-muted">
            This build has no embedded player (or libmpv is missing), so playback was handed to
            the external player. You can go back and keep browsing.
          </p>
          <button className="rounded-md bg-elevated px-4 py-2 text-sm hover:bg-raised" onClick={onLeave}>
            ← Back
          </button>
        </CenterCard>
      )}
      {phase.kind === "error" && (
        <CenterCard>
          <div className="text-base font-semibold text-danger">Could not play</div>
          <p className="max-w-md break-words text-sm text-ink-muted">{phase.message}</p>
          <button className="rounded-md bg-elevated px-4 py-2 text-sm hover:bg-raised" onClick={onLeave}>
            ← Back
          </button>
        </CenterCard>
      )}
      {phase.kind === "playing" && mpv.pausedForCache && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <div className="flex items-center gap-3 rounded-lg bg-canvas/70 px-4 py-3">
            <Spinner />
            <span className="text-sm text-ink-muted">Buffering… {mpv.buffering}%</span>
          </div>
        </div>
      )}
      {phase.kind === "ended" && (
        <CenterCard>
          <div className="text-base font-semibold">Playback finished</div>
          <div className="flex gap-2">
            <button
              className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-canvas hover:opacity-90"
              onClick={togglePauseOrReplay}
            >
              ↺ Replay
            </button>
            <button className="rounded-md bg-elevated px-4 py-2 text-sm hover:bg-raised" onClick={onLeave}>
              ← Back
            </button>
          </div>
        </CenterCard>
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
        <div
          className={`absolute inset-x-0 bottom-0 flex flex-col gap-1.5 bg-gradient-to-t from-canvas/90 to-transparent px-4 pb-3 pt-10 transition-opacity duration-300 ${
            controlsShown ? "opacity-100" : "pointer-events-none opacity-0"
          }`}
        >
          <SeekBar timePos={mpv.timePos} duration={mpv.duration} onPoke={poke} />
          <div className="flex items-center gap-4">
            <button
              className="w-10 rounded-md bg-canvas/50 py-1 text-lg hover:bg-elevated"
              onClick={togglePauseOrReplay}
              title="Play/Pause (Space)"
            >
              {phase.kind === "ended" ? "↺" : mpv.pause ? "▶" : "⏸"}
            </button>
            <span className="font-mono text-xs text-ink-muted">
              {formatTime(mpv.timePos)} / {formatTime(mpv.duration)}
            </span>
            <VolumeControl volume={mpv.volume} mute={mpv.mute} onPoke={poke} />
            <div className="flex-1" />
            <TrackMenu label="Audio" type="audio" tracks={mpv.tracks} current={mpv.aid} property="aid" />
            <TrackMenu label="Subtitles" type="sub" tracks={mpv.tracks} current={mpv.sid} property="sid" allowOff />
            <button
              className="rounded-md bg-canvas/50 px-3 py-1 text-sm text-ink-muted hover:bg-elevated hover:text-ink"
              onClick={() => void toggleFullscreen()}
              title="Fullscreen (f)"
            >
              ⛶
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function CenterCard({ children }: { children: React.ReactNode }) {
  return (
    <div className="absolute inset-0 flex items-center justify-center">
      <div className="flex flex-col items-center gap-3 rounded-xl bg-canvas/80 px-8 py-6 text-center">
        {children}
      </div>
    </div>
  );
}

function Spinner() {
  return (
    <div className="h-6 w-6 animate-spin rounded-full border-2 border-edge border-t-accent" />
  );
}

function SeekBar({
  timePos,
  duration,
  onPoke,
}: {
  timePos: number;
  duration: number;
  onPoke: () => void;
}) {
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

function TorrentStats({ infoHash, compact = false }: { infoHash: string; compact?: boolean }) {
  const [line, setLine] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        // raw librqbit TorrentStats JSON (snake_case)
        const stats = (await engineStats(infoHash)) as {
          progress_bytes?: number;
          total_bytes?: number;
          live?: {
            download_speed?: { human_readable?: string };
            snapshot?: { peer_stats?: { live?: number } };
          } | null;
        };
        if (cancelled) return;
        const parts: string[] = [];
        const speed = stats.live?.download_speed?.human_readable;
        if (speed) parts.push(`↓ ${speed}`);
        const peers = stats.live?.snapshot?.peer_stats?.live;
        if (typeof peers === "number") parts.push(`${peers} peers`);
        if (
          typeof stats.progress_bytes === "number" &&
          typeof stats.total_bytes === "number" &&
          stats.total_bytes > 0
        ) {
          parts.push(`${Math.round((stats.progress_bytes / stats.total_bytes) * 100)}%`);
        }
        setLine(parts.join(" · ") || null);
      } catch {
        if (!cancelled) setLine(null);
      }
    };
    void tick();
    const id = window.setInterval(() => void tick(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [infoHash]);
  if (!line) return null;
  return (
    <span className={`font-mono text-xs text-ink-subtle ${compact ? "" : "mt-1"}`}>{line}</span>
  );
}
