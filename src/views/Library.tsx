import { EmptyState } from "../components/ui";

export function LibraryView() {
  return (
    <main className="flex-1 overflow-y-auto px-12 pt-32">
      <header className="mb-10 flex flex-col gap-2">
        <span className="text-[11px] font-bold uppercase tracking-[0.42em] text-ink-subtle">
          Collections
        </span>
        <h1 className="font-display text-[44px] font-medium leading-[1.05] tracking-tight text-ink">
          My Library
        </h1>
      </header>
      <EmptyState title="Nothing here yet">
        Watchlist and continue-watching land together with the embedded player milestone —
        Zeiler will remember what you play and where you stopped.
      </EmptyState>
    </main>
  );
}
