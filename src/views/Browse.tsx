// The endless grid follows Harbor's filter view (src/views/filter/cinemeta-fallback.tsx;
// Copyright (c) 2026 Harbor, MIT, see THIRD-PARTY-NOTICES.md).
import { LayoutGrid } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { PickCard } from "../components/cards";
import { EmptyState, Spinner } from "../components/ui";
import {
  type CatalogRef,
  type ExtraValue,
  boardableCatalogs,
  catalogDisplayName,
  catalogRef,
  catalogTypes,
  extraLabel,
  filterProps,
  typeLabel,
} from "../lib/catalogs";
import { StaleRequest, loadPage, pageEnds } from "../lib/rows";
import { scrollParent } from "../components/CatalogRows";
import { useAddons, useSettings } from "../lib/store";
import type { MetaPreview } from "../lib/types";
import { type BrowseFrame, useView } from "../lib/view";

/** Most titles one grid loads (Harbor's Cinemeta fallback grid has the same cap). */
const PAGE_CAP = 600;

/**
 * A catalog browser for every type: pick a type, one of its catalogs, and any filter the
 * catalog declares (genre, year, sort, …), then scroll an endless grid. Everything comes
 * from the installed addons' manifests; the choices live in the Browse frame, so Back
 * from a title lands here as it was.
 */
export function BrowseView({ frame }: { frame: BrowseFrame }) {
  const { addons } = useAddons();
  const { settings } = useSettings();
  const { openMeta, updateBrowse } = useView();
  const all = useMemo(
    () => boardableCatalogs(addons, settings?.showAdult ?? false),
    [addons, settings?.showAdult],
  );
  const types = catalogTypes(all);
  const type = types.includes(frame.type) ? frame.type : (types[0] ?? frame.type);
  const sources = all.filter((r) => r.catalog.type === type);
  const source = pickSource(sources, frame);
  const props = source ? filterProps(source.catalog) : [];
  const extra = source ? effectiveExtra(source, frame.extra) : [];
  const ref = source ? catalogRef(source.addon, source.catalog, extra) : null;

  const go = (next: Partial<Omit<BrowseFrame, "kind">>) =>
    updateBrowse({
      type,
      transportUrl: source?.addon.transportUrl,
      catalogId: source?.catalog.id,
      extra,
      ...next,
    });

  const selected = (name: string) => extra.filter((e) => e.name === name).map((e) => e.value);
  const setValues = (name: string, values: string[]) =>
    go({ extra: [...extra.filter((e) => e.name !== name), ...values.map((value) => ({ name, value }))] });

  return (
    <main className="absolute inset-0 z-30 overflow-y-auto bg-canvas">
      <header className="relative flex flex-col gap-6 px-12 pb-8 pt-28">
        <div className="flex items-center gap-3">
          <span className="flex h-9 w-9 items-center justify-center rounded-xl bg-elevated/70 text-ink-muted">
            <LayoutGrid size={16} />
          </span>
          <span className="text-[12.5px] font-medium uppercase tracking-[0.22em] text-ink-subtle">
            Browse
          </span>
        </div>
        <h1 className="font-display text-[56px] font-medium leading-[0.95] tracking-tight text-ink">
          {[typeLabel(type), ...extra.filter((e) => e.name === "genre").map((e) => e.value)].join(" · ")}
        </h1>

        {types.length > 1 && (
          <ChipRow label="Type">
            {types.map((t) => (
              <Chip key={t} on={t === type} onClick={() => go({ type: t, transportUrl: undefined, catalogId: undefined, extra: keepGenre(extra) })}>
                {typeLabel(t)}
              </Chip>
            ))}
          </ChipRow>
        )}

        {sources.length > 1 && (
          <ChipRow label="Catalog">
            {sources.map((r) => (
              <Chip
                key={r.key}
                on={r === source}
                onClick={() =>
                  go({ transportUrl: r.addon.transportUrl, catalogId: r.catalog.id, extra: keepGenre(extra) })
                }
              >
                {catalogDisplayName(r.catalog)}
                <span className="ml-1.5 text-ink-subtle">· {r.addon.manifest.name}</span>
              </Chip>
            ))}
          </ChipRow>
        )}

        {props.map((prop) => {
          const values = selected(prop.name);
          const multi = prop.optionsLimit > 1;
          return (
            <ChipRow key={prop.name} label={extraLabel(prop)}>
              {!prop.isRequired && (
                <Chip on={values.length === 0} onClick={() => setValues(prop.name, [])}>
                  All
                </Chip>
              )}
              {prop.options.map((option) => {
                const on = values.includes(option);
                return (
                  <Chip
                    key={option}
                    on={on}
                    onClick={() => {
                      if (!multi) return setValues(prop.name, [option]);
                      const next = on ? values.filter((v) => v !== option) : [...values, option];
                      // a required filter keeps at least one value; a multi-choice one
                      // at most optionsLimit (the oldest choice gives way)
                      if (next.length === 0 && prop.isRequired) return;
                      setValues(prop.name, next.slice(-prop.optionsLimit));
                    }}
                  >
                    {option}
                  </Chip>
                );
              })}
            </ChipRow>
          );
        })}
      </header>

      <div className="flex flex-col gap-8 px-12 pb-24">
        {ref ? (
          <BrowseGrid key={ref.key} catalog={ref} onOpen={openMeta} />
        ) : (
          <EmptyState title={`No ${typeLabel(type).toLowerCase()} catalogs`}>
            None of your addons has a {typeLabel(type).toLowerCase()} catalog to browse
            {frame.extra.some((e) => e.name === "genre") ? " with this genre" : ""}.
          </EmptyState>
        )}
      </div>
    </main>
  );
}

/** The frame's catalog; else the first of the type that offers the frame's genre; else
 * the type's first catalog. */
function pickSource(sources: CatalogRef[], frame: BrowseFrame): CatalogRef | null {
  const named = sources.find(
    (r) => r.addon.transportUrl === frame.transportUrl && r.catalog.id === frame.catalogId,
  );
  if (named) return named;
  const genre = frame.extra.find((e) => e.name === "genre")?.value;
  if (genre) {
    return (
      sources.find((r) =>
        filterProps(r.catalog).some((p) => p.name === "genre" && p.options.includes(genre)),
      ) ?? null
    );
  }
  return sources[0] ?? null;
}

/** The extras to load `source` with: the frame's choices the catalog declares (capped
 * at each filter's optionsLimit), and each required filter's first option when the
 * frame names none. */
function effectiveExtra(source: CatalogRef, wanted: ExtraValue[]): ExtraValue[] {
  return filterProps(source.catalog).flatMap((prop) => {
    const values = wanted
      .filter((e) => e.name === prop.name && prop.options.includes(e.value))
      .map((e) => e.value)
      .slice(0, Math.max(1, prop.optionsLimit));
    const chosen = values.length > 0 ? values : prop.isRequired ? [prop.options[0]] : [];
    return chosen.map((value) => ({ name: prop.name, value }));
  });
}

/** Moving to another catalog or type keeps the genre (when the new one has it). */
function keepGenre(extra: ExtraValue[]): ExtraValue[] {
  return extra.filter((e) => e.name === "genre" && !/^\d{4}$/.test(e.value));
}

function BrowseGrid({
  catalog,
  onOpen,
}: {
  catalog: CatalogRef;
  onOpen: (meta: MetaPreview) => void;
}) {
  const [items, setItems] = useState<MetaPreview[]>([]);
  const [exhausted, setExhausted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const loading = useRef(false);
  // the next page's skip: titles the addon sent so far, adult ones included
  const offset = useRef(0);
  const left = useRef(false);
  const sentinel = useRef<HTMLDivElement>(null);

  const loadNext = useRef(async () => {});
  const shown = useRef<MetaPreview[]>([]);
  loadNext.current = async () => {
    if (loading.current || exhausted) return;
    loading.current = true;
    let again = false;
    try {
      const response = await loadPage(catalog, offset.current, () => left.current);
      offset.current += response.fetched;
      const seen = new Set(shown.current.map((m) => m.id));
      const fresh = response.metas.filter((m) => !seen.has(m.id));
      const next = [...shown.current, ...fresh];
      shown.current = next;
      setItems(next);
      if (pageEnds(catalog, response, fresh.length, offset.current) || next.length >= PAGE_CAP) {
        setExhausted(true);
      } else {
        // a page Rust emptied (all adult) leaves the sentinel where it was, in view,
        // and the observer won't fire again: ask for the next one now
        again = response.metas.length === 0;
      }
    } catch (e) {
      if (e instanceof StaleRequest) return;
      setError(e && typeof e === "object" && "message" in e ? String(e.message) : String(e));
      setExhausted(true);
    } finally {
      loading.current = false;
    }
    if (again) void loadNext.current();
  };

  useEffect(() => {
    left.current = false;
    void loadNext.current();
    return () => {
      left.current = true;
    };
  }, []);

  useEffect(() => {
    const el = sentinel.current;
    if (!el || exhausted) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) void loadNext.current();
      },
      { root: scrollParent(el), rootMargin: "800px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [exhausted, items.length]);

  return (
    <>
      {error && items.length === 0 && (
        <EmptyState title="Could not load this catalog">{error}</EmptyState>
      )}
      {!error && exhausted && items.length === 0 && (
        <EmptyState title="Nothing here">
          {catalog.addon.manifest.name} has nothing for these filters.
        </EmptyState>
      )}
      <div className="grid grid-cols-3 gap-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-7">
        {items.map((meta, i) => (
          <PickCard key={`${meta.id}-${i}`} meta={meta} onClick={() => onOpen(meta)} />
        ))}
      </div>
      {!exhausted && (
        <div ref={sentinel} className="flex justify-center py-6">
          <Spinner />
        </div>
      )}
      {exhausted && items.length > 0 && (
        <p className="text-center text-[11.5px] text-ink-subtle">
          That's everything {catalog.addon.manifest.name} has here.
        </p>
      )}
    </>
  );
}

function ChipRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 items-center gap-4">
      <span className="w-16 shrink-0 text-[11px] font-semibold uppercase tracking-[0.18em] text-ink-subtle">
        {label}
      </span>
      <div className="no-scrollbar flex min-w-0 gap-2 overflow-x-auto py-1">{children}</div>
    </div>
  );
}

function Chip({
  on,
  onClick,
  children,
}: {
  on: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      aria-pressed={on}
      className={`h-9 shrink-0 whitespace-nowrap rounded-full border px-4 text-[13px] font-medium transition-colors ${
        on
          ? "border-ink bg-ink text-canvas"
          : "border-edge-soft text-ink-muted hover:border-ink hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}
