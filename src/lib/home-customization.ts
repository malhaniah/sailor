// Home row order, hiding and renaming. Adapted from Harbor's
// src/lib/home-customization.ts (Copyright (c) 2026 Harbor, MIT; see
// THIRD-PARTY-NOTICES.md). Zeiler stores renames as a list, so a settings patch
// (which merges objects) can also remove one.
import type { HomeRows } from "./types";

export interface CustomRow {
  key: string;
  name: string;
}

export const EMPTY_HOME_ROWS: HomeRows = { order: [], hidden: [], renamed: [] };

export function rowName(custom: HomeRows, key: string, fallback: string): string {
  return custom.renamed.find((r) => r.key === key)?.name ?? fallback;
}

export function isRowHidden(custom: HomeRows, key: string): boolean {
  return custom.hidden.includes(key);
}

/** The rows' keys in the user's order: the ones they placed, then new rows in their
 * natural order. Keys of rows that no longer exist are dropped. */
export function effectiveOrder(keys: string[], custom: HomeRows): string[] {
  const live = new Set(keys);
  const placed = custom.order.filter((k) => live.has(k));
  const seen = new Set(placed);
  return [...placed, ...keys.filter((k) => !seen.has(k))];
}

/** `rows` in the user's order, renamed, without the hidden ones unless asked. */
export function applyHomeRowCustomization<T extends CustomRow>(
  rows: T[],
  custom: HomeRows,
  includeHidden = false,
): T[] {
  const byKey = new Map(rows.map((r) => [r.key, r]));
  return effectiveOrder(
    rows.map((r) => r.key),
    custom,
  )
    .filter((k) => includeHidden || !isRowHidden(custom, k))
    .map((k) => {
      const row = byKey.get(k)!;
      return { ...row, name: rowName(custom, k, row.name) };
    });
}

export function moveRow(custom: HomeRows, keys: string[], key: string, delta: -1 | 1): HomeRows {
  const order = effectiveOrder(keys, custom);
  const i = order.indexOf(key);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= order.length) return custom;
  const next = order.slice();
  [next[i], next[j]] = [next[j], next[i]];
  return { ...custom, order: next };
}

export function toggleRowHidden(custom: HomeRows, key: string): HomeRows {
  const hidden = isRowHidden(custom, key)
    ? custom.hidden.filter((k) => k !== key)
    : [...custom.hidden, key];
  return { ...custom, hidden };
}

/** An empty name (or the row's own) removes the rename. */
export function renameRow(custom: HomeRows, key: string, name: string, original: string): HomeRows {
  const trimmed = name.trim().slice(0, 80);
  const renamed = custom.renamed.filter((r) => r.key !== key);
  if (trimmed && trimmed !== original) renamed.push({ key, name: trimmed });
  return { ...custom, renamed };
}

export function resetHomeRows(): HomeRows {
  return EMPTY_HOME_ROWS;
}
