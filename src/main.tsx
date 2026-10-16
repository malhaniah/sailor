import React, { useEffect, useLayoutEffect, useRef, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { isPlayerHash, playerActive, subscribeRoute } from "./lib/playback-handoff";
import { PlayerPage } from "./pages/player/PlayerPage";
import "./index.css";

// The Player screen (#/player) is drawn over the app shell, which stays mounted but
// hidden (display:none + inert) underneath. The view stack therefore survives playback
// and Back (history.back) lands on the stream picker exactly where the user left it.
// The player is mounted outside StrictMode so the mpv instance isn't started/stopped
// twice in dev; the shell keeps its StrictMode.
function Root() {
  const hash = useSyncExternalStore(subscribeRoute, () => window.location.hash);
  const active = isPlayerHash(hash);
  return (
    <>
      <KeptMounted hidden={active}>
        <React.StrictMode>
          <App />
        </React.StrictMode>
      </KeptMounted>
      {active && <PlayerPage key={hash} hash={hash} />}
    </>
  );
}

/**
 * Hides its children without unmounting them. `inert` keeps focus, clicks and assistive
 * tech out of the hidden tree (window-level key listeners still fire; they check
 * `playerActive()`). display:none resets scroll offsets, so they are recorded when the
 * player opens and put back when the tree shows again.
 */
function KeptMounted({ hidden, children }: { hidden: boolean; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const scrolled = useRef<[Element, number, number][]>([]);

  // Route subscribers all run in one hashchange callback, before React commits the
  // render that hides this tree, so the offsets are still live here.
  useEffect(
    () =>
      subscribeRoute(() => {
        const root = ref.current;
        if (!playerActive() || !root || root.style.display === "none") return;
        scrolled.current = [];
        for (const el of root.querySelectorAll("*")) {
          if (el.scrollTop !== 0 || el.scrollLeft !== 0) {
            scrolled.current.push([el, el.scrollTop, el.scrollLeft]);
          }
        }
      }),
    [],
  );

  useLayoutEffect(() => {
    if (hidden) return;
    for (const [el, top, left] of scrolled.current) {
      if (el.isConnected) {
        el.scrollTop = top;
        el.scrollLeft = left;
      }
    }
    scrolled.current = [];
  }, [hidden]);

  return (
    <div ref={ref} className="h-full" style={hidden ? { display: "none" } : undefined} inert={hidden}>
      {children}
    </div>
  );
}

// dev-only hook so `VITE_ZEILER_AUTOPLAY="#/player?url=..." npm run tauri dev`
// drops straight into the player for testing
if (import.meta.env.DEV && import.meta.env.VITE_ZEILER_AUTOPLAY && !window.location.hash) {
  window.location.hash = import.meta.env.VITE_ZEILER_AUTOPLAY as string;
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Root />);
