import type { Descriptor, ExtraProp, ManifestCatalog } from "./types";

export interface ExtraValue {
  name: string;
  value: string;
}

/// Normalized extra-property declarations (modern `extra` wins over the legacy form) —
/// mirrors `ManifestCatalog::extra_props` in Rust.
export function extraProps(catalog: ManifestCatalog): ExtraProp[] {
  if (catalog.extra) return catalog.extra;
  const supported = catalog.extraSupported ?? [];
  const required = catalog.extraRequired ?? [];
  return supported.map((name) => ({
    name,
    isRequired: required.includes(name),
    options: [],
    optionsLimit: 1,
  }));
}

export function supportsExtra(catalog: ManifestCatalog, name: string): boolean {
  return extraProps(catalog).some((p) => p.name === name);
}

/** The extras that load this catalog with no input: each required prop's first option.
 * null when a required prop has nothing to pick from (search, id lists). Mirrors
 * `ManifestCatalog::default_extra` in Rust. */
export function defaultExtra(catalog: ManifestCatalog): ExtraValue[] | null {
  const out: ExtraValue[] = [];
  for (const prop of extraProps(catalog)) {
    if (!prop.isRequired) continue;
    const first = prop.options?.[0];
    if (first === undefined) return null;
    out.push({ name: prop.name, value: first });
  }
  return out;
}

/** Can be a row (or a Browse source) without user input. */
export function isBoardable(catalog: ManifestCatalog): boolean {
  return defaultExtra(catalog) !== null;
}

/** The props a filter UI offers: those with options, except paging and free text. */
export function filterProps(catalog: ManifestCatalog): ExtraProp[] {
  return extraProps(catalog).filter(
    (p) => (p.options?.length ?? 0) > 0 && p.name !== "search" && p.name !== "skip",
  );
}

export function genreOptions(catalog: ManifestCatalog): string[] {
  return extraProps(catalog).find((p) => p.name === "genre")?.options ?? [];
}

/** A filter's label: a "genre" whose options are all years reads "Year" (Cinemeta's
 * "New"); any other name is title-cased ("sort" → "Sort"). */
export function extraLabel(prop: ExtraProp): string {
  if (prop.name === "genre") {
    const years = prop.options.length > 0 && prop.options.every((o) => /^\d{4}$/.test(o));
    return years ? "Year" : "Genre";
  }
  return titleCase(prop.name);
}

function titleCase(text: string): string {
  const spaced = text.replace(/[_-]+/g, " ").replace(/([a-z])([A-Z])/g, "$1 $2");
  return spaced.slice(0, 1).toUpperCase() + spaced.slice(1);
}

export function catalogDisplayName(catalog: ManifestCatalog): string {
  return catalog.name ?? catalog.id;
}

/** A row's title: the catalog's name, plus the value of each required extra it was
 * loaded with ("New · 2026"). */
export function catalogTitle(ref: Pick<CatalogRef, "catalog" | "extra">): string {
  const required = new Set(
    extraProps(ref.catalog)
      .filter((p) => p.isRequired)
      .map((p) => p.name),
  );
  const values = ref.extra.filter((e) => required.has(e.name)).map((e) => e.value);
  return [catalogDisplayName(ref.catalog), ...values].join(" · ");
}

// ---- types --------------------------------------------------------------------------

const TYPE_ORDER = ["movie", "series", "anime", "channel", "tv"];

const TYPE_LABEL: Record<string, string> = {
  movie: "Movies",
  series: "Series",
  anime: "Anime",
  channel: "Channels",
  tv: "Live TV",
  other: "Other",
};

export function typeLabel(type: string): string {
  return TYPE_LABEL[type] ?? titleCase(type);
}

/** Every type the addons have boardable catalogs for: movie, series, anime, channel,
 * tv, then the rest alphabetically. */
export function catalogTypes(refs: CatalogRef[]): string[] {
  const types = [...new Set(refs.map((r) => r.catalog.type))];
  const rank = (t: string) => {
    const i = TYPE_ORDER.indexOf(t);
    return i < 0 ? TYPE_ORDER.length : i;
  };
  return types.sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}

// ---- catalog references -------------------------------------------------------------

export interface CatalogRef {
  addon: Descriptor;
  catalog: ManifestCatalog;
  /** sent with every request for this row (paging adds `skip`) */
  extra: ExtraValue[];
  /** stable and safe to persist: see `catalogKey` */
  key: string;
}

/** FNV-1a, 32-bit, hex: a short non-reversible tag for a transport URL. */
function shortHash(text: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, "0");
}

/** A row's key: the manifest id, a hash of the transport URL (two configured installs
 * of one addon differ), type, catalog id and extras. Never the URL itself, which can
 * carry an addon's configuration: keys are stored in settings (`homeRows`). */
export function catalogKey(
  addon: Descriptor,
  catalog: ManifestCatalog,
  extra: ExtraValue[],
): string {
  const extras = extra.map((e) => `${e.name}=${e.value}`).join("&");
  return [addon.manifest.id, shortHash(addon.transportUrl), catalog.type, catalog.id, extras].join(
    "|",
  );
}

export function catalogRef(
  addon: Descriptor,
  catalog: ManifestCatalog,
  extra: ExtraValue[],
): CatalogRef {
  return { addon, catalog, extra, key: catalogKey(addon, catalog, extra) };
}

/** Whether an addon's catalogs may show: an addon flagged adult only with `showAdult`.
 * (Adult genre options and catalog names never reach here while it is off: Rust takes
 * them out of `addon_list`.) */
export function addonVisible(addon: Descriptor, showAdult: boolean): boolean {
  return showAdult || !addon.manifest.behaviorHints.adult;
}

/** Every catalog that can be a row, loaded with its default extras, in addon order. */
export function boardableCatalogs(addons: Descriptor[], showAdult: boolean): CatalogRef[] {
  return addons
    .filter((addon) => addonVisible(addon, showAdult))
    .flatMap((addon) =>
      addon.manifest.catalogs.flatMap((catalog) => {
        const extra = defaultExtra(catalog);
        // keyed without the defaults: "New" keeps its place on Home when its first
        // option moves on to the next year
        return extra ? [{ ...catalogRef(addon, catalog, extra), key: catalogKey(addon, catalog, []) }] : [];
      }),
    );
}

export function searchableCatalogs(addons: Descriptor[], showAdult: boolean): CatalogRef[] {
  // A catalog is searchable only if it declares `search` AND has no other required
  // extra we cannot supply (reference is_extra_supported semantics).
  return addons
    .filter((addon) => addonVisible(addon, showAdult))
    .flatMap((addon) =>
      addon.manifest.catalogs
        .filter(
          (catalog) =>
            supportsExtra(catalog, "search") &&
            extraProps(catalog).every((p) => !p.isRequired || p.name === "search"),
        )
        .map((catalog) => catalogRef(addon, catalog, [])),
    );
}

// ---- anime --------------------------------------------------------------------------

const ANIME_PREFIXES = ["kitsu", "mal", "anilist", "anidb"];
const ANIME_NAME = /anime|\bmal\b|anilist|kitsu|aniworld|crunchyroll|funimation/i;

/** An anime catalog filed under another type: its addon serves only anime ids, or its
 * name says so (Harbor's isAnimeRow, src/views/anime.tsx; MIT, see
 * THIRD-PARTY-NOTICES.md). */
export function isAnimeCatalog(ref: CatalogRef): boolean {
  if (ref.catalog.type === "anime") return true;
  const prefixes = ref.addon.manifest.idPrefixes ?? [];
  if (
    prefixes.length > 0 &&
    prefixes.every((p) => ANIME_PREFIXES.some((a) => p.toLowerCase().startsWith(a)))
  ) {
    return true;
  }
  return ANIME_NAME.test(catalogDisplayName(ref.catalog));
}

/** A loaded row whose first titles are mostly anime ids. */
export function looksAnime(ids: string[]): boolean {
  const sample = ids.slice(0, 6);
  if (sample.length === 0) return false;
  const anime = sample.filter((id) => ANIME_PREFIXES.some((p) => id.startsWith(`${p}:`)));
  return anime.length * 2 >= sample.length;
}
