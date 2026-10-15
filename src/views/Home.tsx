import { useMemo } from "react";
import { PickCard, TopRankCard } from "../components/cards";
import { HeroCarousel } from "../components/Hero";
import { Row } from "../components/Row";
import { EmptyState, Spinner } from "../components/ui";
import { browsableCatalogs, catalogDisplayName } from "../lib/catalogs";
import { dedupRows, useCatalogRows } from "../lib/rows";
import { useAddons } from "../lib/store";
import { useView } from "../lib/view";

const TYPE_LABEL: Record<string, string> = {
  movie: "Movies",
  series: "Series",
  anime: "Anime",
  tv: "TV",
};

export function HomeView() {
  const { addons, loaded } = useAddons();
  const { openMeta, setView } = useView();
  const catalogs = useMemo(() => {
    const all = browsableCatalogs(addons);
    // movie/series first, then the rest, keeping addon order inside each bucket
    return [
      ...all.filter((c) => c.catalog.type === "movie" || c.catalog.type === "series"),
      ...all.filter((c) => c.catalog.type !== "movie" && c.catalog.type !== "series"),
    ];
  }, [addons]);
  const { rows, loadMore } = useCatalogRows(catalogs);

  if (!loaded) {
    return (
      <main className="flex-1 overflow-y-auto px-12 pt-28">
        <Spinner label="Loading…" />
      </main>
    );
  }

  if (catalogs.length === 0) {
    return (
      <main className="flex-1 overflow-y-auto px-12 pt-32">
        <EmptyState title="Welcome aboard">
          Sailor ships with no content. Install an addon that provides catalogs and this page
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

  const first = rows.get(catalogs[0]?.key ?? "");
  const heroItems = (first?.items ?? []).filter((m) => m.background || m.poster).slice(0, 5);
  const heroIds = new Set(heroItems.map((m) => m.id));
  const rankItems = (first?.items ?? []).filter((m) => !heroIds.has(m.id)).slice(0, 10);
  const reserved = new Set([...heroIds, ...rankItems.map((m) => m.id)]);

  const ordered = catalogs
    .map((c) => ({ key: c.key, state: rows.get(c.key) }))
    .filter((r): r is { key: string; state: NonNullable<typeof r.state> } => !!r.state);
  const deduped = dedupRows(ordered, reserved);
  const anythingLoaded = ordered.length > 0;

  const firstTypeLabel = TYPE_LABEL[catalogs[0]?.catalog.type ?? ""] ?? "Catalog";

  return (
    <main className="flex-1 overflow-y-auto px-5 pb-14 pt-24 sm:px-8 lg:px-12 lg:pt-28">
      <div className="flex flex-col gap-12">
        <HeroCarousel
          items={heroItems}
          typeLabel={firstTypeLabel}
          onOpen={(meta) => openMeta(meta.type, meta.id)}
        />

        {rankItems.length >= 10 && (
          <Row title={`Top 10 — ${catalogDisplayName(catalogs[0].catalog)}`} min={180}>
            {rankItems.map((meta, i) => (
              <TopRankCard
                key={meta.id}
                meta={meta}
                rank={i + 1}
                onClick={() => openMeta(meta.type, meta.id)}
              />
            ))}
          </Row>
        )}

        {!anythingLoaded && <RowSkeletons />}

        {deduped.map(({ key, state, items }) => (
          <Row
            key={key}
            title={catalogDisplayName(state.ref.catalog)}
            kicker={`${state.ref.addon.manifest.name} · ${state.ref.catalog.type}`}
            min={148}
            onEndReached={() => loadMore(key)}
          >
            {items.map((meta, i) => (
              <PickCard
                key={`${meta.id}-${i}`}
                meta={meta}
                onClick={() => openMeta(meta.type, meta.id)}
              />
            ))}
          </Row>
        ))}
      </div>
    </main>
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
