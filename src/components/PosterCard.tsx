import { useState } from "react";
import type { MetaPreview } from "../lib/types";

export function PosterCard({ meta, onClick }: { meta: MetaPreview; onClick: () => void }) {
  const [broken, setBroken] = useState(false);
  const landscape = meta.posterShape === "landscape";
  return (
    <button
      onClick={onClick}
      className="group flex w-full flex-col gap-1.5 text-left focus:outline-none"
      title={meta.name}
    >
      <div
        className={`relative w-full overflow-hidden rounded-xl bg-hull ring-buoy/60 transition group-hover:ring-2 ${
          landscape ? "aspect-video" : meta.posterShape === "square" ? "aspect-square" : "aspect-[2/3]"
        }`}
      >
        {meta.poster && !broken ? (
          <img
            src={meta.poster}
            alt=""
            loading="lazy"
            onError={() => setBroken(true)}
            className="h-full w-full object-cover"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center text-3xl font-bold text-rig">
            {meta.name?.slice(0, 1).toUpperCase() || "?"}
          </div>
        )}
        {meta.imdbRating && (
          <div className="absolute right-1.5 top-1.5 rounded bg-abyss/80 px-1.5 py-0.5 text-[11px] font-semibold text-flare">
            ★ {meta.imdbRating}
          </div>
        )}
      </div>
      <div className="truncate text-sm font-medium text-foam">{meta.name}</div>
      {meta.releaseInfo && <div className="-mt-1 text-xs text-mist">{meta.releaseInfo}</div>}
    </button>
  );
}
