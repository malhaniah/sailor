import {
  type ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import { addonList, settingsGet, settingsSet } from "./ipc";
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
  const [settings, setSettings] = useState<Settings | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);

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
      .then((s) => {
        setSettings(s);
        applyTheme(s.theme);
      })
      .catch(() => setSettings(null));
  }, [reload]);

  const save = useCallback(
    async (patch: Partial<Settings>) => {
      if (!settings) return;
      const next = { ...settings, ...patch };
      setSettings(next); // optimistic
      applyTheme(next.theme);
      try {
        setSettings(await settingsSet(next));
      } catch {
        setSettings(settings);
        applyTheme(settings.theme);
      }
    },
    [settings],
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
