import { ArrowDown, ArrowUp, Eye, EyeOff, SlidersHorizontal } from "lucide-react";
import { type ReactNode, useMemo, useState } from "react";
import { CatalogRows } from "../components/CatalogRows";
import { TopRankCard } from "../components/cards";
import { HeroCarousel } from "../components/Hero";
import { LibraryCard } from "../components/LibraryCard";
import { Row } from "../components/Row";
import { EmptyState, Spinner } from "../components/ui";
import { catalogTitle, looksAnime, typeLabel } from "../lib/catalogs";
import {
  EMPTY_HOME_ROWS,
  applyHomeRowCustomization,
  isRowHidden,
  moveRow,
  renameRow,
  resetHomeRows,
  toggleRowHidden,
} from "../lib/home-customization";
import { continueLine, useLibrary, useOpenLibraryEntry } from "../lib/library";
import { libraryClearProgress } from "../lib/library-ipc";
import { roomCatalogs } from "../lib/rooms";
import { type RowState, leadRow, useCatalogRows } from "../lib/rows";
import { useAddons, useSettings } from "../lib/store";
import type { HomeRows } from "../lib/types";
import { useView } from "../lib/view";

/** How many titles the Home row shows; the Library page has all of them. */
const CONTINUE_ROW_MAX = 20;

export function HomeView() {
  const { addons, loaded } = useAddons();
  const { settings, save } = useSettings();
  const { openMeta, setView } = useView();
  const [customizing, setCustomizing] = useState(false);
  const hideAnime = settings?.hideAnime ?? false;
  const showAdult = settings?.showAdult ?? false;
  const custom = settings?.homeRows ?? EMPTY_HOME_ROWS;
  // every row Home can show (movies and series, then anime unless hidden, then the rest)
  const all = useMemo(
    () =>
      roomCatalogs(addons, "home", { hideAnime, showAdult }).map((ref) => ({
        ...ref,
        name: catalogTitle(ref),
      })),
    [addons, hideAnime, showAdult],
  );
  const catalogs = useMemo(() => applyHomeRowCustomization(all, custom), [all, custom]);
  const { rows, loadMore, request, version } = useCatalogRows(catalogs);
  // an anime catalog filed under series, found out once it loaded
  const keep = hideAnime
    ? (state: RowState) => !looksAnime(state.items.map((m) => m.id))
    : undefined;

  if (!loaded) {
    return (
      <main className="flex-1 overflow-y-auto px-12 pt-28">
        <Spinner label="Loading…" />
      </main>
    );
  }

  if (all.length === 0) {
    return (
      <main className="flex-1 overflow-y-auto px-12 pt-32">
        <EmptyState title="Welcome aboard">
          Zeiler ships with no content. Install an addon that provides catalogs and this page
          becomes your harbor.
          <div className="mt-5">
            <button
              onClick={() => setView("addons")}
              className="h-11 rounded-full bg-ink px-6 text-[14px] font-semibold text-canvas transition-transform hover:scale-[1.03]"
            >
              Open Addons
            </button>
          </div>
        </EmptyState>
      </main>
    );
  }

  // the hero and Top 10 come from the first row (skipping one that turned out anime)
  const lead = leadRow(catalogs, rows, keep) ?? catalogs[0];
  const first = rows.get(lead?.key ?? "");
  const heroItems = (first?.items ?? []).filter((m) => m.background || m.poster).slice(0, 5);
  const heroIds = new Set(heroItems.map((m) => m.id));
  const rankItems = (first?.items ?? []).filter((m) => !heroIds.has(m.id)).slice(0, 10);
  const reserved = new Set([...heroIds, ...rankItems.map((m) => m.id)]);

  return (
    <main className="flex-1 overflow-y-auto px-5 pb-14 pt-24 sm:px-8 lg:px-12 lg:pt-28">
      <div className="flex flex-col gap-12">
        {catalogs.length > 0 && (
          <HeroCarousel
            items={heroItems}
            typeLabel={typeLabel(lead.catalog.type)}
            onOpen={(meta) => openMeta(meta)}
          />
        )}

        <ContinueWatchingRow />

        {customizing ? (
          <CustomizePanel
            rows={all}
            custom={custom}
            onChange={(homeRows) => void save({ homeRows })}
            onDone={() => setCustomizing(false)}
          />
        ) : (
          <>
            {rankItems.length >= 10 && (
              <Row title={`Top 10 — ${lead.name}`} min={180}>
                {rankItems.map((meta, i) => (
                  <TopRankCard
                    key={meta.id}
                    meta={meta}
                    rank={i + 1}
                    onClick={() => openMeta(meta)}
                  />
                ))}
              </Row>
            )}
            {catalogs.length === 0 && (
              <EmptyState title="Every row is hidden">
                Customize Home to show some of them again.
              </EmptyState>
            )}
            <CatalogRows
              refs={catalogs}
              rows={rows}
              request={request}
              loadMore={loadMore}
              version={version}
              reserved={reserved}
              onOpen={openMeta}
              title={(ref) => ref.name}
              kicker={(ref) => `${ref.addon.manifest.name} · ${typeLabel(ref.catalog.type)}`}
              keep={keep}
            />
            <div className="flex justify-center">
              <button
                onClick={() => setCustomizing(true)}
                className="flex h-10 items-center gap-2 rounded-full border border-edge-soft px-5 text-[13px] font-medium text-ink-muted transition-colors hover:border-ink hover:text-ink"
              >
                <SlidersHorizontal size={15} />
                Customize Home
              </button>
            </div>
          </>
        )}
      </div>
    </main>
  );
}

/** Reorder, hide and rename Home's catalog rows (saved in settings as `homeRows`). */
function CustomizePanel({
  rows,
  custom,
  onChange,
  onDone,
}: {
  rows: { key: string; name: string; addon: { manifest: { name: string } }; catalog: { type: string } }[];
  custom: HomeRows;
  onChange: (next: HomeRows) => void;
  onDone: () => void;
}) {
  const keys = rows.map((r) => r.key);
  const ordered = applyHomeRowCustomization(rows, custom, true);
  const originals = new Map(rows.map((r) => [r.key, r.name]));
  return (
    <section className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h2 className="font-display text-[26px] font-medium tracking-tight text-ink">
            Customize Home
          </h2>
          <p className="text-[13px] text-ink-muted">
            Move rows up or down, hide the ones you don't want, or give them your own names.
          </p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => onChange(resetHomeRows())}
            className="h-10 rounded-full border border-edge-soft px-5 text-[13px] font-medium text-ink-muted transition-colors hover:border-ink hover:text-ink"
          >
            Reset
          </button>
          <button
            onClick={onDone}
            className="h-10 rounded-full bg-ink px-5 text-[13px] font-semibold text-canvas"
          >
            Done
          </button>
        </div>
      </div>
      <ol className="flex flex-col gap-2">
        {ordered.map((row, i) => {
          const hidden = isRowHidden(custom, row.key);
          const original = originals.get(row.key) ?? row.name;
          return (
            <li
              key={row.key}
              className={`flex items-center gap-3 rounded-2xl border border-edge-soft bg-elevated/40 px-4 py-2.5 ${
                hidden ? "opacity-55" : ""
              }`}
            >
              <div className="flex min-w-0 flex-1 flex-col">
                <input
                  // remounts with the new name after a reset
                  key={row.name}
                  aria-label={`Name of the row ${original}`}
                  defaultValue={row.name}
                  maxLength={80}
                  placeholder={original}
                  onBlur={(e) => {
                    const next = renameRow(custom, row.key, e.target.value, original);
                    if (JSON.stringify(next.renamed) !== JSON.stringify(custom.renamed)) onChange(next);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") e.currentTarget.blur();
                  }}
                  className="h-8 min-w-0 rounded-lg bg-transparent px-2 text-[15px] font-medium text-ink outline-none focus:bg-canvas/60"
                />
                <span className="px-2 text-[11px] uppercase tracking-[0.16em] text-ink-subtle">
                  {row.addon.manifest.name} · {typeLabel(row.catalog.type)}
                  {row.name !== original && ` · was ${original}`}
                </span>
              </div>
              <IconButton label="Move up" disabled={i === 0} onClick={() => onChange(moveRow(custom, keys, row.key, -1))}>
                <ArrowUp size={16} />
              </IconButton>
              <IconButton
                label="Move down"
                disabled={i === ordered.length - 1}
                onClick={() => onChange(moveRow(custom, keys, row.key, 1))}
              >
                <ArrowDown size={16} />
              </IconButton>
              <IconButton label={hidden ? "Show" : "Hide"} onClick={() => onChange(toggleRowHidden(custom, row.key))}>
                {hidden ? <EyeOff size={16} /> : <Eye size={16} />}
              </IconButton>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

function IconButton({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-ink-muted transition-colors hover:bg-canvas/60 hover:text-ink disabled:opacity-30 disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}

/**
 * The titles the user is partway through, most recent first: a card resumes (the
 * stream picker for the episode or movie it stopped at); its ✕ takes it out of the row.
 */
function ContinueWatchingRow() {
  const { continueWatching } = useLibrary();
  const { setView } = useView();
  const { open, pending } = useOpenLibraryEntry();
  const shown = continueWatching.slice(0, CONTINUE_ROW_MAX);
  // Row measures its cells once, on mount: mount it only with cards to measure
  if (shown.length === 0) return null;
  return (
    <Row
      title={
        <button onClick={() => setView("library")} className="transition-colors hover:text-accent">
          Continue Watching
        </button>
      }
      count={continueWatching.length > 1 ? continueWatching.length : undefined}
      min={148}
    >
      {shown.map((entry) => (
        <LibraryCard
          key={entry.id}
          entry={entry}
          sub={continueLine(entry)}
          pending={pending === entry.id}
          onOpen={() => open(entry, "resume")}
          // a failed dismiss leaves the card where it is
          onDismiss={() => void libraryClearProgress(entry.id).catch(() => undefined)}
        />
      ))}
    </Row>
  );
}

export function RowSkeletons({ count = 5 }: { count?: number }) {
  return (
    <>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="flex flex-col gap-5 pl-[9px]">
          <div className="h-5 w-44 rounded bg-elevated/30" />
          <div className="flex gap-5 overflow-hidden">
            {Array.from({ length: 8 }, (_, j) => (
              <div key={j} className="w-[148px] shrink-0">
                <div className="aspect-[2/3] rounded-xl bg-elevated/40" />
                <div className="mt-2 h-3 w-3/5 rounded bg-elevated/35" />
              </div>
            ))}
          </div>
        </div>
      ))}
    </>
  );
}
