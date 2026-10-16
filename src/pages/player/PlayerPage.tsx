/**
 * The Player screen (`#/player?h=<id>`): the single place playback starts, on every OS.
 *
 * It takes its `PlayerLaunch` from the one-shot handoff (src/lib/playback-handoff.ts) and
 * starts it through the unified player API (`player_start`); Rust picks the backend —
 * following the launch's `backend` ("auto" = the `playerBackend` setting; the picker's
 * "Open in external mpv" forces "external") — and the page renders whichever it got:
 * - embedded libmpv (Windows `embedded` builds): the video is NOT an element on this
 *   page — mpv renders into a native child window pinned underneath the transparent
 *   webview. The page makes the document background transparent while mounted, keeps
 *   the native video rect in sync with the layout (`player_set_geometry`, physical px)
 *   and draws the controls as overlays (PlayerChrome.tsx);
 * - external mpv in its own window, remote-controlled over its JSON IPC: the title's
 *   art fills the page and the same controls are docked at the bottom
 *   (ExternalRemote.tsx). When built-in playback fell back to mpv, a dismissible notice
 *   says why. Back keeps mpv playing once it plays (while it is still opening, Back
 *   cancels; after it ended, Back closes it); Stop closes it. Only if mpv's IPC never
 *   comes up is the passive "Playing in mpv" card shown instead (UncontrolledPlayback),
 *   with the same fallback notice.
 *
 * The app shell stays mounted (hidden, `inert`) underneath, so Back (history.back)
 * returns to the stream picker exactly as the user left it — see src/main.tsx.
 *
 * Files: launch.ts (which launch to play), usePlaybackSession.ts (start, resolve,
 * events, cleanup), usePlayerControls.ts (actions + keyboard), playback-clock.ts
 * (position/duration/pause/buffering store), mpv-state.ts (slower mpv state + phase),
 * PlayerChrome.tsx (embedded), ExternalRemote.tsx (external), ControlsBar.tsx (both),
 * states.tsx (status cards, title labels), TorrentStats.tsx.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { leavePlayer, type PlayerLaunch, reportLaunchFailed } from "../../lib/playback-handoff";
import { playerSetGeometry, playerStop } from "../../lib/player-ipc";
import { ExternalRemote } from "./ExternalRemote";
import { launchForRoute, launchIdForRoute, releaseRoute } from "./launch";
import { createPlaybackClock } from "./playback-clock";
import { PlayerChrome } from "./PlayerChrome";
import { NothingToPlay, UncontrolledPlayback } from "./states";
import { usePlaybackSession } from "./usePlaybackSession";

/** `hash` is the player route this page was mounted for (the page is keyed by it). */
export function PlayerPage({ hash }: { hash: string }) {
  const [launch] = useState(() => launchForRoute(hash));
  useEffect(() => () => releaseRoute(hash), [hash]);
  useEffect(() => {
    // A reload drops the handoff but not the Rust-side player: stop a built-in video
    // left playing, which would sit hidden behind this screen with nothing to control
    // it. Only that one — an external mpv the user left playing with Back (it said so)
    // keeps playing, also when a stale history entry (Forward) lands here.
    if (launch) return;
    void playerStop("embedded").catch(() => undefined);
  }, [launch]);
  if (!launch) return <NothingToPlay onBack={leavePlayer} />;
  return <PlayerSession launch={launch} launchId={launchIdForRoute(hash)} />;
}

function PlayerSession({ launch, launchId }: { launch: PlayerLaunch; launchId: string | null }) {
  const [clock] = useState(createPlaybackClock);
  const videoRef = useRef<HTMLDivElement>(null);
  const infoHash = launch.source.kind === "torrent" ? launch.source.infoHash : null;

  // While this page is up, the document must not paint a background —
  // the native video window (embedded backend) sits directly behind the webview.
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

  const session = usePlaybackSession(launch, clock, sendGeometry);
  const { phase, backend, mpv } = session;

  // Keep the native video rect glued to the layout (embedded only; the region exists
  // only while PlayerChrome is drawn).
  useEffect(() => {
    if (backend === "external") return;
    sendGeometry();
    const el = videoRef.current;
    const observer = new ResizeObserver(sendGeometry);
    if (el) observer.observe(el);
    window.addEventListener("resize", sendGeometry);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", sendGeometry);
    };
  }, [sendGeometry, backend]);

  // Tell the screen that opened this launch (the stream picker) that it failed, once.
  const reported = useRef(false);
  useEffect(() => {
    if (phase.kind !== "error" || reported.current || !launchId) return;
    reported.current = true;
    reportLaunchFailed(launchId);
  }, [phase.kind, launchId]);

  // The session's cleanup stops the player (except external mpv that is playing, which
  // Back keeps — see backKeepsPlaying) and removes the torrent (also when its resolve
  // is still running).
  const leave = leavePlayer;

  if (phase.kind === "uncontrolled") {
    return (
      <UncontrolledPlayback
        launch={launch}
        infoHash={infoHash}
        fallbackReason={session.fallbackReason}
        dismissFallback={session.dismissFallback}
        onBack={leave}
        onStop={session.stopAndLeave}
      />
    );
  }

  if (backend === "external") {
    return (
      <ExternalRemote
        launch={launch}
        phase={phase}
        mpv={mpv}
        clock={clock}
        notice={session.notice}
        dismissNotice={session.dismissNotice}
        fallbackReason={session.fallbackReason}
        dismissFallback={session.dismissFallback}
        infoHash={infoHash}
        onBack={leave}
        onStop={session.stopAndLeave}
        onPlayAgain={session.playAgain}
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
      notice={session.notice}
      dismissNotice={session.dismissNotice}
      title={mpv.mediaTitle ?? launch.title ?? filename ?? "Now playing"}
      infoHash={infoHash}
      videoRef={videoRef}
      onLeave={leave}
      onPlayAgain={session.playAgain}
    />
  );
}
