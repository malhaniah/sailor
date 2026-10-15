import { CalendarClock, ChevronDown, Play } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Poster, fallbackGradient } from "../components/Poster";
import { ErrorNote, Spinner } from "../components/ui";
import { errorMessage, metaGet } from "../lib/ipc";
import { useView } from "../lib/view";
import type { MetaItem, Video } from "../lib/types";

export function DetailView({ type, id }: { type: string; id: string }) {
  const { openPicker } = useView();
  const [meta, setMeta] = useState<MetaItem | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [logoOk, setLogoOk] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setMeta(null);
    setError(null);
    setLogoOk(true);
    metaGet(type, id)
      .then((m) => !cancelled && setMeta(m))
      .catch((e) => !cancelled && setError(errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [type, id]);

  if (error) {
    return (
      <main className="absolute inset-0 z-30 overflow-y-auto bg-canvas">
        <div className="px-12 pt-32">
          <ErrorNote message={error} />
        </div>
      </main>
    );
  }
  if (!meta) {
    return (
      <main className="absolute inset-0 z-30 overflow-y-auto bg-canvas">
        <div className="flex h-full items-center justify-center">
          <Spinner label="Loading details…" />
        </div>
      </main>
    );
  }

  const live = meta.behaviorHints.isLive || meta.type === "tv";
  const isSeries = meta.videos.length > 0 && !live;
  const defaultVideo = isSeries
    ? (meta.videos.find((v) => v.id === meta.behaviorHints.defaultVideoId) ?? null)
    : null;

  const smartPlay = () => {
    if (isSeries) {
      const firstEpisode =
        defaultVideo ??
        [...meta.videos]
          .filter((v) => (v.season ?? 0) > 0)
          .sort((a, b) => (a.season ?? 0) - (b.season ?? 0) || (a.episode ?? 0) - (b.episode ?? 0))[0] ??
        meta.videos[0];
      openPicker(meta, firstEpisode);
    } else {
      openPicker(meta, null);
    }
  };

  const backdrop = meta.background ?? meta.poster;

  return (
    <main className="absolute inset-0 z-30 overflow-y-auto bg-canvas">
      {/* hero */}
      <section className="relative">
        <div className="relative h-[78vh] min-h-[640px] overflow-hidden">
          {backdrop ? (
            <img src={backdrop} alt="" className="absolute inset-0 h-full w-full object-cover" />
          ) : (
            <div className="absolute inset-0" style={{ background: fallbackGradient(meta.id) }} />
          )}
          <div className="absolute inset-0 bg-gradient-to-t from-canvas via-canvas/55 via-45% to-transparent" />
          <div className="absolute inset-0 bg-gradient-to-r from-canvas/85 via-canvas/35 to-transparent" />

          <div className="absolute inset-x-0 bottom-0 px-12 pb-14">
            <div className="max-w-3xl">
              <div className="relative flex min-h-[120px] flex-col justify-end">
                {meta.logo && logoOk ? (
                  <img
                    src={meta.logo}
                    alt={meta.name}
                    onError={() => setLogoOk(false)}
                    className="max-h-[124px] max-w-[440px] object-contain object-left drop-shadow-[0_6px_24px_rgba(0,0,0,0.45)]"
                  />
                ) : (
                  <h1 className="font-display text-[56px] font-medium leading-[0.95] tracking-tight text-ink lg:text-[80px]">
                    {meta.name}
                  </h1>
                )}
              </div>

              <div className="mt-6 flex flex-wrap items-center gap-3 text-[13px] font-medium text-ink-muted">
                {meta.releaseInfo && <Pill>{meta.releaseInfo}</Pill>}
                {meta.imdbRating && (
                  <Pill>
                    <span className="rounded-[3px] bg-[#f5c518] px-1 text-[9px] font-bold text-black">
                      IMDb
                    </span>
                    <span className="font-semibold text-ink">{meta.imdbRating}</span>
                  </Pill>
                )}
                {meta.runtime && <Pill>{meta.runtime}</Pill>}
                {meta.genres.slice(0, 3).map((g) => (
                  <Pill key={g}>{g}</Pill>
                ))}
              </div>

              <div className="mt-9 flex gap-3">
                <button
                  onClick={smartPlay}
                  className="flex h-12 items-center gap-2.5 rounded-full bg-ink px-7 text-[15px] font-semibold text-canvas shadow-[0_8px_24px_rgba(0,0,0,0.3),inset_0_1px_0_rgba(255,255,255,0.65),inset_0_-1px_0_rgba(0,0,0,0.18)] transition-transform duration-200 hover:scale-[1.03] active:scale-[0.98]"
                >
                  <Play size={18} fill="currentColor" />
                  {isSeries && defaultVideo
                    ? `Play S${defaultVideo.season ?? "?"}:E${defaultVideo.episode ?? "?"}`
                    : "Play"}
                </button>
                {live && (
                  <span className="flex h-12 items-center gap-2 rounded-full border border-edge bg-elevated/40 px-6 text-[14px] font-medium text-ink-muted">
                    <CalendarClock size={16} />
                    Live channel
                  </span>
                )}
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* body */}
      <div className="flex flex-col gap-16 px-12 pb-24 pt-14">
        {meta.description && <Synopsis text={meta.description} />}
        {isSeries && <EpisodesSection meta={meta} onPick={(video) => openPicker(meta, video)} />}
        <InfoBlock meta={meta} />
      </div>
    </main>
  );
}

function Pill({ children }: { children: React.ReactNode }) {
  return (
    <span className="flex items-center gap-1.5 rounded-full border border-edge-soft bg-canvas/85 px-3 py-1">
      {children}
    </span>
  );
}

function Synopsis({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const [clamped, setClamped] = useState(false);
  const ref = useRef<HTMLParagraphElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const check = () => setClamped(el.scrollHeight > el.clientHeight + 2);
    check();
    const ro = new ResizeObserver(check);
    ro.observe(el);
    return () => ro.disconnect();
  }, [text]);

  return (
    <div className="max-w-3xl">
      <p
        ref={ref}
        className={`text-[16px] leading-relaxed text-ink-muted ${open ? "" : "line-clamp-4"}`}
      >
        {text}
      </p>
      {(clamped || open) && (
        <button
          onClick={() => setOpen(!open)}
          className="mt-3 flex items-center gap-1 text-[13px] font-medium text-ink-muted transition-colors hover:text-ink"
        >
          {open ? "Show less" : "Show more"}
          <ChevronDown size={14} className={`transition-transform ${open ? "rotate-180" : ""}`} />
        </button>
      )}
    </div>
  );
}

// ---- episodes -------------------------------------------------------------------------

function EpisodesSection({ meta, onPick }: { meta: MetaItem; onPick: (video: Video) => void }) {
  const seasons = useMemo(() => {
    const unique = [...new Set(meta.videos.map((v) => v.season ?? -1))];
    const rank = (s: number) => (s === 0 ? Number.MAX_SAFE_INTEGER : s === -1 ? -1 : s);
    return unique.sort((a, b) => rank(a) - rank(b));
  }, [meta.videos]);
  const [season, setSeason] = useState(() => seasons.find((s) => s > 0) ?? seasons[0]);

  const episodes = useMemo(
    () =>
      meta.videos
        .filter((v) => (v.season ?? -1) === season)
        .sort((a, b) => (a.episode ?? 0) - (b.episode ?? 0)),
    [meta.videos, season],
  );

  return (
    <section className="flex flex-col gap-5">
      <div className="flex items-end justify-between gap-6">
        <div>
          <h3 className="text-[22px] font-medium tracking-tight text-ink">Episodes</h3>
          <p className="mt-1 text-[13px] text-ink-subtle">{episodes.length} episodes</p>
        </div>
        {seasons.length > 1 && (
          <SeasonPicker seasons={seasons} value={season} onChange={setSeason} />
        )}
      </div>

      <div className="flex flex-col gap-1">
        {episodes.map((video) => (
          <EpisodeRow key={video.id} video={video} onClick={() => onPick(video)} />
        ))}
      </div>
    </section>
  );
}

function seasonLabel(season: number): string {
  if (season === 0) return "Specials";
  if (season === -1) return "Videos";
  return `Season ${season}`;
}

function SeasonPicker({
  seasons,
  value,
  onChange,
}: {
  seasons: number[];
  value: number;
  onChange: (season: number) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className="flex h-10 items-center gap-2 rounded-full border border-edge-soft bg-canvas/90 pl-4 pr-3 text-[13.5px] font-medium text-ink transition-colors hover:bg-canvas"
      >
        {seasonLabel(value)}
        <ChevronDown size={15} className={`transition-transform duration-200 ${open ? "rotate-180" : ""}`} />
      </button>
      {open && (
        <div className="animate-fade-in absolute right-0 top-full z-20 mt-2 w-64 rounded-2xl border border-edge-soft bg-canvas py-1.5 shadow-2xl">
          <div className="max-h-[60vh] overflow-y-auto">
            {seasons.map((s) => (
              <button
                key={s}
                onClick={() => {
                  onChange(s);
                  setOpen(false);
                }}
                className={`block w-full px-4 py-2.5 text-left text-[13.5px] font-medium ${
                  s === value ? "bg-ink/10 text-ink" : "text-ink-muted hover:bg-elevated/60 hover:text-ink"
                }`}
              >
                {seasonLabel(s)}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function EpisodeRow({ video, onClick }: { video: Video; onClick: () => void }) {
  const parsed = video.released ? new Date(video.released) : null;
  const released = parsed && Number.isFinite(parsed.getTime()) ? parsed : null;
  const upcoming = released !== null && released.getTime() > Date.now();
  const metaLine = [
    video.season != null && video.episode != null ? `S${video.season} E${video.episode}` : null,
    released ? released.toISOString().slice(0, 10) : null,
  ]
    .filter(Boolean)
    .join("  ·  ");

  return (
    <button
      onClick={onClick}
      className={`group flex gap-6 rounded-2xl px-4 py-5 text-left transition-colors hover:bg-elevated/30 ${
        upcoming ? "opacity-55" : ""
      }`}
    >
      <div className="relative w-[200px] shrink-0 overflow-hidden rounded-lg">
        <Poster src={video.thumbnail} seed={video.id} ratio="landscape" className="rounded-lg" />
        <div className="absolute inset-0 flex items-center justify-center bg-canvas/40 opacity-0 transition-opacity group-hover:opacity-100">
          <span className="flex h-12 w-12 items-center justify-center rounded-full bg-ink text-canvas">
            <Play size={18} fill="currentColor" className="ml-0.5" />
          </span>
        </div>
        {video.episode != null && (
          <span className="absolute left-1.5 top-1.5 rounded-md bg-canvas/95 px-1.5 py-0.5 text-[11px] font-semibold text-ink">
            {video.episode}
          </span>
        )}
      </div>
      <div className="flex min-w-0 flex-col gap-1.5 py-1">
        <div className="flex items-center gap-2.5">
          <span className="truncate text-[16px] font-semibold text-ink">
            {video.title ?? video.id}
          </span>
          {upcoming && (
            <span className="rounded-[5px] border border-edge-soft bg-elevated/40 px-1.5 text-[9px] font-bold uppercase tracking-[0.14em] text-ink-subtle">
              Upcoming
            </span>
          )}
        </div>
        {metaLine && <span className="text-[12px] text-ink-subtle">{metaLine}</span>}
        {video.overview && (
          <p className="line-clamp-2 text-[13.5px] leading-relaxed text-ink-muted">
            {video.overview}
          </p>
        )}
      </div>
    </button>
  );
}

// ---- information ----------------------------------------------------------------------

function InfoBlock({ meta }: { meta: MetaItem }) {
  const rows: [string, string | null | undefined][] = [
    ["Type", meta.type],
    ["Released", meta.released?.slice(0, 10)],
    ["Runtime", meta.runtime],
    ["Genres", meta.genres.length ? meta.genres.join(" · ") : null],
    ["Website", meta.website],
    ["ID", meta.id],
  ];
  const present = rows.filter(([, v]) => v);
  if (present.length === 0) return null;
  return (
    <section className="border-t border-edge-soft pt-12">
      <h3 className="mb-6 text-[22px] font-medium tracking-tight text-ink">Information</h3>
      <dl className="grid grid-cols-1 gap-x-12 gap-y-5 sm:grid-cols-2 lg:grid-cols-3">
        {present.map(([label, value]) => (
          <div key={label}>
            <dt className="text-[12px] font-medium uppercase tracking-[0.18em] text-ink-subtle">
              {label}
            </dt>
            <dd className="mt-1 break-words text-[14.5px] text-ink">{value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
