// Harbor-style navigation: no router library, a frame stack in context.
// Root views reset the stack (settings pushes); detail/filter/picker frames push on top.
import {
  type ReactNode,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type { MetaItem, Video } from "./types";

export type RootView =
  | "home"
  | "discover"
  | "movies"
  | "shows"
  | "anime"
  | "library"
  | "addons"
  | "settings";

export const ROOT_VIEWS: RootView[] = [
  "home",
  "discover",
  "movies",
  "shows",
  "anime",
  "library",
  "addons",
  "settings",
];

export type Frame =
  | { kind: RootView }
  | { kind: "meta"; type: string; id: string }
  | { kind: "filter"; genre: string; type: string }
  | { kind: "picker"; meta: MetaItem; video: Video | null };

const STACK_MAX = 30;

interface ViewCtx {
  stack: Frame[];
  top: Frame;
  view: RootView;
  canGoBack: boolean;
  setView: (view: RootView) => void;
  openMeta: (type: string, id: string) => void;
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
      openMeta: (type, id) => push({ kind: "meta", type, id }),
      openFilter: (type, genre) => push({ kind: "filter", type, genre }),
      openPicker: (meta, video) => push({ kind: "picker", meta, video }),
    };
  }, [stack, push, setView, goBack]);

  // Escape pops overlay frames (never a root view).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const target = e.target as HTMLElement | null;
      if (target && /^(input|textarea|select)$/i.test(target.tagName)) return;
      const top = value.top;
      if (top.kind === "meta" || top.kind === "filter" || top.kind === "picker") {
        value.goBack();
      }
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
