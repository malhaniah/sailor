import { listen } from "@tauri-apps/api/event";
import {
  type ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { SETTINGS_CHANGED, addonList, settingsGet, settingsPatch } from "./ipc";
import { applyTheme } from "./theme";
import type { Descriptor, Settings } from "./types";

// ---- addons ---------------------------------------------------------------------------

interface AddonsCtx {
  addons: Descriptor[];
  loaded: boolean;
  reload: () => Promise<void>;
}

const AddonsContext = createContext<AddonsCtx>({
  addons: [],
  loaded: false,
  reload: async () => {},
});

export const useAddons = () => useContext(AddonsContext);

// ---- settings -------------------------------------------------------------------------

interface SettingsCtx {
  settings: Settings | null;
  /**
   * Optimistically applies `patch`, then sends only its keys to Rust (`settings_patch`,
   * merged server-side so concurrent writers never undo each other). On failure the
   * patched keys revert. Every change, from anywhere, arrives via `settings://changed`.
   */
  save: (patch: Partial<Settings>) => Promise<void>;
}

const SettingsContext = createContext<SettingsCtx>({
  settings: null,
  save: async () => {},
});

export const useSettings = () => useContext(SettingsContext);

// ---- search overlay -------------------------------------------------------------------

interface SearchCtx {
  open: boolean;
  setOpen: (open: boolean) => void;
}

const SearchContext = createContext<SearchCtx>({ open: false, setOpen: () => {} });
export const useSearch = () => useContext(SearchContext);

// ---- combined provider ----------------------------------------------------------------

export function StoreProvider({ children }: { children: ReactNode }) {
  const [addons, setAddons] = useState<Descriptor[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [settings, setSettingsState] = useState<Settings | null>(null);
  // latest settings for save(), which must not close over a stale render
  const settingsRef = useRef<Settings | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);

  const setSettings = useCallback((next: Settings) => {
    settingsRef.current = next;
    setSettingsState(next);
    applyTheme(next.theme);
  }, []);

  const reload = useCallback(async () => {
    try {
      setAddons(await addonList());
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void reload();
    settingsGet()
      .then(setSettings)
      .catch(() => {});
    const unlisten = listen<Settings>(SETTINGS_CHANGED, (e) => setSettings(e.payload));
    return () => {
      void unlisten.then((off) => off());
    };
  }, [reload, setSettings]);

  const save = useCallback(
    async (patch: Partial<Settings>) => {
      const current = settingsRef.current;
      if (!current) return;
      const keys = (Object.keys(patch) as (keyof Settings)[]).filter(
        (k) => patch[k] !== undefined,
      );
      if (keys.length === 0) return;
      const changed: Partial<Settings> = {};
      const previous: Partial<Settings> = {};
      for (const k of keys) {
        Object.assign(changed, { [k]: patch[k] });
        Object.assign(previous, { [k]: current[k] });
      }
      setSettings({ ...current, ...changed }); // optimistic
      try {
        setSettings(await settingsPatch(changed));
      } catch {
        const latest = settingsRef.current ?? current;
        setSettings({ ...latest, ...previous });
      }
    },
    [setSettings],
  );

  return (
    <AddonsContext.Provider value={{ addons, loaded, reload }}>
      <SettingsContext.Provider value={{ settings, save }}>
        <SearchContext.Provider value={{ open: searchOpen, setOpen: setSearchOpen }}>
          {children}
        </SearchContext.Provider>
      </SettingsContext.Provider>
    </AddonsContext.Provider>
  );
}
