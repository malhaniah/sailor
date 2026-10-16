import { SearchOverlay } from "./chrome/SearchOverlay";
import { Sidebar } from "./chrome/Sidebar";
import { Topbar } from "./chrome/Topbar";
import { UpdateBanner } from "./chrome/UpdateBanner";
import { LibraryProvider } from "./lib/library";
import { StoreProvider } from "./lib/store";
import { EmptyState } from "./components/ui";
import {
  type Frame,
  type RootView,
  ROOT_VIEWS,
  ViewProvider,
  unreachable,
  useView,
} from "./lib/view";
import { AddonsView } from "./views/Addons";
import { DetailView } from "./views/Detail";
import { DiscoverView } from "./views/Discover";
import { FilterView } from "./views/Filter";
import { HomeView } from "./views/Home";
import { LibraryView } from "./views/Library";
import { PlayPickerView } from "./views/PlayPicker";
import { SettingsView } from "./views/Settings";
import { TypeRoomView } from "./views/TypeRoom";

export default function App() {
  return (
    <StoreProvider>
      <LibraryProvider>
        <ViewProvider>
          <Shell />
        </ViewProvider>
      </LibraryProvider>
    </StoreProvider>
  );
}

function Shell() {
  const { stack, top } = useView();
  // Harbor rule: side chrome hides when the picker or settings sit on top.
  const hideSidebar = top.kind === "picker" || top.kind === "settings";
  const rootFrame = stack.find((f) => ROOT_VIEWS.includes(f.kind as RootView));
  const root = (rootFrame?.kind ?? "home") as RootView;
  const overlays = stack.filter(isOverlay);

  return (
    <div className="relative flex h-full">
      {!hideSidebar && <Sidebar />}
      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
        {/* top fade so content scrolling under the fixed topbar stays legible */}
        <div
          aria-hidden
          className="pointer-events-none absolute inset-x-0 top-0 z-30 h-24 bg-gradient-to-b from-canvas/85 via-canvas/40 to-transparent"
        />
        <RootViewHost view={root} />
        {overlays.map((frame, i) => (
          <OverlayFrame key={`${frame.kind}-${i}`} frame={frame} />
        ))}
      </div>
      <Topbar />
      <SearchOverlay />
      <UpdateBanner />
    </div>
  );
}

function RootViewHost({ view }: { view: RootView }) {
  switch (view) {
    case "home":
      return <HomeView />;
    case "discover":
      return <DiscoverView />;
    case "movies":
      return <TypeRoomView room="movies" />;
    case "shows":
      return <TypeRoomView room="shows" />;
    case "anime":
      return <TypeRoomView room="anime" />;
    case "library":
      return <LibraryView />;
    case "addons":
      return <AddonsView />;
    case "settings":
      return <SettingsView />;
    case "live":
      return <ReservedView title="Live TV" />;
    default:
      return unreachable(view);
  }
}

/** Frames drawn over the root view (settings pushes on top as an overlay too). */
function isOverlay(frame: Frame): boolean {
  switch (frame.kind) {
    case "meta":
    case "filter":
    case "picker":
    case "settings":
    case "browse":
    case "addon-detail":
      return true;
    case "home":
    case "discover":
    case "movies":
    case "shows":
    case "anime":
    case "library":
    case "addons":
    case "live":
      return false;
    default:
      return unreachable(frame);
  }
}

/** Placeholder for a reserved navigation name; nothing navigates here yet. */
function ReservedView({ title }: { title: string }) {
  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center bg-canvas px-12">
      <EmptyState title={title}>Not in Zeiler yet.</EmptyState>
    </div>
  );
}

function OverlayFrame({ frame }: { frame: Frame }) {
  switch (frame.kind) {
    case "meta":
      return <DetailView type={frame.type} id={frame.id} preview={frame.preview} />;
    case "filter":
      return <FilterView type={frame.type} genre={frame.genre} />;
    case "picker":
      return <PlayPickerView meta={frame.meta} video={frame.video} />;
    case "settings":
      return (
        <div className="absolute inset-0 z-40">
          <SettingsView />
        </div>
      );
    case "browse":
      return <ReservedView title="Browse" />;
    case "addon-detail":
      return <ReservedView title="Addon" />;
    // root views render in RootViewHost
    case "home":
    case "discover":
    case "movies":
    case "shows":
    case "anime":
    case "library":
    case "addons":
    case "live":
      return null;
    default:
      return unreachable(frame);
  }
}
