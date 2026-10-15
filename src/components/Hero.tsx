import { Info, Play, TrendingUp } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { MetaPreview } from "../lib/types";
import { fallbackGradient } from "./Poster";

/**
 * Harbor-style hero carousel. `card` = Home's rounded 560px billboard;
 * `bleed` = the Movies room's full-bleed 78vh cinema hero.
 */
export function HeroCarousel({
  items,
  variant = "card",
  typeLabel,
  onOpen,
  autoMs = 13000,
}: {
  items: MetaPreview[];
  variant?: "card" | "bleed";
  typeLabel: string;
  onOpen: (meta: MetaPreview) => void;
  autoMs?: number;
}) {
  const [active, setActive] = useState(0);
  const hovered = useRef(false);

  useEffect(() => {
    if (items.length < 2) return;
    const timer = setInterval(() => {
      if (!hovered.current && !document.hidden) {
        setActive((a) => (a + 1) % items.length);
      }
    }, autoMs);
    return () => clearInterval(timer);
  }, [items.length, autoMs]);

  if (items.length === 0) {
    return (
      <div
        className={
          variant === "card"
            ? "min-h-[560px] animate-pulse rounded-[28px] border border-edge-soft bg-elevated/30"
            : "h-[72vh] min-h-[560px] animate-pulse bg-elevated/30"
        }
      />
    );
  }

  const shell =
    variant === "card"
      ? "group relative h-[560px] cursor-pointer overflow-hidden rounded-[28px] bg-canvas"
      : "group relative h-[78vh] min-h-[640px] cursor-pointer overflow-hidden bg-canvas";

  return (
    <div className="flex flex-col gap-4">
      <section
        className={shell}
        style={{ isolation: "isolate" }}
        onMouseEnter={() => (hovered.current = true)}
        onMouseLeave={() => (hovered.current = false)}
        onClick={() => onOpen(items[active])}
      >
        {items.map((meta, i) => (
          <Slide key={meta.id} meta={meta} active={i === active} variant={variant} rank={i + 1} typeLabel={typeLabel} onOpen={onOpen} />
        ))}
      </section>
      {items.length > 1 && (
        <div className={`flex justify-center gap-2.5 ${variant === "bleed" ? "-mt-12 relative z-10 pb-2" : "pt-1"}`}>
          {items.map((_, i) => (
            <button
              key={i}
              aria-label={`Slide ${i + 1}`}
              onClick={() => setActive(i)}
              className={`h-1.5 rounded-full transition-all duration-300 ${
                i === active ? "w-12 bg-ink" : "w-6 bg-ink-muted/70 hover:bg-ink-muted"
              }`}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function Slide({
  meta,
  active,
  variant,
  rank,
  typeLabel,
  onOpen,
}: {
  meta: MetaPreview;
  active: boolean;
  variant: "card" | "bleed";
  rank: number;
  typeLabel: string;
  onOpen: (meta: MetaPreview) => void;
}) {
  const [logoOk, setLogoOk] = useState(true);
  const art = meta.background ?? meta.poster;
  return (
    <div
      className={`absolute transition-opacity duration-700 ease-[cubic-bezier(0.32,0.72,0.24,1)] ${
        variant === "card" ? "inset-[2px] rounded-[26px] overflow-hidden" : "inset-0"
      } ${active ? "opacity-100" : "pointer-events-none opacity-0"}`}
    >
      {art ? (
        <img src={art} alt="" className="absolute inset-0 h-full w-full object-cover opacity-90" />
      ) : (
        <div className="absolute inset-0" style={{ background: fallbackGradient(meta.id) }} />
      )}
      <div className="absolute inset-0 bg-gradient-to-r from-canvas via-canvas/85 via-50% to-transparent" />
      <div className="absolute inset-x-0 bottom-0 h-2/5 bg-gradient-to-t from-canvas via-canvas/70 via-50% to-transparent" />

      <div className={`relative flex h-full flex-col justify-center ${variant === "card" ? "p-14" : "justify-end pb-24 pl-20 pr-20"}`}>
        <div className="max-w-2xl">
          <div className="mb-5 inline-flex items-center gap-1.5 rounded-md bg-canvas/85 px-2.5 py-1 text-[12px] font-semibold text-ink">
            <TrendingUp size={13} className="text-accent" />
            #{rank} in {typeLabel} Today
          </div>
          <div className="flex min-h-[112px] items-end">
            {meta.logo && logoOk ? (
              <img
                src={meta.logo}
                alt={meta.name}
                onError={() => setLogoOk(false)}
                className="max-h-[120px] max-w-[460px] object-contain object-left drop-shadow-[0_6px_22px_rgba(0,0,0,0.45)]"
              />
            ) : (
              <h1 className="font-display text-[56px] font-medium leading-[0.98] tracking-tight text-ink lg:text-[68px]">
                {meta.name}
              </h1>
            )}
          </div>
          {meta.description && (
            <p className="mt-6 line-clamp-3 max-w-xl text-[16px] leading-relaxed text-ink-muted">
              {meta.description}
            </p>
          )}
          <div className="mt-6 flex flex-wrap gap-x-8 text-[14px]">
            {meta.releaseInfo && (
              <span>
                <span className="text-ink-subtle">Year: </span>
                <span className="text-ink">{meta.releaseInfo}</span>
              </span>
            )}
            {meta.imdbRating && (
              <span className="flex items-center gap-1.5">
                <span className="rounded-[3px] bg-[#f5c518] px-1 text-[10px] font-bold text-black">
                  IMDb
                </span>
                <span className="text-ink">{meta.imdbRating}</span>
              </span>
            )}
            {meta.genres.length > 0 && (
              <span className="text-ink-subtle">{meta.genres.slice(0, 3).join(" · ")}</span>
            )}
          </div>
          <div className="mt-9 flex gap-3">
            <button
              onClick={(e) => {
                e.stopPropagation();
                onOpen(meta);
              }}
              className="flex h-12 items-center gap-2.5 rounded-full bg-ink px-7 text-[15px] font-semibold text-canvas shadow-[0_8px_24px_rgba(0,0,0,0.3),inset_0_1px_0_rgba(255,255,255,0.65),inset_0_-1px_0_rgba(0,0,0,0.18)] transition-transform duration-200 hover:scale-[1.03] active:scale-[0.98]"
            >
              <Play size={17} fill="currentColor" />
              Play
            </button>
            <button
              onClick={(e) => {
                e.stopPropagation();
                onOpen(meta);
              }}
              className="flex h-12 items-center gap-2 rounded-full border border-edge bg-canvas/55 px-6 text-[15px] font-medium text-ink shadow-[inset_0_1px_0_rgba(255,255,255,0.05)] transition-colors hover:border-ink-subtle hover:bg-canvas/75"
            >
              <Info size={16} />
              More info
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
