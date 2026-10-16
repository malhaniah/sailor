import { useEffect, useRef } from "react";
import { type CatalogRef, catalogTitle } from "../lib/catalogs";
import { type RowState, dedupRows } from "../lib/rows";
import type { MetaPreview } from "../lib/types";
import { PickCard } from "./cards";
import { Row } from "./Row";

/**
 * Catalog rows in `refs` order. A row that hasn't loaded yet is a placeholder that asks
 * for its first page when it scrolls near; a loaded row too thin to show (or one `keep`
 * turns away) disappears, and the placeholders below move up into view.
 */
export function CatalogRows<R extends CatalogRef>({
  refs,
  rows,
  request,
  loadMore,
  version,
  reserved,
  onOpen,
  title = catalogTitle,
  kicker = (ref) => ref.addon.manifest.name,
  keep,
  dedup = true,
}: {
  refs: R[];
  rows: Map<string, RowState>;
  request: (key: string) => void;
  loadMore: (key: string) => void;
  /** from useCatalogRows: placeholders re-arm when it changes */
  version: number;
  /** ids shown above (hero, Top 10): left out of every row */
  reserved: Set<string>;
  onOpen: (meta: MetaPreview) => void;
  title?: (ref: R) => string;
  kicker?: (ref: R) => string;
  /** whether a loaded row belongs here (e.g. not mostly anime in Movies) */
  keep?: (state: RowState) => boolean;
  /** leave titles out of a row when an earlier row shows them (not for channel
   * line-ups, whose categories overlap by design) */
  dedup?: boolean;
}) {
  const loaded = refs
    .map((ref) => ({ key: ref.key, state: rows.get(ref.key) }))
    .filter((r): r is { key: string; state: RowState } => !!r.state)
    .filter((r) => !keep || keep(r.state));
  const shown = new Map(
    dedup
      ? dedupRows(loaded, reserved).map((r) => [r.key, r.items])
      : loaded.filter((r) => r.state.items.length > 0).map((r) => [r.key, r.state.items]),
  );

  return (
    <>
      {refs.map((ref) => {
        if (!rows.has(ref.key)) {
          return (
            <RowPlaceholder key={`${ref.key}#${version}`} onNear={() => request(ref.key)} />
          );
        }
        const items = shown.get(ref.key);
        if (!items) return null;
        return (
          <Row
            key={ref.key}
            title={title(ref)}
            kicker={kicker(ref)}
            // channel logos and stills are wide: fewer, larger cells
            min={items[0]?.posterShape === "landscape" ? 240 : 148}
            onEndReached={() => loadMore(ref.key)}
          >
            {items.map((meta, i) => (
              <PickCard key={`${meta.id}-${i}`} meta={meta} onClick={() => onOpen(meta)} />
            ))}
          </Row>
        );
      })}
    </>
  );
}

/** A row's skeleton; asks for the row once it is within a screen of the viewport. */
export function RowPlaceholder({ onNear }: { onNear?: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const onNearRef = useRef(onNear);
  onNearRef.current = onNear;
  useEffect(() => {
    const el = ref.current;
    if (!el || !onNearRef.current) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          observer.disconnect();
          onNearRef.current?.();
        }
      },
      // the margin only reaches past the edge of the scrolling element when it is the
      // root (the viewport would clip at that element's edge)
      { root: scrollParent(el), rootMargin: "600px 0px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <div ref={ref} className="flex flex-col gap-5 pl-[9px]" aria-hidden>
      <div className="h-5 w-44 rounded bg-elevated/30" />
      <div className="flex gap-5 overflow-hidden">
        {Array.from({ length: 8 }, (_, j) => (
          <div key={j} className="w-[148px] shrink-0">
            <div className="aspect-[2/3] rounded-xl bg-elevated/40" />
            <div className="mt-2 h-3 w-3/5 rounded bg-elevated/35" />
          </div>
        ))}
      </div>
    </div>
  );
}

/** The nearest ancestor that scrolls vertically (null: the viewport). */
export function scrollParent(el: Element): Element | null {
  for (let node = el.parentElement; node; node = node.parentElement) {
    const overflow = getComputedStyle(node).overflowY;
    if (overflow === "auto" || overflow === "scroll") return node;
  }
  return null;
}
