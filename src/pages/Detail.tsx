import { useEffect, useMemo, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Badge, CopyField, ErrorNote, Modal, Spinner } from "../components/ui";
import {
  engineRemove,
  engineResolve,
  errorKind,
  errorMessage,
  metaGet,
  playerPlay,
  settingsGet,
  streamsGet,
} from "../lib/ipc";
import type { AddonStreams, MetaItem, Stream, Video } from "../lib/types";

export function DetailPage({ type, id }: { type: string; id: string }) {
  const [meta, setMeta] = useState<MetaItem | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [videoId, setVideoId] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setMeta(null);
    setError(null);
    setVideoId(null);
    metaGet(type, id)
      .then((m) => {
        if (cancelled) return;
        setMeta(m);
        // Live items (tv/EPG) always stream by the CHANNEL id, never a programme id.
        const live = m.behaviorHints.isLive || m.type === "tv";
        const defaultId =
          m.behaviorHints.defaultVideoId ?? (m.videos.length === 0 || live ? m.id : null);
        if (defaultId) setVideoId(defaultId);
      })
      .catch((e) => !cancelled && setError(errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [type, id]);

  if (error) {
    return (
      <div className="p-8">
        <BackButton />
        <div className="mt-4">
          <ErrorNote message={error} />
        </div>
      </div>
    );
  }
  if (!meta) {
    return (
      <div className="p-8">
        <BackButton />
        <div className="mt-6">
          <Spinner label="Loading details…" />
        </div>
      </div>
    );
  }

  return (
    <div className="relative">
      {meta.background && (
        <div
          className="pointer-events-none absolute inset-x-0 top-0 h-72 bg-cover bg-center opacity-20 [mask-image:linear-gradient(to_bottom,black,transparent)]"
          style={{ backgroundImage: `url(${JSON.stringify(meta.background)})` }}
        />
      )}
      <div className="relative flex flex-col gap-6 p-8">
        <BackButton />
        <header className="flex gap-6">
          {meta.poster && (
            <img
              src={meta.poster}
              alt=""
              className="h-56 w-auto shrink-0 rounded-xl object-cover shadow-lg"
            />
          )}
          <div className="flex min-w-0 flex-col gap-2">
            <h1 className="text-3xl font-bold">{meta.name}</h1>
            <div className="flex flex-wrap items-center gap-2 text-sm text-mist">
              {meta.releaseInfo && <span>{meta.releaseInfo}</span>}
              {meta.runtime && <span>· {meta.runtime}</span>}
              {meta.imdbRating && <span className="text-flare">· ★ {meta.imdbRating}</span>}
            </div>
            <div className="flex flex-wrap gap-1.5">
              {meta.genres.map((g) => (
                <Badge key={g}>{g}</Badge>
              ))}
            </div>
            {meta.description && (
              <p className="mt-1 max-w-2xl text-sm leading-relaxed text-mist">{meta.description}</p>
            )}
          </div>
        </header>

        {meta.videos.length > 0 && !(meta.behaviorHints.isLive || meta.type === "tv") && (
          <EpisodePicker meta={meta} selected={videoId} onSelect={setVideoId} />
        )}

        {videoId && (
          <StreamsPanel
            key={videoId}
            type={meta.type}
            videoId={videoId}
            title={meta.name}
            embedded={meta.videos.find((v) => v.id === videoId)?.streams ?? []}
          />
        )}
      </div>
    </div>
  );
}

function BackButton() {
  return (
    <button
      onClick={() => window.history.back()}
      className="w-fit rounded-lg bg-hull px-3 py-1.5 text-sm text-mist hover:bg-rig hover:text-foam"
    >
      ← Back
    </button>
  );
}

function EpisodePicker({
  meta,
  selected,
  onSelect,
}: {
  meta: MetaItem;
  selected: string | null;
  onSelect: (videoId: string) => void;
}) {
  const seasons = useMemo(() => {
    const unique = [...new Set(meta.videos.map((v) => v.season ?? -1))];
    // regular seasons ascending; specials (0) last; season-less entries first
    return unique.sort((a, b) => {
      const rank = (s: number) => (s === 0 ? Number.MAX_SAFE_INTEGER : s);
      return rank(a) - rank(b);
    });
  }, [meta.videos]);
  const [season, setSeason] = useState<number>(() => seasons.find((s) => s > 0) ?? seasons[0]);

  const episodes = useMemo(
    () =>
      meta.videos
        .filter((v) => (v.season ?? -1) === season)
        .sort((a, b) => (a.episode ?? 0) - (b.episode ?? 0)),
    [meta.videos, season],
  );

  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        {seasons.map((s) => (
          <button
            key={s}
            onClick={() => setSeason(s)}
            className={`rounded-lg px-3 py-1.5 text-sm font-medium ${
              s === season ? "bg-buoy text-abyss" : "bg-hull text-mist hover:bg-rig"
            }`}
          >
            {s === 0 ? "Specials" : s === -1 ? "Videos" : `Season ${s}`}
          </button>
        ))}
      </div>
      <div className="flex max-h-72 flex-col gap-1 overflow-y-auto pr-2">
        {episodes.map((video) => (
          <EpisodeRow
            key={video.id}
            video={video}
            active={selected === video.id}
            onClick={() => onSelect(video.id)}
          />
        ))}
      </div>
    </section>
  );
}

function EpisodeRow({
  video,
  active,
  onClick,
}: {
  video: Video;
  active: boolean;
  onClick: () => void;
}) {
  const parsed = video.released ? new Date(video.released) : null;
  // addon-supplied dates are arbitrary strings; an Invalid Date must never crash render
  const released = parsed && Number.isFinite(parsed.getTime()) ? parsed : null;
  const upcoming = released !== null && released.getTime() > Date.now();
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-3 rounded-lg px-3 py-2 text-left text-sm ${
        active ? "bg-hull text-foam" : "text-mist hover:bg-hull/60 hover:text-foam"
      } ${upcoming ? "opacity-50" : ""}`}
    >
      <span className="w-10 shrink-0 font-mono text-xs text-mist">
        {video.episode != null ? `E${String(video.episode).padStart(2, "0")}` : "—"}
      </span>
      <span className="min-w-0 flex-1 truncate">{video.title ?? video.id}</span>
      {upcoming && <Badge tone="warn">upcoming</Badge>}
      {released && !upcoming && (
        <span className="shrink-0 text-xs text-mist/70">{released.toISOString().slice(0, 10)}</span>
      )}
    </button>
  );
}

// --- streams ----------------------------------------------------------------------------

type PlayPhase =
  | { kind: "idle" }
  | { kind: "resolving"; label: string }
  | { kind: "playing"; infoHash?: string; streamUrl: string }
  | { kind: "error"; message: string; streamUrl?: string };

function StreamsPanel({
  type,
  videoId,
  title,
  embedded,
}: {
  type: string;
  videoId: string;
  title: string;
  embedded: Stream[];
}) {
  const [groups, setGroups] = useState<AddonStreams[] | null>(null);
  const [phase, setPhase] = useState<PlayPhase>({ kind: "idle" });

  useEffect(() => {
    let cancelled = false;
    setGroups(null);
    streamsGet(type, videoId).then((g) => !cancelled && setGroups(g));
    return () => {
      cancelled = true;
    };
  }, [type, videoId]);

  async function play(stream: Stream) {
    const hints = stream.behaviorHints;
    const headerEntries = Object.entries(hints.proxyHeaders?.request ?? {});
    // extract UA/Referer case-insensitively; forward the rest
    const uaEntry = headerEntries.find(([name]) => /^user-agent$/i.test(name));
    const referrerEntry = headerEntries.find(([name]) => /^referer$/i.test(name));
    const forwarded = headerEntries.filter((e) => e !== uaEntry && e !== referrerEntry);
    const mediaTitle = hints.filename ?? stream.name ?? title;
    try {
      if (stream.url) {
        setPhase({ kind: "resolving", label: "Starting player…" });
        await playerPlay({
          url: stream.url,
          title: mediaTitle,
          headers: forwarded as [string, string][],
          userAgent: uaEntry?.[1] ?? null,
          referrer: referrerEntry?.[1] ?? null,
        });
        setPhase({ kind: "playing", streamUrl: stream.url });
      } else if (stream.infoHash) {
        setPhase({ kind: "resolving", label: "Finding peers and fetching metadata…" });
        const resolved = await engineResolve({
          infoHash: stream.infoHash,
          fileIdx: stream.fileIdx ?? null,
          sources: stream.announce ?? [],
          filename: hints.filename ?? null,
          fileMustInclude: stream.fileMustInclude ?? [],
        });
        setPhase({ kind: "resolving", label: "Starting player…" });
        await playerPlay({ url: resolved.streamUrl, title: mediaTitle });
        setPhase({ kind: "playing", infoHash: resolved.infoHash, streamUrl: resolved.streamUrl });
      } else if (stream.externalUrl) {
        if (/^https?:\/\//i.test(stream.externalUrl)) await openUrl(stream.externalUrl);
        setPhase({ kind: "idle" });
      } else if (stream.ytId) {
        await openUrl(`https://www.youtube.com/watch?v=${encodeURIComponent(stream.ytId)}`);
        setPhase({ kind: "idle" });
      }
    } catch (e) {
      const message = errorMessage(e);
      // When mpv is missing, surface the stream URL so the user can play it elsewhere.
      const streamUrl =
        errorKind(e) === "player" && stream.url ? stream.url : undefined;
      setPhase({ kind: "error", message, streamUrl });
    }
  }

  async function stopTorrent(infoHash: string) {
    try {
      const settings = await settingsGet();
      await engineRemove(infoHash, settings.deleteOnStop);
    } catch {
      /* best effort */
    }
    setPhase({ kind: "idle" });
  }

  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold uppercase tracking-wider text-mist">
        Streams <span className="font-normal normal-case">for {videoId}</span>
      </h2>

      {embedded.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <div className="text-xs font-semibold text-mist">Embedded</div>
          {embedded.map((stream, i) => (
            <StreamRow key={`embedded-${i}`} stream={stream} onPlay={() => void play(stream)} />
          ))}
        </div>
      )}

      {groups === null && <Spinner label="Asking your addons for streams…" />}

      {groups !== null && groups.length === 0 && embedded.length === 0 && (
        <ErrorNote message="None of your installed addons provides streams for this item. Install a stream addon (Addons page) to play content." />
      )}

      {groups?.map((group) => (
        <div key={group.transportUrl} className="flex flex-col gap-1.5">
          <div className="flex items-center gap-2 text-xs font-semibold text-mist">
            {group.addonName}
            {group.error && <Badge tone="warn">{group.error}</Badge>}
          </div>
          {group.streams.map((stream, i) => (
            <StreamRow key={i} stream={stream} onPlay={() => void play(stream)} />
          ))}
        </div>
      ))}

      {phase.kind === "resolving" && (
        <Modal title="Preparing stream" onClose={() => setPhase({ kind: "idle" })}>
          <Spinner label={phase.label} />
          <p className="mt-3 text-xs text-mist">
            Torrent streams can take up to a minute to find peers on a cold start.
          </p>
        </Modal>
      )}

      {phase.kind === "playing" && (
        <Modal title="Playing in mpv" onClose={() => setPhase({ kind: "idle" })}>
          <p className="text-sm text-mist">
            Playback was handed to the external player. You can keep browsing.
          </p>
          {phase.infoHash && (
            <button
              className="mt-4 rounded-lg bg-coral/20 px-4 py-2 text-sm font-medium text-coral hover:bg-coral/30"
              onClick={() => void stopTorrent(phase.infoHash!)}
            >
              Stop torrent &amp; clean up
            </button>
          )}
        </Modal>
      )}

      {phase.kind === "error" && (
        <Modal title="Could not play" onClose={() => setPhase({ kind: "idle" })}>
          <ErrorNote message={phase.message} />
          {phase.streamUrl && (
            <div className="mt-4 flex flex-col gap-2">
              <div className="text-xs text-mist">Stream URL (play it in any player):</div>
              <CopyField value={phase.streamUrl} />
            </div>
          )}
        </Modal>
      )}
    </section>
  );
}

function StreamRow({ stream, onPlay }: { stream: Stream; onPlay: () => void }) {
  const hints = stream.behaviorHints;
  const kind = stream.url
    ? { label: "direct", tone: "good" as const }
    : stream.infoHash
      ? { label: "torrent", tone: "warn" as const }
      : stream.ytId
        ? { label: "youtube", tone: "default" as const }
        : stream.externalUrl
          ? { label: "external", tone: "default" as const }
          : { label: "unsupported", tone: "default" as const };
  const unsupported = kind.label === "unsupported";
  const size = hints.videoSize ? `${(hints.videoSize / 1e9).toFixed(2)} GB` : null;

  return (
    <button
      disabled={unsupported}
      onClick={onPlay}
      className="flex items-center gap-3 rounded-lg border border-rig/50 bg-hull/50 px-3 py-2 text-left text-sm hover:border-buoy/50 hover:bg-hull disabled:opacity-40"
    >
      <Badge tone={kind.tone}>{kind.label}</Badge>
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium text-foam">{stream.name ?? "Stream"}</div>
        {stream.description && (
          <div className="truncate text-xs text-mist">{stream.description}</div>
        )}
      </div>
      {size && <span className="shrink-0 text-xs text-mist">{size}</span>}
    </button>
  );
}
