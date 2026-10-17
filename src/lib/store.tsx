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
import { ADDONS_CHANGED, SETTINGS_CHANGED, addonList, settingsGet, settingsPatch } from "./ipc";
import { applyTheme } from "./theme";
import type { Descriptor, Settings, SettingsPatch } from "./types";

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
   * merged server-side so concurrent writers never undo each other; a group such as
   * `subtitles` is merged field by field). On failure the patched keys revert. Every
   * change, from anywhere, arrives via `settings://changed`.
   */
  save: (patch: SettingsPatch) => Promise<void>;
}

const SettingsContext = createContext<SettingsCtx>({
  settings: null,
  save: async () => {},
});

export const useSettings = () => useContext(SettingsContext);

/** `patch` merged into `base` the way `settings_patch` merges it: objects key by key,
 * arrays and scalars replaced. */
function mergePatch(base: unknown, patch: unknown): unknown {
  const isObject = (v: unknown): v is Record<string, unknown> =>
    typeof v === "object" && v !== null && !Array.isArray(v);
  if (!isObject(base) || !isObject(patch)) return patch;
  const out: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(patch)) {
    if (value !== undefined) out[key] = mergePatch(base[key], value);
  }
  return out;
}

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
    // an account sync replaced the profile's addons
    const unlistenAddons = listen(ADDONS_CHANGED, () => void reload());
    return () => {
      void unlisten.then((off) => off());
      void unlistenAddons.then((off) => off());
    };
  }, [reload, setSettings]);

  const save = useCallback(
    async (patch: SettingsPatch) => {
      const current = settingsRef.current;
      if (!current) return;
      const keys = (Object.keys(patch) as (keyof Settings)[]).filter(
        (k) => patch[k] !== undefined,
      );
      if (keys.length === 0) return;
      const changed: SettingsPatch = {};
      const previous: Partial<Settings> = {};
      for (const k of keys) {
        Object.assign(changed, { [k]: patch[k] });
        Object.assign(previous, { [k]: current[k] });
      }
      setSettings(mergePatch(current, changed) as Settings); // optimistic
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
