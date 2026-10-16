import { useMemo } from "react";
import { PickCard, TopRankCard } from "../components/cards";
import { HeroCarousel } from "../components/Hero";
import { Row } from "../components/Row";
import { EmptyState } from "../components/ui";
import { browsableCatalogs, catalogDisplayName } from "../lib/catalogs";
import { dedupRows, useCatalogRows } from "../lib/rows";
import { useAddons, useSettings } from "../lib/store";
import { useView } from "../lib/view";
import { RowSkeletons } from "./Home";

const ROOMS = {
  movies: {
    types: ["movie"],
    label: "Movies",
    hero: "bleed" as const,
    kicker: "Featured tonight",
    title: "Movies",
    subtitle: "Every movie catalog your addons provide, in one room.",
  },
  shows: {
    types: ["series", "tv"],
    label: "Series",
    hero: "mast" as const,
    kicker: "Tonight",
    title: "Tonight's lineup",
    subtitle: "Series from your installed addons — trending, returning, and all-time runs.",
  },
  anime: {
    types: ["anime"],
    label: "Anime",
    hero: "mast" as const,
    kicker: "Simulcast",
    title: "Anime harbor",
    subtitle: "Every anime catalog your addons provide, in one room.",
  },
};

export function TypeRoomView({ room }: { room: keyof typeof ROOMS }) {
  const config = ROOMS[room];
  const { addons, reload } = useAddons();
  const { settings, save } = useSettings();
  const { openMeta, setView } = useView();
  const catalogs = useMemo(
    () => browsableCatalogs(addons).filter((c) => config.types.includes(c.catalog.type)),
    [addons, config.types],
  );
  const { rows, loadMore } = useCatalogRows(catalogs);

  if (catalogs.length === 0) {
    // the built-in Anime Kitsu fills the Anime room; point at its switch when it is off
    const kitsuOff = room === "anime" && settings?.builtinKitsu === false;
    return (
      <main className="flex-1 overflow-y-auto px-12 pt-32">
        <EmptyState title={`No ${config.label.toLowerCase()} catalogs`}>
          {kitsuOff
            ? "The built-in Anime Kitsu addon is switched off (Settings → Content), and none of your installed addons provides anime catalogs."
            : `None of your installed addons provides ${config.label.toLowerCase()} catalogs yet.`}
          <div className="mt-5 flex justify-center gap-3">
            {kitsuOff && (
              <button
                onClick={async () => {
                  await save({ builtinKitsu: true });
                  await reload();
                }}
                className="h-11 rounded-full bg-ink px-6 text-[14px] font-semibold text-canvas transition-transform hover:scale-[1.03]"
              >
                Turn on Anime Kitsu
              </button>
            )}
            <button
              onClick={() => setView("addons")}
              className={
                kitsuOff
                  ? "h-11 rounded-full border border-edge px-6 text-[14px] font-semibold text-ink-muted transition-colors hover:border-ink hover:text-ink"
                  : "h-11 rounded-full bg-ink px-6 text-[14px] font-semibold text-canvas transition-transform hover:scale-[1.03]"
              }
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

  const rowsBlock = (
    <>
      {rankItems.length >= 10 && (
        <Row title={`Top 10 ${config.label} Today`} min={216}>
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
      {ordered.length === 0 && <RowSkeletons count={4} />}
      {deduped.map(({ key, state, items }) => (
        <Row
          key={key}
          title={catalogDisplayName(state.ref.catalog)}
          kicker={`${state.ref.addon.manifest.name}`}
          min={148}
          onEndReached={() => loadMore(key)}
        >
          {items.map((meta, i) => (
            <PickCard
              key={`${meta.id}-${i}`}
              meta={meta}
              onClick={() => openMeta(meta)}
            />
          ))}
        </Row>
      ))}
    </>
  );

  if (config.hero === "bleed") {
    return (
      <main className="relative h-full overflow-y-auto bg-canvas">
        <HeroCarousel
          items={heroItems}
          variant="bleed"
          typeLabel={config.label}
          autoMs={11000}
          onOpen={(meta) => openMeta(meta)}
        />
        <div className="relative mx-auto flex max-w-[1700px] flex-col gap-12 px-12 pb-32 pt-12">
          {rowsBlock}
        </div>
      </main>
    );
  }

  return (
    <main className="relative h-full overflow-y-auto bg-canvas">
      <div className="relative mx-auto flex max-w-[1700px] flex-col gap-12 px-12 pb-32 pt-32">
        <header className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.42em] text-ink-subtle">
            {config.kicker}
          </span>
          <h1 className="font-display text-[44px] font-medium leading-[1.05] tracking-tight text-ink">
            {config.title}
          </h1>
          <p className="max-w-2xl text-[15px] leading-relaxed text-ink-muted">{config.subtitle}</p>
        </header>
        {heroItems.length > 0 && (
          <HeroCarousel
            items={heroItems}
            typeLabel={config.label}
            onOpen={(meta) => openMeta(meta)}
          />
        )}
        {rowsBlock}
      </div>
    </main>
  );
}
