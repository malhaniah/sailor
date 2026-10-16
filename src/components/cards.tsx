import { Poster, PosterProgress, fallbackGradient } from "./Poster";
import { useTitleProgress } from "../lib/library";
import type { MetaPreview, PosterShape } from "../lib/types";

export const SILK = "ease-[cubic-bezier(0.32,0.72,0.24,1)]";

/** The `Poster` ratio for an addon's `posterShape`. */
export function posterRatio(shape: PosterShape): "portrait" | "landscape" | "square" {
  return shape === "landscape" ? "landscape" : shape === "square" ? "square" : "portrait";
}

/** PickCard's poster shadow, with the amber ring on hover (Harbor's recipe). */
export const CARD_SHADOW =
  "shadow-[0_2px_8px_-2px_rgba(0,0,0,0.4),inset_0_1px_0_rgba(255,255,255,0.06)] transition-[box-shadow] duration-300 group-hover:shadow-[0_24px_48px_-14px_rgba(0,0,0,0.65),0_0_0_1.5px_rgba(232,170,108,0.4),inset_0_1px_0_rgba(255,255,255,0.08)]";

/**
 * Standard poster card: hover lift + deep shadow + amber ring (Harbor's PickCard). A
 * title the user is partway through shows how far along its poster's bottom edge.
 */
export function PickCard({ meta, onClick }: { meta: MetaPreview; onClick: () => void }) {
  const isNew = meta.releaseInfo === String(new Date().getFullYear());
  const progress = useTitleProgress(meta.id);
  return (
    <button onClick={onClick} className="group flex w-full min-w-0 flex-col gap-2.5 text-left">
      <div className={`relative transition-transform duration-300 ${SILK} group-hover:-translate-y-2`}>
        <Poster
          src={meta.poster}
          seed={meta.id}
          ratio={posterRatio(meta.posterShape)}
          className={CARD_SHADOW}
        >
          {progress !== null && <PosterProgress value={progress} />}
        </Poster>
        {isNew && (
          <span className="absolute left-2 top-2 rounded-md border border-edge-soft bg-canvas/95 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-ink">
            New
          </span>
        )}
        {meta.imdbRating && (
          <span
            className={`absolute right-1.5 flex items-center gap-1 rounded-md bg-canvas/95 px-1.5 py-0.5 text-[10px] font-semibold text-ink ${
              progress !== null ? "bottom-3" : "bottom-1.5"
            }`}
          >
            <span className="rounded-[3px] bg-[#f5c518] px-1 text-[8.5px] font-bold text-black">IMDb</span>
            {meta.imdbRating}
          </span>
        )}
      </div>
      <p className="line-clamp-2 text-[13px] font-medium leading-snug text-ink">{meta.name}</p>
    </button>
  );
}

/** Top-10 card: giant outlined numeral behind a right-aligned poster. */
export function TopRankCard({
  meta,
  rank,
  onClick,
}: {
  meta: MetaPreview;
  rank: number;
  onClick: () => void;
}) {
  const big = rank >= 10;
  return (
    <button
      onClick={onClick}
      className="group relative w-full min-w-0"
      style={{ aspectRatio: "228/268", containerType: "inline-size" }}
    >
      <span
        aria-hidden
        className="pointer-events-none absolute left-0 top-0 font-bold leading-[0.85] text-transparent"
        style={{
          fontFamily: "var(--font-display)",
          fontSize: `calc(100cqw * ${big ? 196 : 240} / 228)`,
          letterSpacing: big ? "-0.09em" : "-0.05em",
          WebkitTextStroke: "1.5px var(--color-ink-subtle)",
        }}
      >
        {rank}
      </span>
      <div
        className={`absolute right-0 top-0 z-10 w-[63%] transition-transform duration-300 ${SILK} group-hover:-translate-y-2`}
      >
        <Poster
          src={meta.poster}
          seed={meta.id}
          ratio="portrait"
          className="transition-[box-shadow] duration-300 group-hover:shadow-[0_24px_44px_-14px_rgba(0,0,0,0.6),0_0_0_1.5px_rgba(232,170,108,0.4)]"
        />
      </div>
      <p className="absolute bottom-0 right-0 w-[63%] truncate text-[12px] text-ink-subtle">
        {meta.name}
      </p>
    </button>
  );
}

/** Addon logo tile: image or a deterministic two-color gradient with the initial. */
export function AddonLogo({
  name,
  logo,
  size = 56,
  round = false,
}: {
  name: string;
  logo?: string | null;
  size?: number;
  round?: boolean;
}) {
  const radius = round ? "9999px" : `${Math.round(size * 0.22)}px`;
  const style = {
    width: size,
    height: size,
    borderRadius: radius,
    boxShadow: "0 0 0 1px rgba(255,255,255,0.08)",
  };
  if (logo) {
    // contain, not cover: some logos are wide wordmarks (OpenSubtitles' is 4:1)
    return <img src={logo} alt="" style={{ ...style, objectFit: "contain" }} />;
  }
  return (
    <div
      className="flex items-center justify-center font-semibold text-ink"
      style={{
        ...style,
        fontSize: size * 0.4,
        background: fallbackGradient(name),
      }}
    >
      {name.slice(0, 1).toUpperCase()}
    </div>
  );
}
