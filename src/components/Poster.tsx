import { useState } from "react";

/** Deterministic 3-layer oklch gradient for items without artwork (Harbor's recipe). */
export function fallbackGradient(seed: string): string {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  const a = hash % 360;
  const b = (a + 140) % 360;
  const c = (a + 60) % 360;
  return [
    `radial-gradient(ellipse at 25% 30%, oklch(0.45 0.14 ${a}) 0%, transparent 55%)`,
    `radial-gradient(ellipse at 75% 75%, oklch(0.32 0.10 ${b}) 0%, transparent 55%)`,
    `linear-gradient(135deg, oklch(0.20 0.05 ${c}), oklch(0.10 0.02 ${b}))`,
  ].join(", ");
}

const ASPECTS = {
  portrait: "aspect-[2/3]",
  landscape: "aspect-video",
  wide: "aspect-[16/7]",
  square: "aspect-square",
} as const;

export function Poster({
  src,
  seed,
  ratio = "portrait",
  className = "",
}: {
  src?: string | null;
  seed: string;
  ratio?: keyof typeof ASPECTS;
  className?: string;
}) {
  const [broken, setBroken] = useState(false);
  const showImage = src && !broken;
  return (
    <div className={`relative overflow-hidden rounded-xl ${ASPECTS[ratio]} ${className}`}>
      {showImage ? (
        <img
          src={src}
          alt=""
          loading="lazy"
          decoding="async"
          onError={() => setBroken(true)}
          className="absolute inset-0 h-full w-full object-cover"
        />
      ) : (
        <div className="absolute inset-0" style={{ background: fallbackGradient(seed) }} />
      )}
    </div>
  );
}
