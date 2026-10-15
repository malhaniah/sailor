import {
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";

const GAP = 20;

/**
 * Harbor's horizontal rail: cells sized so an exact number fit the container width,
 * scroll-snap, hidden scrollbars, hover edge-arrows, end-reached paging trigger.
 */
export function Row({
  title,
  kicker,
  min = 148,
  children,
  count,
  onEndReached,
}: {
  title?: ReactNode;
  kicker?: string;
  /** minimum cell width in px; actual width fills the row exactly */
  min?: number;
  /** cells */
  children: ReactNode[];
  count?: number;
  onEndReached?: () => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const trackRef = useRef<HTMLDivElement>(null);
  const [cellWidth, setCellWidth] = useState(min);
  const [canLeft, setCanLeft] = useState(false);
  const [canRight, setCanRight] = useState(false);

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const measure = () => {
      const width = el.clientWidth;
      if (width <= 0) return;
      const fits = Math.max(1, Math.floor((width + GAP) / (min + GAP)));
      setCellWidth((width - (fits - 1) * GAP) / fits);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [min]);

  const updateArrows = useCallback(() => {
    const track = trackRef.current;
    if (!track) return;
    setCanLeft(track.scrollLeft > 10);
    setCanRight(track.scrollLeft + track.clientWidth < track.scrollWidth - 10);
  }, []);

  useEffect(() => {
    updateArrows();
  }, [children.length, cellWidth, updateArrows]);

  const onScroll = useCallback(() => {
    updateArrows();
    const track = trackRef.current;
    if (track && onEndReached) {
      if (track.scrollWidth - (track.scrollLeft + track.clientWidth) < 800) onEndReached();
    }
  }, [updateArrows, onEndReached]);

  const scrollBy = (direction: 1 | -1) => {
    const track = trackRef.current;
    if (track) track.scrollBy({ left: direction * track.clientWidth, behavior: "smooth" });
  };

  if (children.length === 0) return null;

  return (
    <div className="flex min-w-0 flex-col gap-5 pl-[9px]">
      {(title || kicker) && (
        <div className="flex flex-col gap-0.5">
          {title && (
            <h3 className="truncate text-[17px] font-medium tracking-tight text-ink">
              {title}
              {count != null && (
                <span className="ml-2 text-[13px] font-normal text-ink-subtle">{count}</span>
              )}
            </h3>
          )}
          {kicker && (
            <p className="text-[12px] font-medium uppercase tracking-[0.18em] text-ink-subtle">
              {kicker}
            </p>
          )}
        </div>
      )}
      <div ref={containerRef} className="group/row relative min-w-0">
        <div
          ref={trackRef}
          onScroll={onScroll}
          className="no-scrollbar -m-5 grid grid-flow-col gap-5 overflow-x-auto scroll-pl-5 p-5 [scroll-snap-type:x_mandatory] [&>*]:[scroll-snap-align:start]"
          style={{ gridAutoColumns: `${cellWidth}px` }}
        >
          {children}
        </div>
        <EdgeArrow side="left" visible={canLeft} onClick={() => scrollBy(-1)} />
        <EdgeArrow side="right" visible={canRight} onClick={() => scrollBy(1)} />
      </div>
    </div>
  );
}

function EdgeArrow({
  side,
  visible,
  onClick,
}: {
  side: "left" | "right";
  visible: boolean;
  onClick: () => void;
}) {
  return (
    <div
      className={`pointer-events-none absolute inset-y-0 z-10 flex w-9 items-center ${
        side === "left" ? "right-full justify-end pr-1" : "left-full justify-start pl-1"
      }`}
    >
      {visible && (
        <button
          onClick={onClick}
          className="pointer-events-auto flex h-[50%] w-7 items-center justify-center rounded-lg border border-edge-soft/40 bg-canvas/95 text-ink-muted opacity-0 shadow-[0_3px_8px_rgba(0,0,0,0.45)] transition-opacity duration-200 hover:text-ink group-hover/row:opacity-100"
          aria-label={side === "left" ? "Scroll left" : "Scroll right"}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
            {side === "left" ? <path d="M15 18l-6-6 6-6" /> : <path d="M9 6l6 6-6 6" />}
          </svg>
        </button>
      )}
    </div>
  );
}
