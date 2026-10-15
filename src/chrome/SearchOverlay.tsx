import { Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { PickCard } from "../components/cards";
import { Spinner } from "../components/ui";
import { catalogDisplayName, searchableCatalogs } from "../lib/catalogs";
import { catalogLoad } from "../lib/ipc";
import { useAddons, useSearch } from "../lib/store";
import { useView } from "../lib/view";
import type { MetaPreview } from "../lib/types";

interface SectionResult {
  key: string;
  title: string;
  metas: MetaPreview[];
}

export function SearchOverlay() {
  const { open, setOpen } = useSearch();
  const { addons } = useAddons();
  const { openMeta } = useView();
  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const [results, setResults] = useState<SectionResult[] | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const requestId = useRef(0);

  useEffect(() => {
    if (open) {
      setTimeout(() => inputRef.current?.focus(), 30);
    } else {
      setQuery("");
      setResults(null);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, setOpen]);

  // debounce search 450ms
  useEffect(() => {
    if (!open) return;
    const q = query.trim();
    if (q.length < 2) {
      setResults(null);
      setSearching(false);
      return;
    }
    const id = ++requestId.current;
    setSearching(true);
    const timer = setTimeout(async () => {
      const targets = searchableCatalogs(addons);
      const sections = await Promise.all(
        targets.map(async (t): Promise<SectionResult> => {
          try {
            const response = await catalogLoad({
              transportUrl: t.addon.transportUrl,
              type: t.catalog.type,
              id: t.catalog.id,
              extra: [{ name: "search", value: q }],
            });
            return {
              key: t.key,
              title: `${catalogDisplayName(t.catalog)} · ${t.catalog.type} — ${t.addon.manifest.name}`,
              metas: response.metas,
            };
          } catch {
            return { key: t.key, title: "", metas: [] };
          }
        }),
      );
      if (requestId.current === id) {
        setResults(sections.filter((s) => s.metas.length > 0));
        setSearching(false);
      }
    }, 450);
    return () => clearTimeout(timer);
  }, [query, open, addons]);

  if (!open) return null;

  return (
    <div className="animate-fade-in fixed inset-0 z-[200] flex flex-col bg-canvas/95 backdrop-blur-xl">
      <div className="mx-auto mt-16 flex w-full max-w-3xl items-center gap-4 px-8">
        <div className="flex h-14 flex-1 items-center gap-4 rounded-full border border-edge bg-elevated px-6">
          <Search size={18} className="shrink-0 text-ink-subtle" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search movies, shows…"
            className="h-full flex-1 bg-transparent text-[16px] text-ink outline-none placeholder:text-ink-subtle"
          />
          {searching && (
            <div className="h-4 w-4 animate-spin rounded-full border-2 border-edge border-t-accent" />
          )}
        </div>
        <button
          onClick={() => setOpen(false)}
          className="flex h-12 w-12 items-center justify-center rounded-full border border-edge-soft bg-elevated/70 text-ink-muted hover:bg-elevated hover:text-ink"
          aria-label="Close search"
        >
          <X size={18} />
        </button>
      </div>

      <div className="mt-10 flex-1 overflow-y-auto px-12 pb-16">
        <div className="mx-auto flex max-w-6xl flex-col gap-10">
          {query.trim().length < 2 && (
            <p className="text-center text-[14px] text-ink-subtle">
              Type at least two characters. Results come from every installed addon with a
              searchable catalog.
            </p>
          )}
          {searching && results === null && query.trim().length >= 2 && (
            <div className="flex justify-center py-10">
              <Spinner label="Searching all addons…" />
            </div>
          )}
          {results !== null && results.length === 0 && !searching && (
            <p className="text-center text-[14px] text-ink-subtle">No results.</p>
          )}
          {results?.map((section) => (
            <section key={section.key} className="flex flex-col gap-4">
              <h2 className="text-[12px] font-semibold uppercase tracking-[0.18em] text-ink-subtle">
                {section.title}
              </h2>
              <div className="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-5">
                {section.metas.slice(0, 14).map((meta, i) => (
                  <PickCard
                    key={`${meta.id}-${i}`}
                    meta={meta}
                    onClick={() => {
                      setOpen(false);
                      openMeta(meta);
                    }}
                  />
                ))}
              </div>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}
