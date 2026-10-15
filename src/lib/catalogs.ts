import type { Descriptor, ExtraProp, ManifestCatalog } from "./types";

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

export function isBrowsable(catalog: ManifestCatalog): boolean {
  return extraProps(catalog).every((p) => !p.isRequired);
}

export function supportsExtra(catalog: ManifestCatalog, name: string): boolean {
  return extraProps(catalog).some((p) => p.name === name);
}

export function genreOptions(catalog: ManifestCatalog): string[] {
  return extraProps(catalog).find((p) => p.name === "genre")?.options ?? [];
}

export function catalogDisplayName(catalog: ManifestCatalog): string {
  return catalog.name ?? catalog.id;
}

export interface CatalogRef {
  addon: Descriptor;
  catalog: ManifestCatalog;
  key: string;
}

export function browsableCatalogs(addons: Descriptor[]): CatalogRef[] {
  return addons.flatMap((addon) =>
    addon.manifest.catalogs.filter(isBrowsable).map((catalog) => ({
      addon,
      catalog,
      key: `${addon.transportUrl}|${catalog.type}|${catalog.id}`,
    })),
  );
}

export function searchableCatalogs(addons: Descriptor[]): CatalogRef[] {
  // A catalog is searchable only if it declares `search` AND has no other required
  // extra we cannot supply (reference is_extra_supported semantics).
  return addons.flatMap((addon) =>
    addon.manifest.catalogs
      .filter(
        (catalog) =>
          supportsExtra(catalog, "search") &&
          extraProps(catalog).every((p) => !p.isRequired || p.name === "search"),
      )
      .map((catalog) => ({
        addon,
        catalog,
        key: `${addon.transportUrl}|${catalog.type}|${catalog.id}`,
      })),
  );
}
