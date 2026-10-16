import { ArrowLeft, Search } from "lucide-react";
import { useEffect, useState } from "react";
import { playerActive } from "../lib/playback-handoff";
import { useSearch } from "../lib/store";
import { useView } from "../lib/view";

const IN_TAURI = "__TAURI_INTERNALS__" in window;

export function Topbar() {
  const { canGoBack, goBack, top } = useView();
  const { setOpen } = useSearch();

  // "/" opens search anywhere (except while typing, or under the Player screen, where
  // the shell is hidden and the overlay would steal the player's keyboard focus)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "/" || e.ctrlKey || e.metaKey || e.altKey || playerActive()) return;
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (/^(input|textarea|select)$/i.test(target.tagName) || target.isContentEditable)
      ) {
        return;
      }
      e.preventDefault();
      setOpen(true);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setOpen]);

  if (top.kind === "picker") return null;

  return (
    <header className="fixed left-0 right-0 top-0 z-[55] h-20">
      <div
        data-tauri-drag-region
        className="relative z-10 grid h-full grid-cols-[1fr_auto_1fr] items-center gap-3 px-4 sm:px-8"
      >
        <div className="flex items-center pl-[72px] lg:pl-[244px]" data-tauri-drag-region>
          {canGoBack && (
            <button
              onClick={goBack}
              className="flex h-10 shrink-0 items-center gap-2 rounded-full border border-edge-soft/60 bg-canvas/85 pl-3 pr-5 text-[13.5px] font-medium text-ink-muted transition-colors hover:bg-canvas hover:text-ink"
            >
              <ArrowLeft size={15} />
              Back
            </button>
          )}
        </div>

        <button
          onClick={() => setOpen(true)}
          className="flex h-11 w-[14rem] items-center gap-3 rounded-full border border-edge-soft/60 bg-elevated/80 px-5 text-left opacity-80 transition-[width,opacity,background-color] duration-200 hover:bg-elevated hover:opacity-100 sm:w-[20rem] sm:hover:w-[24rem] lg:w-[24rem] lg:hover:w-[28rem] xl:w-[28rem] xl:hover:w-[34rem]"
        >
          <Search size={16} className="shrink-0 text-ink-subtle" />
          <span className="flex-1 truncate text-[14px] text-ink-subtle">
            Search movies, shows…
          </span>
          <kbd className="hidden shrink-0 rounded-md border border-edge-soft bg-canvas/50 px-1.5 py-0.5 font-mono text-[10.5px] font-medium text-ink-subtle sm:inline">
            /
          </kbd>
        </button>

        <div className="flex items-center justify-end gap-1.5">
          {IN_TAURI && <WindowControls />}
        </div>
      </div>
    </header>
  );
}

function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      const refresh = async () => {
        const value = await win.isMaximized().catch(() => false);
        if (!disposed) setMaximized(value);
      };
      void refresh();
      unlisten = await win.onResized(() => void refresh());
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const act = async (action: "minimize" | "toggleMaximize" | "close") => {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow()[action]();
  };

  const buttonClass =
    "flex h-10 w-11 items-center justify-center rounded-xl bg-elevated/70 text-ink-muted transition-colors duration-150 hover:bg-elevated hover:text-ink";

  return (
    <div className="ml-1 flex items-center gap-1.5">
      <button className={buttonClass} onClick={() => void act("minimize")} aria-label="Minimize">
        <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.4">
          <path d="M3 6.5h7" />
        </svg>
      </button>
      <button
        className={buttonClass}
        onClick={() => void act("toggleMaximize")}
        aria-label={maximized ? "Restore" : "Maximize"}
      >
        <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.4">
          {maximized ? (
            <>
              <rect x="3" y="4.5" width="5.5" height="5.5" rx="1.2" />
              <path d="M5 3h4.5A1.5 1.5 0 0 1 11 4.5V9" />
            </>
          ) : (
            <rect x="3" y="3" width="7" height="7" rx="1.2" />
          )}
        </svg>
      </button>
      <button className={buttonClass} onClick={() => void act("close")} aria-label="Close">
        <svg width="13" height="13" viewBox="0 0 13 13" fill="none" stroke="currentColor" strokeWidth="1.4">
          <path d="M3.5 3.5l6 6M9.5 3.5l-6 6" />
        </svg>
      </button>
    </div>
  );
}
