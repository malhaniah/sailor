// Harbor-style navigation: no router library, a frame stack in context.
// Root views reset the stack (settings pushes); detail/browse/picker frames push on top.
import {
  type ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { playerActive } from "./playback-handoff";
import type { MetaItem, MetaPreview, Video } from "./types";

// Names reserved for later milestones (docs/ARCHITECTURE.md, "Navigation names") are
// already in these unions so every switch handles them; nothing navigates to them yet.

export type RootView =
  | "home"
  | "discover"
  | "movies"
  | "shows"
  | "anime"
  | "library"
  | "addons"
  | "settings"
  // reserved: Live TV (addon tv/channel catalogs, then user IPTV)
  | "live";

export const ROOT_VIEWS: RootView[] = [
  "home",
  "discover",
  "movies",
  "shows",
  "anime",
  "library",
  "addons",
  "settings",
  "live",
];

export type Frame =
  | { kind: RootView }
  // the catalog item that was clicked: the detail page falls back to it when no
  // installed addon serves meta for this id
  | { kind: "meta"; type: string; id: string; preview: MetaPreview }
  | { kind: "picker"; meta: MetaItem; video: Video | null }
  // the catalog browser: a type, optionally one addon's catalog and its filters (kept in
  // the frame, so Back restores them); the transport URL never leaves memory
  | BrowseFrame
  // reserved: marketplace addon detail, keyed into the directory index
  | { kind: "addon-detail"; key: string };

export interface BrowseFrame {
  kind: "browse";
  type: string;
  transportUrl?: string;
  catalogId?: string;
  extra: { name: string; value: string }[];
}

/** Exhaustiveness guard: `default: return unreachable(x)` fails to compile when a
 * RootView or Frame kind is added without a case. */
export function unreachable(value: never): never {
  throw new Error(`Unhandled view: ${JSON.stringify(value)}`);
}

/** Overlay frames, which Escape pops (root views never pop; settings has its own close). */
function popsOnEscape(frame: Frame): boolean {
  switch (frame.kind) {
    case "meta":
    case "picker":
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
    case "settings":
    case "live":
      return false;
    default:
      return unreachable(frame);
  }
}

const STACK_MAX = 30;

interface ViewCtx {
  stack: Frame[];
  top: Frame;
  view: RootView;
  canGoBack: boolean;
  setView: (view: RootView) => void;
  openMeta: (preview: MetaPreview) => void;
  openBrowse: (target: Omit<BrowseFrame, "kind">) => void;
  /** Change the Browse frame on top in place (its filters), so Back skips the changes. */
  updateBrowse: (target: Omit<BrowseFrame, "kind">) => void;
  /** Browse `type` filtered to one genre (its first catalog that has the genre). */
  openFilter: (type: string, genre: string) => void;
  openPicker: (meta: MetaItem, video: Video | null) => void;
  goBack: () => void;
}

const ViewContext = createContext<ViewCtx | null>(null);

export function ViewProvider({ children }: { children: ReactNode }) {
  const [stack, setStack] = useState<Frame[]>([{ kind: "home" }]);

  const push = useCallback((frame: Frame) => {
    setStack((prev) => {
      const top = prev[prev.length - 1];
      if (JSON.stringify(top) === JSON.stringify(frame)) return prev;
      const next = [...prev, frame];
      return next.length > STACK_MAX ? [next[0], ...next.slice(2)] : next;
    });
  }, []);

  const setView = useCallback((view: RootView) => {
    setStack((prev) => {
      if (view === "settings") {
        const top = prev[prev.length - 1];
        return top.kind === "settings" ? prev : [...prev, { kind: "settings" }];
      }
      return [{ kind: view }];
    });
  }, []);

  const updateBrowse = useCallback((target: Omit<BrowseFrame, "kind">) => {
    setStack((prev) =>
      prev[prev.length - 1].kind === "browse"
        ? [...prev.slice(0, -1), { kind: "browse", ...target }]
        : prev,
    );
  }, []);

  const goBack = useCallback(() => {
    setStack((prev) => (prev.length > 1 ? prev.slice(0, -1) : prev));
  }, []);

  const value = useMemo<ViewCtx>(() => {
    const top = stack[stack.length - 1];
    const rootish = [...stack].reverse().find((f) => ROOT_VIEWS.includes(f.kind as RootView));
    return {
      stack,
      top,
      view: (rootish?.kind ?? "home") as RootView,
      canGoBack: stack.length > 1,
      setView,
      goBack,
      openMeta: (preview) =>
        push({ kind: "meta", type: preview.type, id: preview.id, preview }),
      openBrowse: (target) => push({ kind: "browse", ...target }),
      updateBrowse,
      openFilter: (type, genre) =>
        push({ kind: "browse", type, extra: [{ name: "genre", value: genre }] }),
      openPicker: (meta, video) => push({ kind: "picker", meta, video }),
    };
  }, [stack, push, setView, goBack, updateBrowse]);

  // Escape pops overlay frames (never a root view). Not while the Player screen is up:
  // the shell stays mounted underneath it, and the player's own Escape goes back.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || playerActive()) return;
      const target = e.target as HTMLElement | null;
      if (target && /^(input|textarea|select)$/i.test(target.tagName)) return;
      if (popsOnEscape(value.top)) value.goBack();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [value]);

  return <ViewContext.Provider value={value}>{children}</ViewContext.Provider>;
}

export function useView(): ViewCtx {
  const ctx = useContext(ViewContext);
  if (!ctx) throw new Error("useView outside ViewProvider");
  return ctx;
}
