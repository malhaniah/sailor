import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { navigate, useAddons } from "../App";
import { PosterCard } from "../components/PosterCard";
import { EmptyState, ErrorNote, Spinner } from "../components/ui";
import {
  browsableCatalogs,
  catalogDisplayName,
  genreOptions,
  supportsExtra,
} from "../lib/catalogs";
import { catalogLoad, errorMessage } from "../lib/ipc";
import type { MetaPreview } from "../lib/types";

export function DiscoverPage() {
  const { addons, loaded } = useAddons();
  const catalogs = useMemo(() => browsableCatalogs(addons), [addons]);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [genre, setGenre] = useState<string>("");
  const [items, setItems] = useState<MetaPreview[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [exhausted, setExhausted] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selected = catalogs.find((c) => c.key === selectedKey) ?? catalogs[0];
  // bumped on every catalog/genre switch so in-flight "load more" responses are dropped
  const epoch = useRef(0);

  const load = useCallback(
    async (skip: number, currentGenre: string) => {
      if (!selected) return;
      const extra: { name: string; value: string }[] = [];
      if (currentGenre) extra.push({ name: "genre", value: currentGenre });
      if (skip > 0) extra.push({ name: "skip", value: String(skip) });
      const response = await catalogLoad({
        transportUrl: selected.addon.transportUrl,
        type: selected.catalog.type,
        id: selected.catalog.id,
        extra,
      });
      return response.metas;
    },
    [selected],
  );

  useEffect(() => {
    if (!selected) return;
    epoch.current += 1;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setItems([]);
    setExhausted(false);
    load(0, genre)
      .then((metas) => {
        if (cancelled || !metas) return;
        setItems(metas);
        if (metas.length === 0) setExhausted(true);
      })
      .catch((e) => !cancelled && setError(errorMessage(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected?.key, genre]);

  if (!loaded) return <div className="p-8"><Spinner label="Loading…" /></div>;

  if (catalogs.length === 0) {
    return (
      <EmptyState title="No catalogs yet">
        Install an addon that provides catalogs to start browsing. Head to the Addons page and
        paste an addon manifest URL.
      </EmptyState>
    );
  }

  const genres = selected ? genreOptions(selected.catalog) : [];
  const canPage = selected ? supportsExtra(selected.catalog, "skip") : false;

  return (
    <div className="flex flex-col gap-5 p-8">
      <div className="flex flex-wrap items-center gap-3">
        <select
          className="rounded-lg border border-rig bg-hull px-3 py-2 text-sm"
          value={selected?.key ?? ""}
          onChange={(e) => {
            setGenre("");
            setSelectedKey(e.target.value);
          }}
        >
          {catalogs.map((c) => (
            <option key={c.key} value={c.key}>
              {c.addon.manifest.name} — {catalogDisplayName(c.catalog)} ({c.catalog.type})
            </option>
          ))}
        </select>
        {genres.length > 0 && (
          <select
            className="rounded-lg border border-rig bg-hull px-3 py-2 text-sm"
            value={genre}
            onChange={(e) => setGenre(e.target.value)}
          >
            <option value="">All genres</option>
            {genres.map((g) => (
              <option key={g} value={g}>
                {g}
              </option>
            ))}
          </select>
        )}
      </div>

      {error && <ErrorNote message={error} />}
      {loading && <Spinner label="Loading catalog…" />}

      <div className="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-4">
        {items.map((meta, i) => (
          <PosterCard
            key={`${meta.id}-${i}`}
            meta={meta}
            onClick={() =>
              navigate(`/detail/${encodeURIComponent(meta.type)}/${encodeURIComponent(meta.id)}`)
            }
          />
        ))}
      </div>

      {!loading && items.length > 0 && canPage && !exhausted && (
        <button
          className="mx-auto rounded-lg bg-hull px-5 py-2 text-sm font-medium text-foam hover:bg-rig disabled:opacity-50"
          disabled={loadingMore}
          onClick={async () => {
            const requestEpoch = epoch.current;
            setLoadingMore(true);
            try {
              const more = await load(items.length, genre);
              if (epoch.current !== requestEpoch) return; // catalog/genre changed meanwhile
              if (!more || more.length === 0) setExhausted(true);
              else setItems((prev) => [...prev, ...more]);
            } catch (e) {
              if (epoch.current === requestEpoch) setError(errorMessage(e));
            } finally {
              setLoadingMore(false);
            }
          }}
        >
          {loadingMore ? "Loading…" : "Load more"}
        </button>
      )}
    </div>
  );
}
