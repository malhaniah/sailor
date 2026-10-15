import { useState } from "react";
import { navigate, useAddons } from "../App";
import { PosterCard } from "../components/PosterCard";
import { EmptyState, ErrorNote, Spinner } from "../components/ui";
import { catalogDisplayName, searchableCatalogs } from "../lib/catalogs";
import { catalogLoad, errorMessage } from "../lib/ipc";
import type { MetaPreview } from "../lib/types";

interface SectionResult {
  key: string;
  title: string;
  metas: MetaPreview[];
  error?: string;
}

export function SearchPage() {
  const { addons } = useAddons();
  const targets = searchableCatalogs(addons);
  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const [results, setResults] = useState<SectionResult[] | null>(null);

  async function run() {
    const q = query.trim();
    if (!q || targets.length === 0) return;
    setSearching(true);
    setResults(null);
    const sections = await Promise.all(
      targets.map(async (t): Promise<SectionResult> => {
        const title = `${t.addon.manifest.name} — ${catalogDisplayName(t.catalog)} (${t.catalog.type})`;
        try {
          const response = await catalogLoad({
            transportUrl: t.addon.transportUrl,
            type: t.catalog.type,
            id: t.catalog.id,
            extra: [{ name: "search", value: q }],
          });
          return { key: t.key, title, metas: response.metas };
        } catch (e) {
          return { key: t.key, title, metas: [], error: errorMessage(e) };
        }
      }),
    );
    setResults(sections);
    setSearching(false);
  }

  return (
    <div className="flex flex-col gap-6 p-8">
      <form
        className="flex gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search movies, shows…"
          className="w-full max-w-xl rounded-lg border border-rig bg-hull px-4 py-2.5 text-sm placeholder:text-mist/60 focus:border-buoy focus:outline-none"
        />
        <button
          type="submit"
          disabled={searching || !query.trim()}
          className="rounded-lg bg-buoy px-5 py-2.5 text-sm font-semibold text-abyss hover:bg-buoy/80 disabled:opacity-50"
        >
          Search
        </button>
      </form>

      {targets.length === 0 && (
        <EmptyState title="Nothing to search">
          None of your installed addons declares a searchable catalog.
        </EmptyState>
      )}

      {searching && <Spinner label="Searching all addons…" />}

      {results?.map((section) =>
        section.metas.length === 0 && !section.error ? null : (
          <section key={section.key} className="flex flex-col gap-3">
            <h2 className="text-sm font-semibold uppercase tracking-wider text-mist">
              {section.title}
            </h2>
            {section.error && <ErrorNote message={section.error} />}
            <div className="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-4">
              {section.metas.map((meta, i) => (
                <PosterCard
                  key={`${meta.id}-${i}`}
                  meta={meta}
                  onClick={() =>
                    navigate(
                      `/detail/${encodeURIComponent(meta.type)}/${encodeURIComponent(meta.id)}`,
                    )
                  }
                />
              ))}
            </div>
          </section>
        ),
      )}

      {results && results.every((s) => s.metas.length === 0) && (
        <EmptyState title="No results">Try a different query.</EmptyState>
      )}
    </div>
  );
}
