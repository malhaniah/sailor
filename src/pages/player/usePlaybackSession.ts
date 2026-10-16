/**
 * One playback session: resolves torrents, starts playback through the unified player
 * API and pumps `player://event`s into the page state and the playback clock.
 *
 * Rust picks the backend (`player_start`, see docs/ARCHITECTURE.md "Player backends"):
 * the launch's `backend` ("auto", or "external" when the picker forced it) goes in, the
 * backend actually used comes out, with `fallbackReason` when built-in playback was
 * wanted but failed and mpv took over. Both backends report the same events, so the
 * page drives either with the same calls (`player_set_property`, `player_command`):
 * - embedded libmpv (Windows `embedded` builds): the video sits under the transparent
 *   webview;
 * - external mpv: its own window, remote-controlled over mpv's JSON IPC. Only when that
 *   IPC never comes up (`remoteControl: false`) does the page fall back to the passive
 *   "Playing in mpv" card.
 *
 * The session owns its torrent: however the page goes away (Back, Escape, native history
 * navigation, an error), the player is stopped and a torrent it resolved is removed from
 * the engine (honouring `deleteOnStop`) — including one whose resolve only completes
 * after the page is gone, since the engine holds a magnet only once its metadata
 * arrives. The one exception is external mpv that is playing: Back keeps that window
 * playing (and its torrent streaming), as in M1; its Stop button ends both. Back while
 * mpv is still opening the stream cancels it instead (`backKeepsPlaying`).
 */
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { errorKind } from "../../lib/ipc";
import {
  leavePlayer,
  type PlayerLaunch,
  playerActive,
  urlRequest,
} from "../../lib/playback-handoff";
import {
  type ActiveBackend,
  engineRemove,
  engineResolve,
  errorMessage,
  onPlayerEvent,
  type PlaybackRequest,
  type PlayerEvent,
  playerBackends,
  playerStart,
  playerStop,
  settingsGet,
} from "../../lib/player-ipc";
import { type Backend, INITIAL_MPV, type MpvState, mpvReducer, type Phase } from "./mpv-state";
import { applyClockEvent, type PlaybackClock } from "./playback-clock";

/** Best-effort: drop a torrent from the engine (deleting its files per settings). */
export async function removeTorrent(infoHash: string): Promise<void> {
  try {
    const settings = await settingsGet();
    await engineRemove(infoHash, settings.deleteOnStop);
  } catch {
    /* best effort: the engine may not hold it (yet, or any more) */
  }
}

/**
 * Whether Back leaves external mpv running in this phase (the screen says so). Only
 * while it plays: Back while it is still opening cancels, and after it ended, failed
 * or was closed, Back closes mpv. (The passive "Playing in mpv" card keeps it too.)
 */
export function backKeepsPlaying(kind: Phase["kind"]): boolean {
  return kind === "playing" || kind === "uncontrolled";
}

/** The message for a failed start: an explicit Built-in choice that this build or OS
 * cannot honour points to the setting (which shows why, in Settings → Player). */
export function startErrorMessage(e: unknown): string {
  if (errorKind(e) === "unsupported") {
    return "Built-in playback is chosen in Settings → Player, but it isn't available on this computer. Choose Auto or External mpv there.";
  }
  return errorMessage(e);
}

/** Where the Rust-side player of this session stands. */
type PlayerSlot =
  | { state: "idle" } // player_start not sent yet
  | { state: "starting" } // player_start in flight
  | { state: "running"; backend: ActiveBackend }
  | { state: "gone" }; // stopped by us, shut down on its own, or never started

export interface PlaybackSession {
  phase: Phase;
  backend: Backend;
  /**
   * A built-in (embedded) session is running, so the native video sits behind the
   * page. Until then — and once it has stopped — the page paints its own background:
   * `backend` can already be "embedded" (the early guess) while nothing is behind it.
   */
  videoBehind: boolean;
  mpv: MpvState;
  /** a non-fatal player error, dismissible */
  notice: string | null;
  dismissNotice: () => void;
  /** why built-in playback fell back to external mpv, dismissible */
  fallbackReason: string | null;
  dismissFallback: () => void;
  /** start the same stream again (after mpv was closed) */
  playAgain: () => void;
  /** stop the player, drop the torrent, leave the page (external mpv's Stop) */
  stopAndLeave: () => Promise<void>;
}

export function usePlaybackSession(
  launch: PlayerLaunch,
  clock: PlaybackClock,
  sendGeometry: () => void,
): PlaybackSession {
  const [phase, setPhase] = useState<Phase>({ kind: "loading", label: "Preparing player…" });
  const [backend, setBackend] = useState<Backend>("unknown");
  const [videoBehind, setVideoBehind] = useState(false);
  const [mpv, dispatch] = useReducer(mpvReducer, INITIAL_MPV);
  const [notice, setNotice] = useState<string | null>(null);
  const [fallbackReason, setFallbackReason] = useState<string | null>(null);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const actions = useRef<{ playAgain: () => void; stopAndLeave: () => Promise<void> }>({
    playAgain: () => undefined,
    stopAndLeave: async () => leavePlayer(),
  });

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    let player: PlayerSlot = { state: "idle" };
    // set while our own Stop runs, so the shutdown it causes is not shown as "closed"
    let stopping = false;
    let request: PlaybackRequest | null = null;
    const source = launch.source;
    const torrentHash = source.kind === "torrent" ? source.infoHash : null;
    // the cleanup and a late start/resolve may both drop it: remove it once
    let torrentDropped = false;
    const dropTorrent = () => {
      if (!torrentHash || torrentDropped) return;
      torrentDropped = true;
      void removeTorrent(torrentHash);
    };

    const fail = (e: unknown) => {
      if (cancelled) return;
      setPhase({
        kind: "error",
        message: startErrorMessage(e),
        // mpv missing or failing to launch: a direct URL still plays in any other player
        streamUrl: errorKind(e) === "player" && source.kind === "url" ? source.url : undefined,
      });
    };

    const onEvent = (event: PlayerEvent) => {
      // events of a session left playing by an earlier launch, until ours replaces it
      if (player.state === "idle" || player.state === "gone") return;
      if (!applyClockEvent(clock, event)) dispatch(event);
      switch (event.type) {
        case "file-loaded":
        case "playback-restart":
          if (player.state === "running" && player.backend === "embedded") sendGeometry();
          if (phaseRef.current.kind === "loading") setPhase({ kind: "playing" });
          break;
        case "end-file":
          if (event.reason === "eof") setPhase({ kind: "ended" });
          else if (event.reason === "error" && phaseRef.current.kind !== "error") {
            setPhase({ kind: "error", message: "playback failed (stream error)" });
          }
          break;
        case "error":
          // fatal only if nothing ever loaded
          if (phaseRef.current.kind === "loading") {
            setPhase({ kind: "error", message: event.message });
          } else {
            setNotice(event.message);
          }
          break;
        case "property":
          if (event.name === "eof-reached") {
            if (event.value === true) setPhase({ kind: "ended" });
            else if (phaseRef.current.kind === "ended") setPhase({ kind: "playing" });
          }
          break;
        case "shutdown":
          player = { state: "gone" };
          setVideoBehind(false);
          if (!stopping && phaseRef.current.kind !== "error") setPhase({ kind: "closed" });
          break;
      }
    };

    /** player_start for the prepared request; Rust picks (and reports) the backend. */
    const start = async () => {
      if (!request) return;
      setPhase({ kind: "loading", label: "Starting playback…" });
      player = { state: "starting" };
      setVideoBehind(false);
      let result;
      try {
        result = await playerStart(request, launch.backend);
      } catch (e) {
        // A refused start leaves nothing of ours running: Rust validates the request
        // before it replaces the previous session, and a later failure has already
        // stopped what it spawned. So the cleanup must not stop a session an earlier
        // screen left playing (nor follow its events).
        player = { state: "gone" };
        throw e;
      }
      if (cancelled) {
        // The page left while the player was starting. Its cleanup sent a stop, but
        // Rust may have run it before this start; stop again unless another player
        // screen is already up (whose session this stop would hit instead).
        if (!playerActive()) void playerStop().catch(() => undefined);
        dropTorrent();
        return;
      }
      player = { state: "running", backend: result.backend };
      setBackend(result.backend);
      setVideoBehind(result.backend === "embedded");
      setFallbackReason(result.fallbackReason ?? null);
      if (!result.remoteControl) {
        setPhase({ kind: "uncontrolled" });
        return;
      }
      if (result.backend === "embedded") sendGeometry();
      // events may already have moved the phase on (file-loaded can beat the reply)
      setPhase((p) =>
        p.kind === "loading"
          ? {
              kind: "loading",
              label: result.backend === "external" ? "Opening in mpv…" : "Opening the stream…",
            }
          : p,
      );
    };

    actions.current = {
      playAgain: () => {
        dispatch({ type: "start-file" });
        void start().catch(fail);
      },
      stopAndLeave: async () => {
        stopping = true;
        await playerStop().catch(() => undefined);
        player = { state: "gone" };
        setVideoBehind(false);
        dropTorrent();
        leavePlayer();
      },
    };

    (async () => {
      // the backend this launch will most likely get, so the loading screen fits
      const [backends, settings] = await Promise.all([
        playerBackends().catch(() => null),
        settingsGet().catch(() => null),
      ]);
      if (cancelled) return;
      const wanted =
        launch.backend === "auto" ? (settings?.playerBackend ?? "auto") : launch.backend;
      setBackend(wanted !== "external" && backends?.embedded.available ? "embedded" : "external");

      const prepared: PlaybackRequest =
        source.kind === "url"
          ? urlRequest(source, launch)
          : {
              url: "",
              title: launch.title,
              headers: [],
              userAgent: null,
              referrer: null,
              startAtSec: launch.startAtSec,
            };
      if (source.kind === "torrent") {
        setPhase({ kind: "loading", label: "Finding peers and fetching metadata…" });
        const resolved = await engineResolve({
          infoHash: source.infoHash,
          fileIdx: source.fileIdx,
          sources: source.sources,
          filename: source.filename,
          fileMustInclude: source.fileMustInclude,
        });
        if (cancelled) {
          // The page left while the magnet was resolving: the cleanup's remove found
          // nothing (the engine holds a magnet only once its metadata arrives), so
          // remove the torrent it has only just added — past the run-once guard, which
          // that no-op remove already tripped.
          torrentDropped = true;
          void removeTorrent(source.infoHash);
          return;
        }
        prepared.url = resolved.streamUrl;
        prepared.title ??= resolved.fileName ?? null;
      }
      if (!prepared.url) throw new Error("nothing to play: the route carried no url/infoHash");
      request = prepared;

      unlisten = await onPlayerEvent(onEvent);
      if (cancelled) return;
      await start();
    })().catch(fail);

    return () => {
      cancelled = true;
      unlisten?.();
      unlisten = null;
      // Back keeps external mpv playing (and its torrent streaming), as in M1
      const keepPlaying =
        player.state === "running" &&
        player.backend === "external" &&
        backKeepsPlaying(phaseRef.current.kind);
      if (keepPlaying) return;
      const stopped =
        player.state === "starting" || player.state === "running"
          ? playerStop().catch(() => undefined)
          : Promise.resolve();
      player = { state: "gone" };
      void stopped.then(dropTorrent);
    };
    // launch and clock are fixed for the lifetime of the page
  }, [launch, clock, sendGeometry]);

  const playAgain = useCallback(() => actions.current.playAgain(), []);
  const stopAndLeave = useCallback(() => actions.current.stopAndLeave(), []);
  return {
    phase,
    backend,
    videoBehind,
    mpv,
    notice,
    dismissNotice: useCallback(() => setNotice(null), []),
    fallbackReason,
    dismissFallback: useCallback(() => setFallbackReason(null), []),
    playAgain,
    stopAndLeave,
  };
}
