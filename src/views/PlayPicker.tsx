import { ChevronDown, ChevronLeft, ExternalLink, Play } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AddonLogo } from "../components/cards";
import { fallbackGradient } from "../components/Poster";
import { Badge, EmptyState, Modal } from "../components/ui";
import { errorMessage, streamsGet } from "../lib/ipc";
import { launchForStream, openPlayer, subscribeLaunchFailures } from "../lib/playback-handoff";
import { useView } from "../lib/view";
import type { AddonStreams, MetaItem, Stream, Video } from "../lib/types";

type Row = { stream: Stream; addonName: string; transportUrl: string; key: string };

/** Only streams that leave the app (YouTube, external links) can fail here; everything
 * else plays on the Player screen, which reports failures back (`subscribeLaunchFailures`). */
type Phase = { kind: "idle" } | { kind: "error"; message: string };

export function PlayPickerView({ meta, video }: { meta: MetaItem; video: Video | null }) {
  const { goBack } = useView();
  const live = meta.behaviorHints.isLive || meta.type === "tv";
  const videoId = live
    ? (meta.behaviorHints.defaultVideoId ?? meta.id)
    : (video?.id ?? meta.behaviorHints.defaultVideoId ?? meta.id);

  const [groups, setGroups] = useState<AddonStreams[] | null>(null);
  const [filter, setFilter] = useState<string | null>(null); // transportUrl or null = all
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [failed, setFailed] = useState<Set<string>>(new Set());

  useEffect(() => {
    let cancelled = false;
    setGroups(null);
    setFailed(new Set());
    streamsGet(meta.type, videoId).then((g) => !cancelled && setGroups(g));
    return () => {
      cancelled = true;
    };
  }, [meta.type, videoId]);

  // Launch id -> row key for streams this picker opened in the Player, so a failure
  // reported there marks the row here too (this view stays mounted under the player).
  const opened = useRef(new Map<string, string>());
  useEffect(() => {
    opened.current.clear();
    return subscribeLaunchFailures((id) => {
      const key = opened.current.get(id);
      if (key !== undefined) setFailed((prev) => new Set(prev).add(key));
    });
  }, [meta.type, videoId]);

  const rows = useMemo(() => {
    const result: Row[] = [];
    if (video && video.streams.length > 0) {
      video.streams.forEach((stream, i) =>
        result.push({
          stream,
          addonName: "Embedded",
          transportUrl: "embedded",
          key: `embedded-${i}`,
        }),
      );
    }
    for (const group of groups ?? []) {
      group.streams.forEach((stream, i) =>
        result.push({
          stream,
          addonName: group.addonName,
          transportUrl: group.transportUrl,
          key: `${group.transportUrl}-${i}`,
        }),
      );
    }
    return filter ? result.filter((r) => r.transportUrl === filter) : result;
  }, [groups, video, filter]);

  const settled = groups !== null;
  const addonOptions = useMemo(() => {
    const seen = new Map<string, { name: string; count: number }>();
    if (video && video.streams.length > 0) {
      seen.set("embedded", { name: "Embedded", count: video.streams.length });
    }
    for (const group of groups ?? []) {
      if (group.streams.length > 0 || group.error) {
        seen.set(group.transportUrl, { name: group.addonName, count: group.streams.length });
      }
    }
    return [...seen.entries()];
  }, [groups, video]);

  function launchFor(row: Row) {
    return launchForStream({
      meta,
      video,
      stream: row.stream,
      // "embedded" marks streams that came with the meta itself, not from an addon
      transportUrl: row.transportUrl === "embedded" ? null : row.transportUrl,
      addonName: row.addonName,
    });
  }

  /** Direct and torrent streams open Zeiler's Player screen, which resolves and plays
   * them; YouTube and external links leave the app. `backend: "external"` is the row's
   * secondary action ("Open in external mpv"): the same Player screen, with mpv in its
   * own window under Zeiler's remote controls, whatever the backend setting says. */
  async function play(row: Row, backend: "auto" | "external" = "auto") {
    const { stream } = row;
    const launch = launchFor(row);
    if (launch) {
      opened.current.set(openPlayer({ ...launch, backend }), row.key);
      return;
    }
    try {
      if (stream.externalUrl) {
        if (/^https?:\/\//i.test(stream.externalUrl)) await openUrl(stream.externalUrl);
        setPhase({ kind: "idle" });
      } else if (stream.ytId) {
        await openUrl(`https://www.youtube.com/watch?v=${encodeURIComponent(stream.ytId)}`);
        setPhase({ kind: "idle" });
      }
    } catch (e) {
      setFailed((prev) => new Set(prev).add(row.key));
      setPhase({ kind: "error", message: errorMessage(e) });
    }
  }

  const backdrop = video?.thumbnail ?? meta.background ?? meta.poster;

  return (
    <main className="absolute inset-0 z-50 overflow-y-auto bg-canvas">
      {/* backdrop */}
      <div className="pointer-events-none fixed inset-0 -z-10">
        {backdrop ? (
          <img src={backdrop} alt="" className="h-full w-full scale-105 object-cover opacity-50" />
        ) : (
          <div className="h-full w-full" style={{ background: fallbackGradient(meta.id) }} />
        )}
        <div className="absolute inset-0 bg-gradient-to-b from-canvas/30 via-canvas/85 to-canvas" />
        <div className="absolute inset-0 bg-gradient-to-r from-canvas/55 via-transparent to-canvas/55" />
      </div>

      <button
        onClick={goBack}
        className="fixed left-5 top-5 z-[200] flex h-11 w-11 items-center justify-center rounded-full border border-edge bg-canvas/80 text-ink backdrop-blur-md transition-colors hover:bg-elevated"
        aria-label="Back"
      >
        <ChevronLeft size={20} strokeWidth={2.2} />
      </button>

      <div className="relative mx-auto flex min-h-full w-full max-w-5xl flex-col gap-12 px-12 pb-32 pt-32">
        {/* header */}
        <header>
          <div className="text-[11px] font-semibold uppercase tracking-[0.32em] text-ink-subtle">
            {video
              ? `${meta.name} · Season ${video.season ?? "?"} · Episode ${String(video.episode ?? "?").padStart(2, "0")}`
              : [meta.releaseInfo, ...meta.genres.slice(0, 2)].filter(Boolean).join(" · ")}
          </div>
          <h1 className="mt-3 font-display text-[48px] font-medium leading-[0.96] tracking-tight text-ink lg:text-[64px]">
            {video ? (video.title ?? `Episode ${video.episode ?? "?"}`) : meta.name}
          </h1>
          {video?.overview && (
            <p className="mt-2 max-w-2xl text-[14.5px] leading-relaxed text-ink-muted">
              {video.overview}
            </p>
          )}
        </header>

        {/* searching */}
        {!settled && rows.length === 0 && (
          <div className="flex flex-col items-center gap-8 py-24">
            <div className="animate-loader-pulse font-display text-[48px] font-medium text-ink lg:text-[72px]">
              {meta.name}
            </div>
            <div className="text-[11px] font-semibold uppercase tracking-[0.42em] text-ink-subtle">
              Searching streams
            </div>
          </div>
        )}

        {/* empty */}
        {settled && rows.length === 0 && (
          <EmptyState title={`${groups?.length ?? 0} addons answered — nothing playable`}>
            No installed addon returned a stream for{" "}
            <span className="font-mono text-[12px]">{videoId}</span>. Install a stream addon
            that covers this content.
          </EmptyState>
        )}

        {/* addon filter */}
        {settled && addonOptions.length > 1 && (
          <AddonFilter
            options={addonOptions}
            value={filter}
            total={rows.length}
            onChange={setFilter}
          />
        )}

        {/* rows */}
        {rows.length > 0 && (
          <div className="flex flex-col gap-2">
            {rows.map((row) => (
              <StreamRow
                key={row.key}
                row={row}
                failed={failed.has(row.key)}
                onPlay={() => void play(row)}
                onPlayExternal={() => void play(row, "external")}
              />
            ))}
          </div>
        )}

        {/* per-addon errors */}
        {settled && (
          <div className="flex flex-wrap gap-2">
            {(groups ?? [])
              .filter((g) => g.error)
              .map((g) => (
                <Badge key={g.transportUrl} tone="warn">
                  {g.addonName}: {g.error}
                </Badge>
              ))}
          </div>
        )}
      </div>

      {/* overlays */}
      {phase.kind === "error" && (
        <Modal title="Could not play" onClose={() => setPhase({ kind: "idle" })}>
          <p className="text-[13.5px] leading-relaxed text-ink">{phase.message}</p>
        </Modal>
      )}
    </main>
  );
}

function AddonFilter({
  options,
  value,
  total,
  onChange,
}: {
  options: [string, { name: string; count: number }][];
  value: string | null;
  total: number;
  onChange: (value: string | null) => void;
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

  const selected = value ? options.find(([url]) => url === value) : null;

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className="flex w-full items-center justify-between gap-3 rounded-2xl bg-elevated/60 px-4 py-3 text-[15px] font-medium text-ink ring-1 ring-edge-soft transition-colors hover:bg-elevated"
      >
        <span className="flex items-center gap-3">
          <AddonLogo name={selected ? selected[1].name : "All"} size={36} round />
          {selected ? selected[1].name : "All sources"}
        </span>
        <ChevronDown size={17} className={`transition-transform duration-200 ${open ? "rotate-180" : ""}`} />
      </button>
      {open && (
        <div className="animate-fade-in absolute inset-x-0 top-full z-20 mt-1 max-h-80 overflow-y-auto rounded-2xl bg-elevated p-1.5 shadow-[0_18px_44px_-14px_rgba(0,0,0,0.7)] ring-1 ring-edge">
          <FilterOption
            label="All sources"
            count={total}
            active={value === null}
            onClick={() => {
              onChange(null);
              setOpen(false);
            }}
          />
          {options.map(([url, info]) => (
            <FilterOption
              key={url}
              label={info.name}
              count={info.count}
              active={value === url}
              onClick={() => {
                onChange(url);
                setOpen(false);
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function FilterOption({
  label,
  count,
  active,
  onClick,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left text-[14px] ${
        active ? "bg-raised text-ink" : "text-ink-muted hover:bg-raised hover:text-ink"
      }`}
    >
      <AddonLogo name={label} size={28} round />
      <span className="flex-1 truncate">{label}</span>
      <span className="text-[12px] text-ink-subtle">{count}</span>
    </button>
  );
}

function StreamRow({
  row,
  failed,
  onPlay,
  onPlayExternal,
}: {
  row: { stream: Stream; addonName: string };
  failed: boolean;
  onPlay: () => void;
  onPlayExternal: () => void;
}) {
  const { stream, addonName } = row;
  const unsupported = !stream.url && !stream.infoHash && !stream.externalUrl && !stream.ytId;
  const kindLabel = stream.url
    ? "direct"
    : stream.infoHash
      ? "torrent"
      : stream.ytId
        ? "youtube"
        : stream.externalUrl
          ? "external"
          : "unsupported";
  const size = stream.behaviorHints.videoSize
    ? `${(stream.behaviorHints.videoSize / 1e9).toFixed(2)} GB`
    : null;

  return (
    <div
      className={`flex items-stretch gap-5 rounded-2xl bg-elevated/40 p-5 ring-1 transition-opacity ${
        failed ? "bg-danger/5 ring-danger/40" : "ring-edge-soft/50"
      }`}
    >
      <div className="flex w-[68px] shrink-0 flex-col items-center gap-1.5">
        <AddonLogo name={addonName} size={56} />
        <span className="text-[9.5px] font-semibold uppercase tracking-wider text-ink-subtle">
          {kindLabel}
        </span>
      </div>
      <div className="min-w-0 flex-1">
        <div className="whitespace-pre-line text-[16px] font-semibold leading-snug text-ink">
          {stream.name ?? addonName}
        </div>
        {stream.description && (
          <div className="mt-1 whitespace-pre-line text-[14.5px] leading-snug text-ink-muted">
            {stream.description}
          </div>
        )}
        <div className="mt-1.5 flex flex-wrap items-center gap-2">
          {size && <Badge>{size}</Badge>}
          {stream.behaviorHints.bingeGroup && <Badge>{stream.behaviorHints.bingeGroup}</Badge>}
          {failed && (
            <span className="text-[13px] font-medium text-[oklch(0.75_0.12_25)]">
              Unavailable, try another.
            </span>
          )}
        </div>
      </div>
      <div className="flex items-center gap-3">
        {(stream.url || stream.infoHash) && (
          <button
            onClick={onPlayExternal}
            className="flex h-10 w-10 items-center justify-center rounded-full bg-raised text-ink-muted transition-colors hover:bg-elevated hover:text-ink"
            aria-label="Open in external mpv"
            title="Open in external mpv (its own window, controlled from Zeiler)"
          >
            <ExternalLink size={16} />
          </button>
        )}
        <button
          disabled={unsupported}
          onClick={onPlay}
          className="flex h-16 w-16 items-center justify-center rounded-full bg-accent text-canvas transition-transform hover:opacity-90 active:scale-95 disabled:cursor-not-allowed disabled:opacity-40"
          aria-label="Play"
        >
          <Play size={26} fill="currentColor" className="ml-0.5" />
        </button>
      </div>
    </div>
  );
}
