import { ChevronLeft, ChevronRight, LayoutGrid } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { CatalogRows } from "../components/CatalogRows";
import { GenreTiles } from "../components/GenreTiles";
import { Poster, fallbackGradient } from "../components/Poster";
import { EmptyState } from "../components/ui";
import { catalogTypes, looksAnime, typeLabel } from "../lib/catalogs";
import { roomCatalogs } from "../lib/rooms";
import { type RowState, leadRow, useCatalogRows } from "../lib/rows";
import { useAddons, useSettings } from "../lib/store";
import { useView } from "../lib/view";
import type { MetaPreview } from "../lib/types";

export function DiscoverView() {
  const { addons } = useAddons();
  const { settings } = useSettings();
  const { openMeta, openBrowse, setView } = useView();
  const hideAnime = settings?.hideAnime ?? false;
  const showAdult = settings?.showAdult ?? false;
  const hideLiveTv = settings?.hideLiveTv ?? false;
  const catalogs = useMemo(
    () => roomCatalogs(addons, "home", { hideAnime, showAdult, hideLiveTv }),
    [addons, hideAnime, showAdult, hideLiveTv],
  );
  const { rows, loadMore, request, version } = useCatalogRows(catalogs);

  if (catalogs.length === 0) {
    return (
      <main className="flex-1 overflow-y-auto px-12 pt-32">
        <EmptyState title="Nothing to discover yet">
          Install an addon with catalogs to fill this page.
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

  // an anime catalog filed under series, found out once it loaded
  const keep = hideAnime ? (state: RowState) => !looksAnime(state.items.map((m) => m.id)) : undefined;
  const lead = leadRow(catalogs, rows, keep) ?? catalogs[0];
  const first = rows.get(lead.key);
  const featured = (first?.items ?? []).filter((m) => m.background || m.poster).slice(0, 8);
  const reserved = new Set(featured.map((m) => m.id));
  const types = catalogTypes(catalogs);
  const rowProps = {
    rows,
    request,
    loadMore,
    version,
    reserved,
    keep,
    onOpen: openMeta,
    kicker: (ref: (typeof catalogs)[number]) =>
      `${ref.addon.manifest.name} · ${typeLabel(ref.catalog.type)}`,
  };

  return (
    <main className="flex-1 overflow-y-auto px-12 pb-20 pt-20">
      <div className="flex flex-col gap-14">
        <FeaturedBanner items={featured} onOpen={(m) => openMeta(m)} />
        <CatalogRows refs={[lead]} {...rowProps} />
        {types.map((type) => (
          <GenreTiles key={type} type={type} />
        ))}
        <div className="flex justify-center">
          <button
            onClick={() => openBrowse({ type: types[0], extra: [] })}
            className="flex h-10 items-center gap-2 rounded-full border border-edge-soft px-5 text-[13px] font-medium text-ink-muted transition-colors hover:border-ink hover:text-ink"
          >
            <LayoutGrid size={15} />
            Browse everything
          </button>
        </div>
        <CatalogRows
          refs={catalogs.filter((c) => c !== lead)}
          {...rowProps}
          reserved={new Set([...reserved, ...(first?.items ?? []).slice(0, 20).map((m) => m.id)])}
        />
      </div>
    </main>
  );
}

// ---- Featured & Recommended (Harbor's BigCardStack + SidePanel) ------------------------

function FeaturedBanner({
  items,
  onOpen,
}: {
  items: MetaPreview[];
  onOpen: (meta: MetaPreview) => void;
}) {
  const [active, setActive] = useState(0);
  const hovered = useRef(false);

  useEffect(() => {
    if (items.length < 2) return;
    const timer = setInterval(() => {
      if (!hovered.current && !document.hidden) setActive((a) => (a + 1) % items.length);
    }, 14000);
    return () => clearInterval(timer);
  }, [items.length]);

  if (items.length === 0) {
    return (
      <div className="grid grid-cols-[minmax(0,1fr)_320px] gap-4">
        <div className="aspect-[16/9] animate-pulse rounded-2xl bg-elevated/30" />
        <div className="animate-pulse rounded-2xl bg-elevated/20" />
      </div>
    );
  }

  const current = items[Math.min(active, items.length - 1)];

  return (
    <section className="flex flex-col gap-3">
      <h2 className="font-display text-[28px] font-medium tracking-tight text-ink">
        Featured &amp; Recommended
      </h2>
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
        <div
          className="group relative block h-full min-h-[420px] w-full cursor-pointer overflow-hidden rounded-2xl border border-edge-soft bg-canvas transition-transform duration-300 ease-[cubic-bezier(0.32,0.72,0.24,1)] hover:-translate-y-1"
          onMouseEnter={() => (hovered.current = true)}
          onMouseLeave={() => (hovered.current = false)}
          onClick={() => onOpen(current)}
        >
          {items.map((meta, i) => {
            const art = meta.background ?? meta.poster;
            return (
              <div
                key={meta.id}
                className={`absolute inset-[2px] overflow-hidden rounded-[14px] transition-opacity duration-700 ${
                  i === active ? "opacity-100" : "opacity-0"
                }`}
              >
                {art ? (
                  <img src={art} alt="" className="h-full w-full object-cover" />
                ) : (
                  <div className="h-full w-full" style={{ background: fallbackGradient(meta.id) }} />
                )}
                <div
                  className="absolute inset-0"
                  style={{
                    background:
                      "linear-gradient(to top, oklch(0.10 0.02 260/0.92) 0%, oklch(0.10 0.02 260/0.20) 38%, transparent 60%)",
                  }}
                />
              </div>
            );
          })}
          <span className="absolute left-4 top-4 rounded-full bg-canvas/55 px-2.5 py-1 text-[10.5px] font-semibold uppercase tracking-[0.22em] text-accent">
            Featured
          </span>
          <div className="absolute bottom-5 left-6 right-6">
            <div className="font-display text-[42px] font-medium leading-none text-ink drop-shadow-[0_2px_18px_rgba(0,0,0,0.5)]">
              {current.name}
            </div>
            <div className="mt-2 text-[13px] text-ink/80">
              {[current.releaseInfo, current.imdbRating && `IMDb ${current.imdbRating}`]
                .filter(Boolean)
                .join(" · ")}
            </div>
          </div>
          <NavCircle side="left" onClick={() => setActive((active - 1 + items.length) % items.length)} />
          <NavCircle side="right" onClick={() => setActive((active + 1) % items.length)} />
        </div>

        <aside className="hidden flex-col gap-3 self-start rounded-2xl border border-edge-soft bg-elevated/35 p-4 xl:flex">
          <div className="flex items-baseline justify-between gap-3">
            <span className="truncate font-display text-[19px] font-medium text-ink">
              {current.name}
            </span>
            <span className="shrink-0 text-[12px] uppercase tracking-[0.16em] text-ink-subtle">
              {String(active + 1).padStart(2, "0")} / {String(items.length).padStart(2, "0")}
            </span>
          </div>
          <div className="grid grid-cols-2 gap-2">
            {items
              .filter((_, i) => i !== active)
              .slice(0, 4)
              .map((meta) => (
                <button
                  key={meta.id}
                  onClick={() => setActive(items.indexOf(meta))}
                  className="group/still overflow-hidden rounded-md border border-edge-soft transition-colors hover:border-ink"
                >
                  <Poster
                    src={meta.background ?? meta.poster}
                    seed={meta.id}
                    ratio="landscape"
                    className="rounded-none transition-transform duration-300 group-hover/still:scale-[1.04]"
                  />
                </button>
              ))}
          </div>
          {current.description && (
            <p className="line-clamp-3 text-[12.5px] leading-relaxed text-ink-muted">
              {current.description}
            </p>
          )}
          {current.imdbRating && (
            <span className="w-fit rounded-full border border-edge-soft bg-canvas/40 px-2.5 py-1 text-[12px] font-semibold text-ink">
              IMDb {current.imdbRating} · Top Rated
            </span>
          )}
        </aside>
      </div>
      {items.length > 1 && (
        <div className="flex justify-center gap-2">
          {items.map((_, i) => (
            <button
              key={i}
              aria-label={`Featured ${i + 1}`}
              onClick={() => setActive(i)}
              className={`h-1.5 rounded-full transition-all duration-300 ${
                i === active ? "w-7 bg-ink" : "w-1.5 bg-ink-subtle/40"
              }`}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function NavCircle({ side, onClick }: { side: "left" | "right"; onClick: () => void }) {
  const Icon = side === "left" ? ChevronLeft : ChevronRight;
  return (
    <button
      aria-label={side === "left" ? "Previous" : "Next"}
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
      className={`absolute top-1/2 flex h-12 w-12 -translate-y-1/2 items-center justify-center rounded-full bg-canvas/65 opacity-0 backdrop-blur-md transition-all duration-200 hover:scale-105 hover:bg-canvas/85 group-hover:opacity-100 ${
        side === "left" ? "left-3" : "right-3"
      }`}
    >
      <Icon size={20} />
    </button>
  );
}
