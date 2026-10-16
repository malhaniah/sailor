/**
 * The Player screen (`#/player?h=<id>`): the single place playback starts, on every OS.
 *
 * It takes its `PlayerLaunch` from the one-shot handoff (src/lib/playback-handoff.ts) and
 * plays it on the best backend this build has:
 * - embedded libmpv (Windows `embedded` builds): the video is NOT an element on this
 *   page — mpv renders into a native child window pinned underneath the transparent
 *   webview. The page makes the document background transparent while mounted, keeps
 *   the native video rect in sync with the layout (`player_set_geometry`, physical px)
 *   and draws the controls as overlays;
 * - otherwise an external mpv window (`player_play`), shown as "Playing in mpv".
 *
 * The app shell stays mounted (hidden, `inert`) underneath, so Back (history.back)
 * returns to the stream picker exactly as the user left it — see src/main.tsx.
 *
 * Files: launch.ts (which launch to play), usePlaybackSession.ts (backend, resolve,
 * events), playback-clock.ts (position/duration/pause/buffering store), mpv-state.ts
 * (slower mpv state + phase), PlayerChrome.tsx / ControlsBar.tsx (embedded UI),
 * states.tsx (status screens), TorrentStats.tsx.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { leavePlayer, type PlayerLaunch, reportLaunchFailed } from "../../lib/playback-handoff";
import { playerEmbeddedStop, playerExternalStop, playerSetGeometry } from "../../lib/player-ipc";
import { launchForRoute, launchIdForRoute, releaseRoute } from "./launch";
import { createPlaybackClock } from "./playback-clock";
import { PlayerChrome } from "./PlayerChrome";
import { ExternalPlayback, NothingToPlay } from "./states";
import { removeTorrent, usePlaybackSession } from "./usePlaybackSession";

/** `hash` is the player route this page was mounted for (the page is keyed by it). */
export function PlayerPage({ hash }: { hash: string }) {
  const [launch] = useState(() => launchForRoute(hash));
  useEffect(() => () => releaseRoute(hash), [hash]);
  useEffect(() => {
    // A reload drops the handoff but not the Rust-side player: stop an embedded session
    // left playing (hidden behind this screen) by the page before the reload. A no-op
    // on non-embedded builds; external mpv is left alone.
    if (!launch) void playerEmbeddedStop().catch(() => undefined);
  }, [launch]);
  if (!launch) return <NothingToPlay onBack={leavePlayer} />;
  return <PlayerSession launch={launch} launchId={launchIdForRoute(hash)} />;
}

function PlayerSession({ launch, launchId }: { launch: PlayerLaunch; launchId: string | null }) {
  const [clock] = useState(createPlaybackClock);
  const videoRef = useRef<HTMLDivElement>(null);
  const infoHash = launch.source.kind === "torrent" ? launch.source.infoHash : null;

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

  const { phase, backend, mpv, notice, dismissNotice } = usePlaybackSession(
    launch,
    clock,
    sendGeometry,
  );

  // Tell the screen that opened this launch (the stream picker) that it failed, once.
  const reported = useRef(false);
  useEffect(() => {
    if (phase.kind !== "error" || reported.current || !launchId) return;
    reported.current = true;
    reportLaunchFailed(launchId);
  }, [phase.kind, launchId]);

  // The session's cleanup removes the torrent (also when its resolve is still running).
  const leave = useCallback(() => {
    void playerEmbeddedStop().catch(() => undefined);
    leavePlayer();
  }, []);

  // external mpv: Back keeps it playing (and its torrent streaming); Stop ends both
  const stopExternal = useCallback(async () => {
    await playerExternalStop().catch(() => undefined);
    if (infoHash) await removeTorrent(infoHash);
    leavePlayer();
  }, [infoHash]);

  if (phase.kind === "external") {
    return (
      <ExternalPlayback
        launch={launch}
        infoHash={infoHash}
        onBack={leavePlayer}
        onStop={stopExternal}
      />
    );
  }

  const filename = launch.source.kind === "torrent" ? launch.source.filename : null;
  return (
    <PlayerChrome
      phase={phase}
      backend={backend}
      mpv={mpv}
      clock={clock}
      notice={notice}
      dismissNotice={dismissNotice}
      title={mpv.mediaTitle ?? launch.title ?? filename ?? "Now playing"}
      infoHash={infoHash}
      videoRef={videoRef}
      onLeave={leave}
    />
  );
}
