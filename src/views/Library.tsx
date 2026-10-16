import { useMemo, useState } from "react";
import { LibraryCard } from "../components/LibraryCard";
import { ChipPicker, EmptyState, ErrorNote, Spinner } from "../components/ui";
import { errorMessage } from "../lib/ipc";
import {
  continueLine,
  episodeLabel,
  formatDay,
  timeLeft,
  useLibrary,
  useLibraryList,
  useOpenLibraryEntry,
} from "../lib/library";
import {
  type LibraryEntry,
  type LibraryFilter,
  type LibrarySort,
  libraryClearProgress,
} from "../lib/library-ipc";
import { unreachable } from "../lib/view";

const TABS: { id: LibraryFilter; label: string }[] = [
  { id: "continueWatching", label: "Continue Watching" },
  { id: "library", label: "Library" },
  { id: "history", label: "History" },
];

/** stremio-core's library sorts, with Stremio's own labels. */
const SORTS: { id: LibrarySort; label: string }[] = [
  { id: "lastWatched", label: "Last watched" },
  { id: "name", label: "A–Z" },
  { id: "nameReverse", label: "Z–A" },
  { id: "timesWatched", label: "Most watched" },
  { id: "watched", label: "Watched" },
  { id: "notWatched", label: "Not watched" },
];

// a Map: types come from addons, and a type such as "constructor" must not find an
// Object.prototype member
const TYPE_LABEL = new Map([
  ["movie", "Movies"],
  ["series", "Series"],
  ["anime", "Anime"],
  ["tv", "TV"],
  ["channel", "Channels"],
]);

const typeLabel = (type: string) =>
  TYPE_LABEL.get(type) ?? `${type.slice(0, 1).toUpperCase()}${type.slice(1)}`;

const EMPTY: Record<LibraryFilter, { title: string; body: string }> = {
  continueWatching: {
    title: "Nothing to continue",
    body: "Start playing a movie or an episode and it shows up here, ready to pick up where you stopped.",
  },
  library: {
    title: "Your library is empty",
    body: "Add titles from their page with Add to Library and they wait for you here.",
  },
  history: {
    title: "No history yet",
    body: "Everything you play is listed here, most recent first.",
  },
};

export function LibraryView() {
  const { continueWatching } = useLibrary();
  // open on Continue Watching when there is something to continue
  const [tab, setTab] = useState<LibraryFilter>(() =>
    continueWatching.length > 0 ? "continueWatching" : "library",
  );
  const [sort, setSort] = useState<LibrarySort>("lastWatched");
  const [type, setType] = useState<string | null>(null);
  const { entries, error } = useLibraryList({ filter: tab, sort: tab === "library" ? sort : undefined });
  const { open, pending } = useOpenLibraryEntry();
  const [actionError, setActionError] = useState<string | null>(null);

  // the type chips come from what this tab holds; the choice is kept across tabs
  const types = useMemo(
    () => [...new Set((entries ?? []).map((e) => e.type))].sort((a, b) => typeLabel(a).localeCompare(typeLabel(b))),
    [entries],
  );
  const activeType = type !== null && types.includes(type) ? type : null;
  const shown = (entries ?? []).filter((e) => activeType === null || e.type === activeType);

  const dismiss = (entry: LibraryEntry) => {
    setActionError(null);
    libraryClearProgress(entry.id).catch((e) => setActionError(errorMessage(e)));
  };

  return (
    <main className="flex-1 overflow-y-auto px-12 pb-20 pt-28">
      <header className="mb-8 flex flex-col gap-2">
        <span className="text-[11px] font-bold uppercase tracking-[0.42em] text-ink-subtle">
          Collections
        </span>
        <h1 className="font-display text-[44px] font-medium leading-[1.05] tracking-tight text-ink">
          My Library
        </h1>
        <p className="max-w-2xl text-[15px] text-ink-muted">
          What you are watching, what you saved for later, and everything you played. Kept
          on this computer.
        </p>
      </header>

      <div role="group" aria-label="Library views" className="mb-6 flex gap-1 border-b border-edge-soft">
        {TABS.map(({ id, label }) => (
          <button
            key={id}
            aria-pressed={tab === id}
            onClick={() => {
              setTab(id);
              setActionError(null);
            }}
            className={`-mb-px border-b-2 px-4 pb-3 pt-1 text-[14.5px] font-medium transition-colors ${
              tab === id
                ? "border-accent text-ink"
                : "border-transparent text-ink-muted hover:text-ink"
            }`}
          >
            {label}
            {id === "continueWatching" && continueWatching.length > 0 && (
              <span className="ml-2 text-[12px] font-normal text-ink-subtle">
                {continueWatching.length}
              </span>
            )}
          </button>
        ))}
      </div>

      {(types.length > 1 || tab === "library") && (
        <div className="mb-8 flex flex-wrap items-center justify-between gap-x-8 gap-y-4">
          {types.length > 1 ? (
            <ChipPicker<string>
              options={["", ...types]}
              value={activeType ?? ""}
              onChange={(t) => setType(t === "" ? null : t)}
              format={(t) => (t === "" ? "All" : typeLabel(t))}
            />
          ) : (
            <span />
          )}
          {tab === "library" && (
            <div className="flex items-center gap-3">
              <span className="text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
                Sort
              </span>
              <ChipPicker<LibrarySort>
                options={SORTS.map((s) => s.id)}
                value={sort}
                onChange={setSort}
                format={(id) => SORTS.find((s) => s.id === id)?.label ?? id}
              />
            </div>
          )}
        </div>
      )}

      {actionError && (
        <div className="mb-6">
          <ErrorNote message={actionError} />
        </div>
      )}

      {error ? (
        <ErrorNote message={`Could not read the library: ${error}`} />
      ) : entries === null ? (
        <Spinner label="Loading…" />
      ) : shown.length === 0 ? (
        <EmptyState title={EMPTY[tab].title}>{EMPTY[tab].body}</EmptyState>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-5 gap-y-9">
          {shown.map((entry) => (
            <LibraryCard
              key={entry.id}
              entry={entry}
              sub={cardLine(tab, entry)}
              pending={pending === entry.id}
              onOpen={() => open(entry, tab === "continueWatching" ? "resume" : "details")}
              onDismiss={tab === "continueWatching" ? () => dismiss(entry) : undefined}
            />
          ))}
        </div>
      )}
    </main>
  );
}

/** The line under a card's name: where it stands in this tab. */
function cardLine(tab: LibraryFilter, entry: LibraryEntry): string | null {
  switch (tab) {
    case "continueWatching":
      return continueLine(entry);
    case "library": {
      if (entry.resume) return [episodeLabel(entry), timeLeft(entry)].filter(Boolean).join(" · ");
      const added = formatDay(entry.addedAt);
      if (!added) return null;
      return `Added ${added === "Today" || added === "Yesterday" ? added.toLowerCase() : added}`;
    }
    case "history":
      return formatDay(entry.lastWatched);
    default:
      return unreachable(tab);
  }
}
