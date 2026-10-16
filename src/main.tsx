import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { PlayerPage } from "./pages/Player";
import "./index.css";

// The player route swaps out the whole shell: the page behind the transparent
// video region must paint nothing but the player's own chrome. It is mounted
// outside StrictMode so the mpv instance isn't started/stopped twice in dev.
function Root() {
  const [hash, setHash] = useState(window.location.hash);
  useEffect(() => {
    const onChange = () => setHash(window.location.hash);
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  return hash.startsWith("#/player") ? (
    <PlayerPage key={hash} />
  ) : (
    <React.StrictMode>
      <App />
    </React.StrictMode>
  );
}

// dev-only hook so `VITE_ZEILER_AUTOPLAY="#/player?url=..." npm run tauri dev`
// drops straight into the player for testing
if (import.meta.env.DEV && import.meta.env.VITE_ZEILER_AUTOPLAY && !window.location.hash) {
  window.location.hash = import.meta.env.VITE_ZEILER_AUTOPLAY as string;
}

ReactDOM.createRoot(document.getElementById("root")!).render(<Root />);
