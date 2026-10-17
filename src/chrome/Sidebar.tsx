import {
  Blocks,
  Compass,
  Film,
  Home,
  Library,
  Radio,
  Settings,
  Sparkles,
  Tv,
} from "lucide-react";
import zeilerLogo from "../assets/zeiler-logo-white.png";
import { useSettings } from "../lib/store";
import { type RootView, useView } from "../lib/view";
import { ProfileChip } from "../views/profiles/ProfileChip";

interface NavEntry {
  view: RootView;
  label: string;
  icon: typeof Home;
}

export function Sidebar() {
  const { view, setView } = useView();
  const { settings } = useSettings();

  const primary: (NavEntry | null)[] = [
    { view: "home", label: "Home", icon: Home },
    { view: "discover", label: "Discover", icon: Compass },
    { view: "movies", label: "Movies", icon: Film },
    { view: "shows", label: "Shows", icon: Tv },
    settings?.hideAnime ? null : { view: "anime", label: "Anime", icon: Sparkles },
    settings?.hideLiveTv ? null : { view: "live", label: "Live TV", icon: Radio },
  ];
  const collections: NavEntry[] = [
    { view: "library", label: "My Library", icon: Library },
    { view: "addons", label: "Addons", icon: Blocks },
    { view: "settings", label: "Settings", icon: Settings },
  ];

  return (
    <aside className="relative z-[60] flex w-[72px] shrink-0 flex-col border-r border-edge-soft bg-canvas lg:w-60">
      <div
        data-tauri-drag-region
        className="flex h-20 shrink-0 items-center justify-center gap-2 px-3 text-ink lg:justify-start lg:px-6"
      >
        <img
          src={zeilerLogo}
          alt=""
          draggable={false}
          className="pointer-events-none h-10 w-10 shrink-0 select-none lg:h-11 lg:w-11"
        />
        <span className="pointer-events-none hidden font-display text-[30px] font-medium tracking-tight lg:inline">
          Zeiler
        </span>
      </div>

      <div className="no-scrollbar flex-1 overflow-y-auto px-4 pb-6 pt-3">
        <nav className="flex flex-col gap-1.5">
          {primary.filter(Boolean).map((entry) => (
            <NavItem key={entry!.label} entry={entry!} active={view === entry!.view} onSelect={setView} />
          ))}
        </nav>
        <div data-tauri-drag-region className="py-2.5">
          <div className="mx-3 h-px bg-gradient-to-r from-transparent via-edge-soft/55 to-transparent" />
        </div>
        <nav className="flex flex-col gap-1.5">
          {collections.map((entry) => (
            <NavItem key={entry.label} entry={entry} active={view === entry.view} onSelect={setView} />
          ))}
        </nav>
        <div data-tauri-drag-region className="min-h-8" />
      </div>

      <ProfileChip />
      <div className="hidden px-6 pb-5 text-[11px] leading-relaxed text-ink-subtle/70 lg:block">
        Zeiler hosts no media, indexes no media, and ships no content addon.
      </div>
    </aside>
  );
}

function NavItem({
  entry,
  active,
  onSelect,
}: {
  entry: NavEntry;
  active: boolean;
  onSelect: (view: RootView) => void;
}) {
  const Icon = entry.icon;
  return (
    <button
      title={entry.label}
      onClick={() => onSelect(entry.view)}
      className={`relative flex h-14 items-center justify-center gap-4 rounded-xl text-[16px] transition-colors duration-150 lg:justify-start lg:px-4 ${
        active ? "bg-elevated text-ink" : "text-ink-muted hover:bg-elevated/50 hover:text-ink"
      }`}
    >
      <Icon size={20} strokeWidth={1.7} />
      <span className="hidden flex-1 text-left lg:inline">{entry.label}</span>
    </button>
  );
}
