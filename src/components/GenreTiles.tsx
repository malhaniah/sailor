import { useMemo } from "react";
import { type CatalogRef, boardableCatalogs, filterProps, typeLabel } from "../lib/catalogs";
import { useAddons, useSettings } from "../lib/store";
import { useView } from "../lib/view";
import { Row } from "./Row";

// Hues for genres Harbor gives a tile (src/components/genre-tiles.tsx, MIT; see
// THIRD-PARTY-NOTICES.md), mapped through one oklch ramp. Any other genre an addon
// declares gets a hue hashed from its name.
const HUES: Record<string, number> = {
  Action: 25,
  Adventure: 60,
  Thriller: 280,
  Crime: 335,
  Drama: 250,
  Romance: 350,
  Mystery: 300,
  "Sci-Fi": 200,
  Fantasy: 160,
  Horror: 15,
  Comedy: 85,
  Family: 120,
  Animation: 190,
  Western: 45,
  War: 100,
  History: 70,
  Documentary: 220,
  Music: 320,
};

function hueOf(genre: string): number {
  if (genre in HUES) return HUES[genre];
  let hash = 0;
  for (let i = 0; i < genre.length; i++) hash = (hash * 31 + genre.charCodeAt(i)) >>> 0;
  return hash % 360;
}

/** The catalog a type's genre tiles open: the first one of that type whose genre
 * filter is optional and not a list of years. */
export function genreSource(refs: CatalogRef[], type: string): CatalogRef | null {
  return (
    refs.find(
      (r) =>
        r.catalog.type === type &&
        filterProps(r.catalog).some(
          (p) => p.name === "genre" && !p.isRequired && !p.options.every((o) => /^\d{4}$/.test(o)),
        ),
    ) ?? null
  );
}

/** One tile per genre the type's first genre catalog declares; nothing when none does. */
export function GenreTiles({
  type,
  title,
  refs,
}: {
  type: string;
  title?: string;
  /** the catalogs to take genres from (a room's own); every boardable one by default */
  refs?: CatalogRef[];
}) {
  const { addons } = useAddons();
  const { settings } = useSettings();
  const { openBrowse } = useView();
  const source = useMemo(
    () => genreSource(refs ?? boardableCatalogs(addons, settings?.showAdult ?? false), type),
    [refs, addons, settings?.showAdult, type],
  );
  const genres = source ? (filterProps(source.catalog).find((p) => p.name === "genre")?.options ?? []) : [];
  if (!source || genres.length === 0) return null;

  return (
    <Row title={title ?? `${typeLabel(type)} by genre`} min={210}>
      {genres.map((name) => {
        const hue = hueOf(name);
        return (
          <button
            key={name}
            onClick={() =>
              openBrowse({
                type,
                transportUrl: source.addon.transportUrl,
                catalogId: source.catalog.id,
                extra: [
                  ...source.extra.filter((e) => e.name !== "genre"),
                  { name: "genre", value: name },
                ],
              })
            }
            className="group relative aspect-[5/4] w-full cursor-pointer overflow-hidden rounded-2xl border border-edge-soft text-left transition-transform duration-300 ease-[cubic-bezier(0.32,0.72,0.24,1)] hover:-translate-y-1"
            style={{
              background: `linear-gradient(150deg, oklch(0.40 0.18 ${hue}), oklch(0.18 0.10 ${hue}))`,
            }}
          >
            <div
              className="absolute inset-x-0 bottom-0 h-2/3"
              style={{
                background: `linear-gradient(to top, oklch(0.14 0.08 ${hue}), transparent)`,
              }}
            />
            <div className="absolute inset-x-5 bottom-5 flex items-end justify-between gap-2">
              <span
                className="font-display text-[24px] font-medium leading-tight drop-shadow-[0_2px_18px_rgba(0,0,0,0.4)]"
                style={{ color: `oklch(0.92 0.06 ${hue})` }}
              >
                {name}
              </span>
              <span className="text-[18px] text-ink/70 transition-transform duration-300 group-hover:translate-x-1">
                ›
              </span>
            </div>
          </button>
        );
      })}
    </Row>
  );
}
