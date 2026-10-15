import { useCallback, useEffect, useRef, useState } from "react";
import { type CatalogRef, supportsExtra } from "./catalogs";
import { catalogLoad } from "./ipc";
import type { MetaPreview } from "./types";

export interface RowState {
  ref: CatalogRef;
  items: MetaPreview[];
  exhausted: boolean;
}

const ROW_CAP = 60;

/** Load the first page of every catalog and expose per-row skip-paging. */
export function useCatalogRows(catalogs: CatalogRef[]): {
  rows: Map<string, RowState>;
  loadMore: (key: string) => void;
} {
  const [rows, setRows] = useState<Map<string, RowState>>(new Map());
  const pending = useRef(new Set<string>());
  const keyList = catalogs.map((c) => c.key).join("|");

  useEffect(() => {
    let cancelled = false;
    setRows(new Map());
    pending.current.clear();
    for (const ref of catalogs) {
      void catalogLoad({
        transportUrl: ref.addon.transportUrl,
        type: ref.catalog.type,
        id: ref.catalog.id,
      })
        .then((response) => {
          if (cancelled) return;
          setRows((prev) => {
            const next = new Map(prev);
            next.set(ref.key, {
              ref,
              items: response.metas,
              exhausted: response.metas.length === 0 || !supportsExtra(ref.catalog, "skip"),
            });
            return next;
          });
        })
        .catch(() => {
          if (cancelled) return;
          setRows((prev) => {
            const next = new Map(prev);
            next.set(ref.key, { ref, items: [], exhausted: true });
            return next;
          });
        });
    }
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keyList]);

  const loadMore = useCallback(
    (key: string) => {
      const row = rows.get(key);
      if (!row || row.exhausted || row.items.length >= ROW_CAP) return;
      if (pending.current.has(key)) return;
      pending.current.add(key);
      void catalogLoad({
        transportUrl: row.ref.addon.transportUrl,
        type: row.ref.catalog.type,
        id: row.ref.catalog.id,
        extra: [{ name: "skip", value: String(row.items.length) }],
      })
        .then((response) => {
          setRows((prev) => {
            const current = prev.get(key);
            if (!current) return prev;
            const seen = new Set(current.items.map((m) => m.id));
            const fresh = response.metas.filter((m) => !seen.has(m.id));
            const next = new Map(prev);
            next.set(key, {
              ...current,
              items: [...current.items, ...fresh],
              exhausted: fresh.length === 0 || current.items.length + fresh.length >= ROW_CAP,
            });
            return next;
          });
        })
        .catch(() => {
          setRows((prev) => {
            const current = prev.get(key);
            if (!current) return prev;
            const next = new Map(prev);
            next.set(key, { ...current, exhausted: true });
            return next;
          });
        })
        .finally(() => pending.current.delete(key));
    },
    [rows],
  );

  return { rows, loadMore };
}

/** Render-time cross-row dedup: hero/top-10 ids never reappear in later rows. */
export function dedupRows(
  ordered: { key: string; state: RowState }[],
  reservedIds: Set<string>,
): { key: string; state: RowState; items: MetaPreview[] }[] {
  const seen = new Set(reservedIds);
  const result: { key: string; state: RowState; items: MetaPreview[] }[] = [];
  for (const { key, state } of ordered) {
    const items = state.items.filter((m) => !seen.has(m.id));
    for (const item of items.slice(0, 20)) seen.add(item.id);
    if (items.length >= 4 || (items.length > 0 && state.items.length < 4)) {
      result.push({ key, state, items });
    }
  }
  return result;
}
