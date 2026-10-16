import { useCallback, useEffect, useRef, useState } from "react";
import { type CatalogRef, supportsExtra } from "./catalogs";
import { catalogLoad } from "./ipc";
import type { CatalogResponse, MetaPreview } from "./types";

export interface RowState {
  ref: CatalogRef;
  items: MetaPreview[];
  /** the next page's skip: titles the addon sent so far, adult ones included */
  offset: number;
  exhausted: boolean;
}

const ROW_CAP = 60;
/** A row stops asking after this many titles, even when Rust kept none of them. */
const OFFSET_CAP = 600;
/** Rows loaded as soon as a page mounts; the rest load when they scroll near. */
export const EAGER_ROWS = 4;
/** Catalog requests in flight at once, across every page… */
const MAX_IN_FLIGHT = 6;
/** …and to one addon, so a slow or dead one can't hold every slot. */
const MAX_PER_ADDON = 2;

/** Thrown for a queued request whose page was left before it could run. */
export class StaleRequest extends Error {}

interface Task {
  addon: string;
  stale: () => boolean;
  run: () => void;
  cancel: () => void;
}

let inFlight = 0;
const perAddon = new Map<string, number>();
const waiting: Task[] = [];

function pump() {
  for (let i = 0; i < waiting.length && inFlight < MAX_IN_FLIGHT; ) {
    const task = waiting[i];
    if (task.stale()) {
      waiting.splice(i, 1);
      task.cancel();
    } else if ((perAddon.get(task.addon) ?? 0) < MAX_PER_ADDON) {
      waiting.splice(i, 1);
      task.run();
    } else {
      i++;
    }
  }
}

/** Run `task` under the caps; a request that went stale while it waited is dropped
 * (rejected with StaleRequest) without being sent. */
function limited<T>(addon: string, stale: () => boolean, task: () => Promise<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    waiting.push({
      addon,
      stale,
      cancel: () => reject(new StaleRequest()),
      run: () => {
        inFlight++;
        perAddon.set(addon, (perAddon.get(addon) ?? 0) + 1);
        task()
          .then(resolve, reject)
          .finally(() => {
            inFlight--;
            perAddon.set(addon, (perAddon.get(addon) ?? 1) - 1);
            pump();
          });
      },
    });
    pump();
  });
}

/** One page of a catalog (with its extras) from `offset`, under the shared caps. */
export function loadPage(
  ref: CatalogRef,
  offset: number,
  stale: () => boolean = () => false,
): Promise<CatalogResponse> {
  const extra = offset > 0 ? [...ref.extra, { name: "skip", value: String(offset) }] : ref.extra;
  return limited(ref.addon.transportUrl, stale, () =>
    catalogLoad({
      transportUrl: ref.addon.transportUrl,
      type: ref.catalog.type,
      id: ref.catalog.id,
      extra,
    }),
  );
}

/** Whether paging is over after a page: the addon sent nothing, can't page, or sent
 * only titles already shown (it ignores skip). A page Rust emptied (all adult) is not
 * the end. */
export function pageEnds(
  ref: CatalogRef,
  response: CatalogResponse,
  fresh: number,
  offset: number,
): boolean {
  return (
    response.fetched === 0 ||
    !supportsExtra(ref.catalog, "skip") ||
    (response.metas.length > 0 && fresh === 0) ||
    offset >= OFFSET_CAP
  );
}

const addonTags = new WeakMap<object, number>();
let nextAddonTag = 0;

/** A number per addon object, so a list of rows can say which objects it came from. */
function addonTag(addon: object): number {
  let tag = addonTags.get(addon);
  if (tag === undefined) {
    tag = nextAddonTag++;
    addonTags.set(addon, tag);
  }
  return tag;
}

/**
 * Rows of catalogs, loaded lazily: the first `eager` at once, every other one when
 * `request(key)` is called (a placeholder scrolling into view). Requests share one
 * concurrency cap; pages are cached in Rust, so coming back to a room is quick.
 *
 * When the list changes, a row whose key and addon stay keeps what it loaded (and a
 * load in flight for it); every other row starts over. `version` changes then too, so
 * placeholders can re-arm.
 */
export function useCatalogRows(
  catalogs: CatalogRef[],
  eager = EAGER_ROWS,
): {
  rows: Map<string, RowState>;
  loadMore: (key: string) => void;
  request: (key: string) => void;
  version: number;
} {
  const [rows, setRows] = useState<Map<string, RowState>>(new Map());
  const [version, setVersion] = useState(0);
  // per row: its current load (a token and the ref it was for); an answer for another
  // token is dropped
  const loads = useRef(new Map<string, { token: object; ref: CatalogRef }>());
  // per row: the token of the load its `loadMore` is waiting on
  const pending = useRef(new Map<string, object>());
  const mounted = useRef(true);
  const latest = useRef(catalogs);
  latest.current = catalogs;
  // the rows and the addon objects they come from: a reload of the addons (after
  // showAdult changes, say) hands out new objects, and their rows start over
  const signature = catalogs.map((c) => `${c.key}@${addonTag(c.addon)}`).join("\n");

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const current = useCallback(
    (key: string, token: object) => mounted.current && loads.current.get(key)?.token === token,
    [],
  );

  const request = useCallback(
    (key: string) => {
      if (loads.current.has(key)) return;
      const ref = latest.current.find((c) => c.key === key);
      if (!ref) return;
      const token = {};
      loads.current.set(key, { token, ref });
      const put = (state: RowState) => {
        if (!current(key, token)) return;
        setRows((prev) => new Map(prev).set(key, state));
      };
      // the first page Rust left something on: a row whose first page was all adult
      // would otherwise stay hidden, with no Row to scroll for more
      const first = async (): Promise<RowState> => {
        let offset = 0;
        for (;;) {
          const response = await loadPage(ref, offset, () => !current(key, token));
          offset += response.fetched;
          const exhausted = pageEnds(ref, response, response.metas.length, offset);
          if (response.metas.length > 0 || exhausted) {
            return { ref, items: response.metas, offset, exhausted };
          }
        }
      };
      first()
        .then(put)
        .catch((e) => {
          if (!(e instanceof StaleRequest)) put({ ref, items: [], offset: 0, exhausted: true });
        });
    },
    [current],
  );

  useEffect(() => {
    const next = new Map(latest.current.map((r) => [r.key, r]));
    // a row whose key and addon stay keeps what it loaded, or its load in flight
    for (const [key, load] of [...loads.current]) {
      if (next.get(key)?.addon !== load.ref.addon) loads.current.delete(key);
    }
    setRows((prev) => {
      const kept = new Map<string, RowState>();
      for (const [key, state] of prev) {
        if (loads.current.get(key)?.ref === state.ref) kept.set(key, state);
      }
      return kept;
    });
    setVersion((v) => v + 1);
    for (const ref of latest.current.slice(0, eager)) request(ref.key);
  }, [signature, eager, request]);

  const loadMore = useCallback(
    (key: string) => {
      const row = rows.get(key);
      const token = loads.current.get(key)?.token;
      if (!row || !token || row.exhausted || row.items.length >= ROW_CAP) return;
      if (pending.current.get(key) === token) return;
      pending.current.set(key, token);
      const update = (change: (state: RowState) => RowState) => {
        if (!current(key, token)) return;
        setRows((prev) => {
          const state = prev.get(key);
          return state ? new Map(prev).set(key, change(state)) : prev;
        });
      };
      loadPage(row.ref, row.offset, () => !current(key, token))
        .then((response) =>
          update((state) => {
            const seen = new Set(state.items.map((m) => m.id));
            const fresh = response.metas.filter((m) => !seen.has(m.id));
            const items = [...state.items, ...fresh];
            const offset = state.offset + response.fetched;
            return {
              ...state,
              items,
              offset,
              exhausted:
                pageEnds(state.ref, response, fresh.length, offset) || items.length >= ROW_CAP,
            };
          }),
        )
        .catch((e) => {
          if (!(e instanceof StaleRequest)) update((state) => ({ ...state, exhausted: true }));
        })
        .finally(() => {
          if (pending.current.get(key) === token) pending.current.delete(key);
        });
    },
    [rows, current],
  );

  return { rows, loadMore, request, version };
}

/** Render-time cross-row dedup: hero/top-10 ids never reappear in later rows. Rows not
 * loaded yet are skipped; a loaded row too thin to show is left out. */
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

/** The row the hero and Top 10 come from: the first one that is not loaded yet, or
 * loaded with titles and kept (`keep`). */
export function leadRow<R extends CatalogRef>(
  catalogs: R[],
  rows: Map<string, RowState>,
  keep?: (state: RowState) => boolean,
): R | undefined {
  return (
    catalogs.find((c) => {
      const state = rows.get(c.key);
      return !state || (state.items.length > 0 && (!keep || keep(state)));
    }) ?? catalogs[0]
  );
}
