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

export type Room = "home" | "movies" | "shows" | "anime" | "live";

/** Types the Live room shows: TV channels and channel catalogs from the user's addons. */
export const LIVE_TYPES = ["tv", "channel"];

/** The types a room's catalogs come from (anime also takes anime catalogs filed under
 * other types). With the Live room hidden, `tv` goes back to Shows. */
export function roomTypes(room: Exclude<Room, "home">, liveRoom: boolean): string[] {
  switch (room) {
    case "movies":
      return ["movie"];
    case "shows":
      return liveRoom ? ["series"] : ["series", "tv"];
    case "anime":
      return ["anime"];
    case "live":
      return LIVE_TYPES;
  }
}

type RoomSettings = Pick<Settings, "hideAnime" | "showAdult" | "hideLiveTv"> | null;

/** The catalogs a room shows as rows, each with its default extras (Cinemeta's "New"
 * becomes "New · 2026"). */
export function roomCatalogs(addons: Descriptor[], room: Room, settings: RoomSettings): CatalogRef[] {
  const all = boardableCatalogs(addons, settings?.showAdult ?? false);
  const liveRoom = !(settings?.hideLiveTv ?? false);
  switch (room) {
    case "movies":
    case "shows":
      return all.filter(
        (r) => roomTypes(room, liveRoom).includes(r.catalog.type) && !isAnimeCatalog(r),
      );
    // a channel stays a channel, even an anime one
    case "live":
      return all.filter((r) => LIVE_TYPES.includes(r.catalog.type));
    case "anime":
      return all.filter(
        (r) => isAnimeCatalog(r) && !(liveRoom && LIVE_TYPES.includes(r.catalog.type)),
      );
    case "home": {
      const hideAnime = settings?.hideAnime ?? false;
      // channels have their own room unless it is hidden (Harbor's home.tsx rule)
      const shown = all.filter(
        (r) =>
          !(hideAnime && isAnimeCatalog(r)) && !(liveRoom && LIVE_TYPES.includes(r.catalog.type)),
      );
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

const PREFERRED_GENRES: Record<Exclude<Room, "home" | "live">, string[]> = {
  movies: ["Action", "Comedy", "Drama", "Sci-Fi", "Thriller", "Animation", "Horror", "Documentary"],
  shows: ["Drama", "Comedy", "Crime", "Sci-Fi", "Mystery", "Animation", "Documentary", "Reality-TV"],
  anime: ["Action", "Romance", "Comedy", "Fantasy", "Slice of Life", "Sci-Fi", "Mystery", "Sports"],
};

/** Genre rows for a room, from its first catalog with an optional genre filter: the
 * preferred genres it declares (else its first ones), one row each. Manifest-driven:
 * a genre no addon declares never gets a row. */
export function spotlightCatalogs(
  roomRefs: CatalogRef[],
  room: Exclude<Room, "home" | "live">,
): CatalogRef[] {
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
