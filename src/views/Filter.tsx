import { Tag } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { PickCard } from "../components/cards";
import { EmptyState, Spinner } from "../components/ui";
import { browsableCatalogs, genreOptions, supportsExtra } from "../lib/catalogs";
import { catalogLoad } from "../lib/ipc";
import { useAddons } from "../lib/store";
import { useView } from "../lib/view";
import type { MetaPreview } from "../lib/types";

const PAGE_CAP = 600;

/** Genre browse page (Harbor's filter view, Cinemeta-grid flavor): infinite grid. */
export function FilterView({ type, genre }: { type: string; genre: string }) {
  const { addons } = useAddons();
  const { openMeta } = useView();
  const [items, setItems] = useState<MetaPreview[]>([]);
  const [exhausted, setExhausted] = useState(false);
  const [error, setError] = useState(false);
  const loadingRef = useRef(false);
  const sentinelRef = useRef<HTMLDivElement>(null);

  // the first catalog of this type that declares a genre extra (options optional)
  const source = browsableCatalogs(addons).find(
    (c) =>
      c.catalog.type === type &&
      supportsExtra(c.catalog, "genre") &&
      (genreOptions(c.catalog).length === 0 || genreOptions(c.catalog).includes(genre)),
  );

  const loadPage = useCallback(
    async (skip: number) => {
      if (!source || loadingRef.current) return;
      loadingRef.current = true;
      try {
        const extra = [{ name: "genre", value: genre }];
        if (skip > 0) extra.push({ name: "skip", value: String(skip) });
        const response = await catalogLoad({
          transportUrl: source.addon.transportUrl,
          type: source.catalog.type,
          id: source.catalog.id,
          extra,
        });
        setItems((prev) => {
          const seen = new Set(prev.map((m) => m.id));
          const fresh = response.metas.filter((m) => !seen.has(m.id));
          if (fresh.length === 0 || prev.length + fresh.length >= PAGE_CAP) setExhausted(true);
          if (!supportsExtra(source.catalog, "skip")) setExhausted(true);
          return [...prev, ...fresh];
        });
      } catch {
        setError(true);
        setExhausted(true);
      } finally {
        loadingRef.current = false;
      }
    },
    [source, genre],
  );

  useEffect(() => {
    setItems([]);
    setExhausted(false);
    setError(false);
    void loadPage(0);
  }, [loadPage]);

  // infinite scroll sentinel
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el || exhausted) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          setItems((current) => {
            void loadPage(current.length);
            return current;
          });
        }
      },
      { rootMargin: "800px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [exhausted, loadPage]);

  return (
    <main className="absolute inset-0 z-30 overflow-y-auto bg-canvas">
      <header className="relative px-12 pb-10 pt-28">
        <div className="flex items-center gap-3">
          <span className="flex h-9 w-9 items-center justify-center rounded-xl bg-elevated/70 text-ink-muted">
            <Tag size={16} />
          </span>
          <span className="text-[12.5px] font-medium uppercase tracking-[0.22em] text-ink-subtle">
            Genre · {type}
          </span>
        </div>
        <h1 className="mt-3 font-display text-[64px] font-medium leading-[0.95] tracking-tight text-ink">
          {genre}
        </h1>
        <p className="mt-3 max-w-2xl text-[15px] leading-relaxed text-ink-muted">
          Everything your addons file under {genre.toLowerCase()}.
        </p>
      </header>

      <div className="flex flex-col gap-8 px-12 pb-24">
        {!source && (
          <EmptyState title="No genre catalog">
            None of your installed addons declares a {type} catalog with genre filtering.
          </EmptyState>
        )}
        {source && error && items.length === 0 && (
          <EmptyState title="Could not load this genre">
            {source.addon.manifest.name} did not answer. Try again later.
          </EmptyState>
        )}
        <div className="grid grid-cols-3 gap-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-7">
          {items.map((meta, i) => (
            <PickCard
              key={`${meta.id}-${i}`}
              meta={meta}
              onClick={() => openMeta(meta)}
            />
          ))}
        </div>
        {source && !exhausted && (
          <div ref={sentinelRef} className="flex justify-center py-6">
            <Spinner />
          </div>
        )}
        {exhausted && items.length > 0 && (
          <p className="text-center text-[11.5px] text-ink-subtle">
            That's everything {source?.addon.manifest.name} has for {genre}.
          </p>
        )}
      </div>
    </main>
  );
}
