/**
 * One playback session: picks the backend, resolves torrents, starts playback and pumps
 * player events into the page state and the playback clock.
 *
 * - Embedded builds (`embedded` feature + libmpv present): mpv renders under the
 *   transparent webview and reports `player://event`s.
 * - Every other build: playback is handed to an external mpv window via `player_play`
 *   (fire-and-forget until the M2 JSON-IPC backend), and the page shows "Playing in mpv".
 *
 * The session owns its torrent: however the page goes away (Back, Escape, native history
 * navigation, an error), a torrent it resolved is removed from the engine (honouring
 * `deleteOnStop`) — including one whose resolve only completes after the page is gone,
 * since the engine holds a magnet only once its metadata arrives. The one exception is a
 * torrent handed to external mpv: Back keeps that playing, and its Stop button removes it.
 */
import { useEffect, useReducer, useRef, useState } from "react";
import { errorKind } from "../../lib/ipc";
import { type PlayerLaunch, urlRequest } from "../../lib/playback-handoff";
import {
  engineRemove,
  engineResolve,
  errorMessage,
  onPlayerEvent,
  type PlaybackRequest,
  playerEmbeddedAvailable,
  playerEmbeddedStart,
  playerEmbeddedStop,
  playerExternalPlay,
  settingsGet,
} from "../../lib/player-ipc";
import { type Backend, INITIAL_MPV, mpvReducer, type Phase } from "./mpv-state";
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

export function usePlaybackSession(
  launch: PlayerLaunch,
  clock: PlaybackClock,
  sendGeometry: () => void,
) {
  const [phase, setPhase] = useState<Phase>({ kind: "loading", label: "Preparing player…" });
  const [backend, setBackend] = useState<Backend>("unknown");
  const [mpv, dispatch] = useReducer(mpvReducer, INITIAL_MPV);
  const [notice, setNotice] = useState<string | null>(null);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    let external = false;
    const source = launch.source;
    const torrentHash = source.kind === "torrent" ? source.infoHash : null;
    // Whether this session must remove the torrent when it ends; false once the torrent
    // is handed to external mpv (whose Stop button removes it).
    let ownsTorrent = torrentHash !== null;
    const dropTorrent = () => {
      if (torrentHash) void removeTorrent(torrentHash);
    };

    (async () => {
      const request: PlaybackRequest =
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

      const embedded = await playerEmbeddedAvailable().catch(() => false);
      if (cancelled) return;
      external = !embedded;
      setBackend(embedded ? "embedded" : "external");

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
          // the page left while the magnet was resolving: the cleanup's remove found
          // nothing, so remove the torrent the engine has only just added
          if (ownsTorrent) dropTorrent();
          return;
        }
        request.url = resolved.streamUrl;
        request.title ??= resolved.fileName ?? null;
      }
      if (!request.url) throw new Error("nothing to play: the route carried no url/infoHash");

      if (!embedded) {
        ownsTorrent = false;
        try {
          await playerExternalPlay(request);
        } catch (e) {
          ownsTorrent = torrentHash !== null;
          if (cancelled && ownsTorrent) dropTorrent(); // the cleanup already ran
          throw e;
        }
        if (!cancelled) setPhase({ kind: "external" });
        return;
      }

      unlisten = await onPlayerEvent((event) => {
        if (!applyClockEvent(clock, event)) dispatch(event);
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
      if (cancelled) return;
      setPhase({
        kind: "error",
        message: errorMessage(e),
        // mpv missing or failing to launch: the URL still plays in any other player
        streamUrl:
          external && errorKind(e) === "player" && source.kind === "url" ? source.url : undefined,
      });
    });

    return () => {
      cancelled = true;
      unlisten?.();
      const stopped = playerEmbeddedStop().catch(() => undefined);
      if (ownsTorrent) void stopped.then(dropTorrent);
    };
    // launch and clock are fixed for the lifetime of the page
  }, [launch, clock, sendGeometry]);

  return { phase, backend, mpv, notice, dismissNotice: () => setNotice(null) };
}
