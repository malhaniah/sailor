import { createContext, useCallback, useContext, useEffect, useState } from "react";
import { addonList } from "./lib/ipc";
import type { Descriptor } from "./lib/types";
import { AddonsPage } from "./pages/Addons";
import { DetailPage } from "./pages/Detail";
import { DiscoverPage } from "./pages/Discover";
import { SearchPage } from "./pages/Search";
import { SettingsPage } from "./pages/Settings";

// --- tiny hash router -----------------------------------------------------------------

export function useHashRoute(): string {
  const [hash, setHash] = useState(window.location.hash);
  useEffect(() => {
    const onChange = () => setHash(window.location.hash);
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  return hash.replace(/^#/, "") || "/discover";
}

export function navigate(path: string) {
  window.location.hash = path;
}

// --- addons context -------------------------------------------------------------------

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

// --- shell ------------------------------------------------------------------------------

const NAV = [
  { path: "/discover", label: "Discover", icon: "🧭" },
  { path: "/search", label: "Search", icon: "🔎" },
  { path: "/addons", label: "Addons", icon: "🧩" },
  { path: "/settings", label: "Settings", icon: "⚙️" },
];

export default function App() {
  const route = useHashRoute();
  const [addons, setAddons] = useState<Descriptor[]>([]);
  const [loaded, setLoaded] = useState(false);

  const reload = useCallback(async () => {
    try {
      setAddons(await addonList());
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  let page: React.ReactNode;
  if (route.startsWith("/detail/")) {
    const [, , type, id] = route.split("/");
    page = <DetailPage type={decodeURIComponent(type)} id={decodeURIComponent(id)} />;
  } else if (route.startsWith("/search")) {
    page = <SearchPage />;
  } else if (route.startsWith("/addons")) {
    page = <AddonsPage />;
  } else if (route.startsWith("/settings")) {
    page = <SettingsPage />;
  } else {
    page = <DiscoverPage />;
  }

  return (
    <AddonsContext.Provider value={{ addons, loaded, reload }}>
      <div className="flex h-full">
        <nav className="flex w-52 shrink-0 flex-col border-r border-rig/60 bg-deep">
          <div className="flex items-center gap-2 px-5 py-5">
            <span className="text-2xl">⛵</span>
            <span className="text-lg font-bold tracking-wide">Sailor</span>
          </div>
          <div className="flex flex-col gap-1 px-3">
            {NAV.map((item) => {
              const active =
                route.startsWith(item.path) ||
                (item.path === "/discover" && route.startsWith("/detail"));
              return (
                <button
                  key={item.path}
                  onClick={() => navigate(item.path)}
                  className={`flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition ${
                    active ? "bg-hull text-foam" : "text-mist hover:bg-hull/60 hover:text-foam"
                  }`}
                >
                  <span>{item.icon}</span>
                  {item.label}
                </button>
              );
            })}
          </div>
          <div className="mt-auto px-5 py-4 text-[11px] leading-relaxed text-mist/70">
            Sailor hosts no media, indexes no media, and ships no content addon.
          </div>
        </nav>
        <main className="min-w-0 flex-1 overflow-y-auto">{page}</main>
      </div>
    </AddonsContext.Provider>
  );
}
