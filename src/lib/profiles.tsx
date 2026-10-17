// Local profiles: who is watching. The shell below this provider is keyed by the
// active profile, so switching remounts it and every store reads the new profile's
// addons, settings and library from scratch.
import { listen } from "@tauri-apps/api/event";
import {
  type ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import { PROFILES_CHANGED, profileList } from "./ipc";
import type { ProfilesState } from "./types";

interface ProfilesCtx {
  state: ProfilesState | null;
  reload: () => Promise<void>;
  /** the picker is showing ("Who's watching?") */
  picking: boolean;
  setPicking: (open: boolean) => void;
}

const ProfilesContext = createContext<ProfilesCtx>({
  state: null,
  reload: async () => {},
  picking: false,
  setPicking: () => {},
});

export const useProfiles = () => useContext(ProfilesContext);

/** `children` gets the active profile's id, and whether it may be used (its PIN was
 * entered, or it has none). While it may not, nothing of it should be mounted. */
export function ProfilesProvider({
  children,
}: {
  children: (activeId: string | null, unlocked: boolean) => ReactNode;
}) {
  const [state, setState] = useState<ProfilesState | null>(null);
  const [picking, setPicking] = useState(false);
  // the shell waits for the first answer, so it mounts once, for the right profile
  const [ready, setReady] = useState(false);

  const reload = useCallback(async () => {
    try {
      setState(await profileList());
    } catch {
      /* the shell still works with the profile Rust loaded */
    }
  }, []);

  useEffect(() => {
    profileList()
      .then((s) => {
        setState(s);
        // at launch: more than one profile, or the active one has a PIN
        if (s.profiles.length > 1 || !s.unlocked) setPicking(true);
      })
      .catch(() => {})
      .finally(() => setReady(true));
    const unlisten = listen<ProfilesState>(PROFILES_CHANGED, (e) => setState(e.payload));
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  return (
    <ProfilesContext.Provider value={{ state, reload, picking, setPicking }}>
      {ready && children(state?.activeId ?? null, state?.unlocked ?? true)}
    </ProfilesContext.Provider>
  );
}
