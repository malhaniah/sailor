// Which catalogs each room shows, and in what order. Merge rules follow Harbor's Home
// (src/views/home.tsx: anime rows go to their own room); MIT, see THIRD-PARTY-NOTICES.md.
import {
  type CatalogRef,
  boardableCatalogs,
  catalogRef,
  catalogTypes,
  extraProps,
  filterProps,
  isAnimeCatalog,
} from "./catalogs";
import type { Descriptor, Settings } from "./types";

export type Room = "home" | "movies" | "shows" | "anime";

/** The room for a type when it has one. */
export function roomForType(type: string): Exclude<Room, "home"> | null {
  switch (type) {
    case "movie":
      return "movies";
    case "series":
    case "tv":
      return "shows";
    case "anime":
      return "anime";
    default:
      return null;
  }
}

/** The types a room's catalogs come from (anime also takes anime catalogs filed under
 * other types). */
export const ROOM_TYPES: Record<Exclude<Room, "home">, string[]> = {
  movies: ["movie"],
  shows: ["series", "tv"],
  anime: ["anime"],
};

type RoomSettings = Pick<Settings, "hideAnime" | "showAdult"> | null;

/** The catalogs a room shows as rows, each with its default extras (Cinemeta's "New"
 * becomes "New · 2026"). */
export function roomCatalogs(addons: Descriptor[], room: Room, settings: RoomSettings): CatalogRef[] {
  const all = boardableCatalogs(addons, settings?.showAdult ?? false);
  switch (room) {
    case "movies":
    case "shows":
      return all.filter((r) => ROOM_TYPES[room].includes(r.catalog.type) && !isAnimeCatalog(r));
    case "anime":
      return all.filter(isAnimeCatalog);
    case "home": {
      const hideAnime = settings?.hideAnime ?? false;
      const shown = all.filter((r) => !(hideAnime && isAnimeCatalog(r)));
      const main = shown.filter(
        (r) => (r.catalog.type === "movie" || r.catalog.type === "series") && !isAnimeCatalog(r),
      );
      const anime = shown.filter(isAnimeCatalog);
      const rest = shown.filter((r) => !main.includes(r) && !anime.includes(r));
      // movies and series first, in addon order; then anime; then every other type
      const restOrdered = catalogTypes(rest).flatMap((t) => rest.filter((r) => r.catalog.type === t));
      return [...main, ...anime, ...restOrdered];
    }
  }
}

const SPOTLIGHT_MAX = 8;

const PREFERRED_GENRES: Record<Exclude<Room, "home">, string[]> = {
  movies: ["Action", "Comedy", "Drama", "Sci-Fi", "Thriller", "Animation", "Horror", "Documentary"],
  shows: ["Drama", "Comedy", "Crime", "Sci-Fi", "Mystery", "Animation", "Documentary", "Reality-TV"],
  anime: ["Action", "Romance", "Comedy", "Fantasy", "Slice of Life", "Sci-Fi", "Mystery", "Sports"],
};

/** Genre rows for a room, from its first catalog with an optional genre filter: the
 * preferred genres it declares (else its first ones), one row each. Manifest-driven:
 * a genre no addon declares never gets a row. */
export function spotlightCatalogs(roomRefs: CatalogRef[], room: Exclude<Room, "home">): CatalogRef[] {
  const source = roomRefs.find((r) =>
    filterProps(r.catalog).some((p) => p.name === "genre" && !p.isRequired),
  );
  if (!source) return [];
  const options = filterProps(source.catalog).find((p) => p.name === "genre")?.options ?? [];
  const preferred = PREFERRED_GENRES[room].filter((g) => options.includes(g));
  const genres = (preferred.length > 0 ? preferred : options).slice(0, SPOTLIGHT_MAX);
  const base = source.extra.filter((e) => e.name !== "genre");
  return genres.map((genre) =>
    catalogRef(source.addon, source.catalog, [...base, { name: "genre", value: genre }]),
  );
}

/** The genre a spotlight row picked (an optional genre extra), else null. */
export function spotlightGenre(ref: CatalogRef): string | null {
  const genre = ref.extra.find((e) => e.name === "genre");
  if (!genre) return null;
  const required = extraProps(ref.catalog).some((p) => p.name === "genre" && p.isRequired);
  return required ? null : genre.value;
}
