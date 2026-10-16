import { CalendarClock, Check, CheckCheck, ChevronDown, Info, Play, Plus } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { NO_HOVER_SHOW } from "../components/LibraryCard";
import { Poster, PosterProgress, fallbackGradient } from "../components/Poster";
import { ErrorNote, Spinner } from "../components/ui";
import { errorMessage, metaGet } from "../lib/ipc";
import { useLibraryItem } from "../lib/library";
import {
  type ResumePoint,
  type VideoRef,
  libraryMarkWatched,
  librarySet,
  metaSummary,
  videoRefs,
} from "../lib/library-ipc";
import { useView } from "../lib/view";
import type { MetaItem, MetaLookup, MetaPreview, Video } from "../lib/types";

const NO_VIDEOS: VideoRef[] = [];

/** A minimal meta object built from the catalog item that was clicked. */
function metaFromPreview(preview: MetaPreview): MetaItem {
  return {
    id: preview.id,
    type: preview.type,
    name: preview.name,
    poster: preview.poster,
    background: preview.background,
    logo: preview.logo,
    posterShape: preview.posterShape,
    description: preview.description,
    releaseInfo: preview.releaseInfo,
    imdbRating: preview.imdbRating,
    genres: preview.genres,
    links: [],
    videos: [],
    behaviorHints: { isLive: false, hasScheduledVideos: false },
  };
}

/** Why the full details are missing, in words a user can act on. */
function fallbackReason(lookup: MetaLookup): string {
  if (lookup.asked.length === 0) {
    return "None of your installed addons provides details for this title (some catalog addons rely on a separate metadata addon such as Cinemeta), so Zeiler is showing what the catalog provided.";
  }
  return `Your metadata addons couldn't load this title (${lookup.errors.join("; ")}), so Zeiler is showing what the catalog provided.`;
}

export function DetailView({
  type,
  id,
  preview,
}: {
  type: string;
  id: string;
  preview: MetaPreview;
}) {
  const { openPicker } = useView();
  const [meta, setMeta] = useState<MetaItem | null>(null);
  const [fallbackNote, setFallbackNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [logoOk, setLogoOk] = useState(true);
  // the library's view of this title, with the watched marks of its episodes
  const refs = useMemo(() => (meta ? videoRefs(meta.videos) : NO_VIDEOS), [meta]);
  const { detail, loaded: libraryLoaded, refresh } = useLibraryItem(meta?.id ?? null, refs);
  const watchedIds = useMemo(() => new Set(detail?.watchedVideoIds ?? []), [detail]);
  const [busy, setBusy] = useState(false);
  const [libraryError, setLibraryError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setMeta(null);
    setFallbackNote(null);
    setError(null);
    setLogoOk(true);
    setLibraryError(null);
    metaGet(type, id)
      .then((lookup) => {
        if (cancelled) return;
        if (lookup.meta) {
          setMeta(lookup.meta);
        } else {
          setMeta(metaFromPreview(preview));
          setFallbackNote(fallbackReason(lookup));
        }
      })
      .catch((e) => !cancelled && setError(errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [type, id, preview]);

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
  // a series without an episode list (catalog-only fallback): episodes are picked by number
  const needsManualEpisode = !live && meta.videos.length === 0 && meta.type === "series";
  const defaultVideo = isSeries
    ? (meta.videos.find((v) => v.id === meta.behaviorHints.defaultVideoId) ?? null)
    : null;
  const entry = detail?.entry ?? null;
  // the episode Continue Watching points at: Play goes there
  const continueVideo =
    isSeries && entry?.inContinueWatching
      ? (meta.videos.find((v) => v.id === entry.videoId) ?? null)
      : null;
  // the video Play asks streams for, as the picker does (a movie's default video)
  const playVideoId = continueVideo?.id ?? (isSeries ? null : (meta.behaviorHints.defaultVideoId ?? meta.id));
  const resumes = !!entry?.resume && entry.resume.videoId === playVideoId;

  /** Runs one library change; buttons wait for it, and the title is read again after. */
  const change = (run: () => Promise<unknown>) => {
    setBusy(true);
    setLibraryError(null);
    run()
      .catch((e) => setLibraryError(errorMessage(e)))
      .finally(() => {
        setBusy(false);
        refresh();
      });
  };
  const summary = () => metaSummary(meta);
  const markVideos = (videoIds: string[], watched: boolean) =>
    change(() => libraryMarkWatched({ meta: summary(), videos: refs, videoIds, watched }));
  const markSeason = (season: number, watched: boolean) =>
    change(() =>
      libraryMarkWatched({ meta: summary(), videos: refs, videoIds: [], season, watched }),
    );

  // "Watched" for the whole title. A series is watched episode by episode: the toggle
  // marks every numbered episode already out (the specials aside; with or without a
  // season, as anime addons send them) and shows whether they all are. The library's
  // own title mark (`timesWatched`, which one episode past 70% already sets) is for
  // everything else, and for series without numbered episodes.
  const seriesEpisodes = isSeries
    ? meta.videos.filter(
        (v) => v.episode != null && (v.season == null || v.season > 0) && !isUpcoming(v),
      )
    : [];
  const byEpisodes = seriesEpisodes.length > 0;
  const titleWatched = byEpisodes
    ? seriesEpisodes.every((v) => watchedIds.has(v.id))
    : !!entry?.watched;
  const toggleTitleWatched = () =>
    byEpisodes
      ? markVideos(
          seriesEpisodes.map((v) => v.id),
          !titleWatched,
        )
      : change(() =>
          libraryMarkWatched({ meta: summary(), videos: refs, videoIds: [], watched: !titleWatched }),
        );

  const smartPlay = () => {
    if (continueVideo) {
      openPicker(meta, continueVideo);
    } else if (isSeries) {
      const firstEpisode =
        defaultVideo ??
        [...meta.videos]
          .filter((v) => (v.season ?? 0) > 0)
          .sort((a, b) => (a.season ?? 0) - (b.season ?? 0) || (a.episode ?? 0) - (b.episode ?? 0))[0] ??
        meta.videos[0];
      openPicker(meta, firstEpisode);
    } else if (needsManualEpisode) {
      openPicker(meta, episodeVideo(meta, 1, 1));
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

              <div className="mt-9 flex flex-wrap gap-3">
                <button
                  onClick={smartPlay}
                  className="flex h-12 items-center gap-2.5 rounded-full bg-ink px-7 text-[15px] font-semibold text-canvas shadow-[0_8px_24px_rgba(0,0,0,0.3),inset_0_1px_0_rgba(255,255,255,0.65),inset_0_-1px_0_rgba(0,0,0,0.18)] transition-transform duration-200 hover:scale-[1.03] active:scale-[0.98]"
                >
                  <Play size={18} fill="currentColor" />
                  {playLabel(
                    resumes ? "Resume" : continueVideo ? "Continue" : "Play",
                    continueVideo ?? defaultVideo,
                  )}
                </button>
                <HeroToggle
                  on={!!entry?.inLibrary}
                  disabled={busy}
                  onClick={() => change(() => librarySet(summary(), !entry?.inLibrary))}
                  icon={entry?.inLibrary ? <Check size={17} strokeWidth={2.4} /> : <Plus size={18} />}
                  label={entry?.inLibrary ? "In Library" : "Add to Library"}
                  title={entry?.inLibrary ? "Remove from your library" : "Add to your library"}
                />
                {!live && (
                  <HeroToggle
                    on={titleWatched}
                    disabled={busy}
                    onClick={toggleTitleWatched}
                    icon={<CheckCheck size={17} strokeWidth={2.2} />}
                    label={titleWatched ? "Watched" : "Mark as watched"}
                    title={
                      byEpisodes
                        ? titleWatched
                          ? "Mark every episode unwatched"
                          : "Mark every episode out so far watched"
                        : titleWatched
                          ? "Mark the whole title unwatched"
                          : "Mark the whole title watched"
                    }
                  />
                )}
                {live && (
                  <span className="flex h-12 items-center gap-2 rounded-full border border-edge bg-elevated/40 px-6 text-[14px] font-medium text-ink-muted">
                    <CalendarClock size={16} />
                    Live channel
                  </span>
                )}
              </div>
              {libraryError && (
                <p role="alert" className="mt-4 text-[13px] font-medium text-[oklch(0.75_0.12_25)]">
                  {libraryError}
                </p>
              )}
            </div>
          </div>
        </div>
      </section>

      {/* body */}
      <div className="flex flex-col gap-16 px-12 pb-24 pt-14">
        {fallbackNote && (
          <div className="flex max-w-3xl items-start gap-3 rounded-2xl border border-edge-soft bg-elevated/40 px-5 py-4 text-[13.5px] leading-relaxed text-ink-muted">
            <Info size={17} className="mt-0.5 shrink-0 text-accent" />
            <span>{fallbackNote}</span>
          </div>
        )}
        {meta.description && <Synopsis text={meta.description} />}
        {isSeries && (
          <EpisodesSection
            meta={meta}
            watchedIds={watchedIds}
            resume={entry?.resume ?? null}
            focusVideoId={continueVideo?.id ?? null}
            libraryLoaded={libraryLoaded}
            busy={busy}
            onPick={(video) => openPicker(meta, video)}
            onMarkVideos={markVideos}
            onMarkSeason={markSeason}
          />
        )}
        {needsManualEpisode && (
          <ManualEpisodePicker onPick={(s, e) => openPicker(meta, episodeVideo(meta, s, e))} />
        )}
        <InfoBlock meta={meta} />
      </div>
    </main>
  );
}

/** "Resume S1:E3", "Continue S1:E4" (moved on to it), "Play S1:E1"; "Resume" / "Play". */
function playLabel(verb: "Resume" | "Continue" | "Play", video: Video | null): string {
  return video ? `${verb} S${video.season ?? "?"}:E${video.episode ?? "?"}` : verb;
}

/**
 * A hero action that is on or off (Add to Library, Mark as watched). The label says
 * which, so there is no `aria-pressed` (a toggle's name must not change); `title`
 * says what a click does.
 */
function HeroToggle({
  on,
  disabled,
  onClick,
  icon,
  label,
  title,
}: {
  on: boolean;
  disabled: boolean;
  onClick: () => void;
  icon: ReactNode;
  label: string;
  title: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={`flex h-12 items-center gap-2 rounded-full border px-6 text-[14px] font-medium backdrop-blur-sm transition-colors disabled:cursor-progress disabled:opacity-70 ${
        on
          ? "border-accent/50 bg-accent/15 text-accent hover:bg-accent/25"
          : "border-edge bg-canvas/60 text-ink hover:bg-elevated"
      }`}
    >
      {icon}
      {label}
    </button>
  );
}

/** A video for a series episode chosen by number (no episode list available). */
function episodeVideo(meta: MetaItem, season: number, episode: number): Video {
  return {
    id: `${meta.id}:${season}:${episode}`,
    title: `Episode ${episode}`,
    season,
    episode,
    streams: [],
  };
}

function ManualEpisodePicker({ onPick }: { onPick: (season: number, episode: number) => void }) {
  const [season, setSeason] = useState(1);
  const [episode, setEpisode] = useState(1);
  const field =
    "h-11 w-20 rounded-xl border border-edge bg-canvas px-3 text-center font-mono text-[14px] text-ink outline-none focus:border-ink";
  const clamp = (value: string) => Math.max(1, Math.min(999, Math.floor(Number(value)) || 1));
  return (
    <section className="flex flex-col gap-4">
      <div>
        <h3 className="text-[22px] font-medium tracking-tight text-ink">Episodes</h3>
        <p className="mt-1 text-[13px] text-ink-subtle">
          No episode list is available, so pick the episode by number.
        </p>
      </div>
      <div className="flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1.5 text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
          Season
          <input type="number" min={1} value={season} onChange={(e) => setSeason(clamp(e.target.value))} className={field} />
        </label>
        <label className="flex flex-col gap-1.5 text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
          Episode
          <input type="number" min={1} value={episode} onChange={(e) => setEpisode(clamp(e.target.value))} className={field} />
        </label>
        <button
          onClick={() => onPick(season, episode)}
          className="flex h-11 items-center gap-2 rounded-xl bg-ink px-5 text-[13.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97]"
        >
          <Play size={15} fill="currentColor" />
          Find streams for S{season}E{episode}
        </button>
      </div>
    </section>
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

function EpisodesSection({
  meta,
  watchedIds,
  resume,
  focusVideoId,
  libraryLoaded,
  busy,
  onPick,
  onMarkVideos,
  onMarkSeason,
}: {
  meta: MetaItem;
  /** the episodes marked watched in the library */
  watchedIds: ReadonlySet<string>;
  resume: ResumePoint | null;
  /** the episode Continue Watching points at: its season opens first */
  focusVideoId: string | null;
  /** the library's first read of this title has answered (`focusVideoId` is known) */
  libraryLoaded: boolean;
  /** a library change is running */
  busy: boolean;
  onPick: (video: Video) => void;
  onMarkVideos: (videoIds: string[], watched: boolean) => void;
  onMarkSeason: (season: number, watched: boolean) => void;
}) {
  const seasons = useMemo(() => {
    const unique = [...new Set(meta.videos.map((v) => v.season ?? -1))];
    const rank = (s: number) => (s === 0 ? Number.MAX_SAFE_INTEGER : s === -1 ? -1 : s);
    return unique.sort((a, b) => rank(a) - rank(b));
  }, [meta.videos]);
  const [season, setSeason] = useState(() => seasons.find((s) => s > 0) ?? seasons[0]);

  // Open on the season of the episode to continue: decided once, when the library's
  // first read answers (after the first render), so neither a season the user picked
  // nor a mark that moves Continue Watching on later moves the view.
  const focused = useRef(false);
  const focusSeason = meta.videos.find((v) => v.id === focusVideoId)?.season ?? null;
  useEffect(() => {
    if (focused.current || !libraryLoaded) return;
    focused.current = true;
    if (focusSeason !== null && seasons.includes(focusSeason)) setSeason(focusSeason);
  }, [libraryLoaded, focusSeason, seasons]);

  const episodes = useMemo(
    () =>
      meta.videos
        .filter((v) => (v.season ?? -1) === season)
        .sort((a, b) => (a.episode ?? 0) - (b.episode ?? 0)),
    [meta.videos, season],
  );

  // A season can be marked once an episode of it is out (the library marks numbered
  // episodes of a season number, never negative). Marking takes in only what is out:
  // a mark moves the title's "last watched" up to the episode's release date, which
  // must not lie in the future. Unmarking clears the whole season.
  const numbered = season < 0 ? [] : episodes.filter((v) => v.episode != null);
  const released = numbered.filter((v) => !isUpcoming(v));
  const seasonWatched =
    released.length > 0 && released.every((v) => watchedIds.has(v.id));
  const markSeason = () => {
    if (seasonWatched || released.length === numbered.length) {
      onMarkSeason(season, !seasonWatched);
    } else {
      onMarkVideos(
        released.map((v) => v.id),
        true,
      );
    }
  };

  return (
    <section className="flex flex-col gap-5">
      <div className="flex flex-wrap items-end justify-between gap-6">
        <div>
          <h3 className="text-[22px] font-medium tracking-tight text-ink">Episodes</h3>
          <p className="mt-1 text-[13px] text-ink-subtle">{episodes.length} episodes</p>
        </div>
        <div className="flex flex-wrap items-center gap-3">
          {released.length > 0 && (
            <button
              onClick={markSeason}
              disabled={busy}
              className={`flex h-10 items-center gap-2 rounded-full border px-4 text-[13px] font-medium transition-colors disabled:cursor-progress disabled:opacity-70 ${
                seasonWatched
                  ? "border-accent/50 bg-accent/15 text-accent hover:bg-accent/25"
                  : "border-edge-soft bg-canvas/90 text-ink-muted hover:border-edge hover:text-ink"
              }`}
            >
              <CheckCheck size={15} strokeWidth={2.2} />
              {seasonWatched ? `${seasonLabel(season)} watched` : `Mark ${seasonLabel(season).toLowerCase()} watched`}
            </button>
          )}
          {seasons.length > 1 && (
            <SeasonPicker
              seasons={seasons}
              value={season}
              onChange={(s) => {
                focused.current = true;
                setSeason(s);
              }}
            />
          )}
        </div>
      </div>

      <div className="flex flex-col gap-1">
        {episodes.map((video) => (
          <EpisodeRow
            key={video.id}
            video={video}
            watched={watchedIds.has(video.id)}
            progress={
              resume?.videoId === video.id && resume.duration > 0
                ? resume.timeOffset / resume.duration
                : null
            }
            busy={busy}
            onClick={() => onPick(video)}
            onMark={(watched) => onMarkVideos([video.id], watched)}
          />
        ))}
      </div>
    </section>
  );
}

/** Not out yet: released after now. */
function isUpcoming(video: Video): boolean {
  const released = video.released ? new Date(video.released).getTime() : Number.NaN;
  return Number.isFinite(released) && released > Date.now();
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

function EpisodeRow({
  video,
  watched,
  progress,
  busy,
  onClick,
  onMark,
}: {
  video: Video;
  watched: boolean;
  /** how far into it the resume point is (0..1), when it is the one to resume */
  progress: number | null;
  busy: boolean;
  onClick: () => void;
  onMark: (watched: boolean) => void;
}) {
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
    <div className="group/episode flex items-center gap-2 rounded-2xl pr-4 transition-colors hover:bg-elevated/30">
      <button
        onClick={onClick}
        className={`group flex min-w-0 flex-1 gap-6 rounded-2xl px-4 py-5 text-left ${
          upcoming ? "opacity-55" : ""
        }`}
      >
        <div className="relative w-[200px] shrink-0 overflow-hidden rounded-lg">
          <Poster src={video.thumbnail} seed={video.id} ratio="landscape" className="rounded-lg">
            {progress !== null && <PosterProgress value={progress} />}
          </Poster>
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
      {/* The watched mark: always shown once set, on hover (or focus) before. An
          episode not out yet can't be marked (its release date would become the
          title's "last watched"), only unmarked. */}
      {upcoming && !watched ? (
        <span aria-hidden className="w-10 shrink-0" />
      ) : (
        <button
          onClick={() => onMark(!watched)}
          disabled={busy}
          aria-label={`${watched ? "Mark unwatched" : "Mark watched"}: ${video.title ?? video.id}`}
          title={watched ? "Watched — mark unwatched" : "Mark watched"}
          className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-full border transition-[opacity,background-color,color] disabled:cursor-progress ${
            watched
              ? "border-accent/50 bg-accent/15 text-accent hover:bg-accent/25"
              : `border-edge-soft text-ink-muted opacity-0 hover:border-edge hover:text-ink focus-visible:opacity-100 group-hover/episode:opacity-100 ${NO_HOVER_SHOW}`
          }`}
        >
          <Check size={17} strokeWidth={2.6} />
        </button>
      )}
    </div>
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
