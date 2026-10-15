import { SearchOverlay } from "./chrome/SearchOverlay";
import { Sidebar } from "./chrome/Sidebar";
import { Topbar } from "./chrome/Topbar";
import { UpdateBanner } from "./chrome/UpdateBanner";
import { StoreProvider } from "./lib/store";
import { type Frame, type RootView, ROOT_VIEWS, ViewProvider, useView } from "./lib/view";
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
      <ViewProvider>
        <Shell />
      </ViewProvider>
    </StoreProvider>
  );
}

function Shell() {
  const { stack, top } = useView();
  // Harbor rule: side chrome hides when the picker or settings sit on top.
  const hideSidebar = top.kind === "picker" || top.kind === "settings";
  const rootFrame = stack.find((f) => ROOT_VIEWS.includes(f.kind as RootView));
  const root = (rootFrame?.kind ?? "home") as RootView;
  const overlays = stack.filter(
    (f) => f.kind === "meta" || f.kind === "filter" || f.kind === "picker" || f.kind === "settings",
  );

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
  }
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
    default:
      return null;
  }
}
