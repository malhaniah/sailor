import { createContext, useContext } from "react";

/**
 * Whether the launch being played is live TV (`PlayerLaunch.live`): no duration to
 * seek in, no speed, never tracked. Provided once by PlayerPage, so the controls can
 * read it without every screen passing it down.
 */
export const LiveContext = createContext(false);

export const useLive = () => useContext(LiveContext);
