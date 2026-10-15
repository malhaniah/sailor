import { Row } from "./Row";

// Harbor's 18 browse-by-genre tiles; palette = per-genre hue mapped through a fixed
// oklch ramp (Harbor hand-tunes each — this keeps the same visual system).
const GENRES: { name: string; hue: number }[] = [
  { name: "Action", hue: 25 },
  { name: "Adventure", hue: 60 },
  { name: "Thriller", hue: 280 },
  { name: "Crime", hue: 335 },
  { name: "Drama", hue: 250 },
  { name: "Romance", hue: 350 },
  { name: "Mystery", hue: 300 },
  { name: "Sci-Fi", hue: 200 },
  { name: "Fantasy", hue: 160 },
  { name: "Horror", hue: 15 },
  { name: "Comedy", hue: 85 },
  { name: "Family", hue: 120 },
  { name: "Animation", hue: 190 },
  { name: "Western", hue: 45 },
  { name: "War", hue: 100 },
  { name: "History", hue: 70 },
  { name: "Documentary", hue: 220 },
  { name: "Music", hue: 320 },
];

export function GenreTiles({ onPick }: { onPick: (genre: string) => void }) {
  return (
    <Row title="Browse by Genre" min={210}>
      {GENRES.map(({ name, hue }) => (
        <button
          key={name}
          onClick={() => onPick(name)}
          className="group relative aspect-[5/4] w-full cursor-pointer overflow-hidden rounded-2xl border border-edge-soft text-left transition-transform duration-300 ease-[cubic-bezier(0.32,0.72,0.24,1)] hover:-translate-y-1"
          style={{
            background: `linear-gradient(150deg, oklch(0.40 0.18 ${hue}), oklch(0.18 0.10 ${hue}))`,
          }}
        >
          <div
            className="absolute inset-x-0 bottom-0 h-2/3"
            style={{
              background: `linear-gradient(to top, oklch(0.14 0.08 ${hue}), transparent)`,
            }}
          />
          <div className="absolute inset-x-5 bottom-5 flex items-end justify-between">
            <span
              className="font-display text-[26px] font-medium drop-shadow-[0_2px_18px_rgba(0,0,0,0.4)]"
              style={{ color: `oklch(0.92 0.06 ${hue})` }}
            >
              {name}
            </span>
            <span className="text-[18px] text-ink/70 transition-transform duration-300 group-hover:translate-x-1">
              ›
            </span>
          </div>
        </button>
      ))}
    </Row>
  );
}
