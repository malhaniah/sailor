import { Check, X } from "lucide-react";
import { CARD_SHADOW, SILK, posterRatio } from "./cards";
import { Poster, PosterProgress } from "./Poster";
import { resumeFraction } from "../lib/library";
import type { LibraryEntry } from "../lib/library-ipc";

/**
 * Types whose titles are watched episode by episode: their item's own "watched" (the
 * library's `timesWatched`, which one episode past 70% already sets) is no badge.
 */
const EPISODIC = new Set(["series", "anime"]);

/** Controls that show on hover are shown outright where nothing hovers (touch). */
export const NO_HOVER_SHOW = "[@media(hover:none)]:opacity-100";

/**
 * A library title as a poster card (PickCard's look, a Harbor recipe; MIT, see
 * THIRD-PARTY-NOTICES.md): the progress bar of its resume
 * point, a Watched badge (not for episodic types), a line under the name (`sub`), and
 * — with `onDismiss` — an ✕ that takes it out of Continue Watching. `pending` dims it
 * while it opens.
 */
export function LibraryCard({
  entry,
  sub,
  pending = false,
  onOpen,
  onDismiss,
}: {
  entry: LibraryEntry;
  sub?: string | null;
  pending?: boolean;
  onOpen: () => void;
  onDismiss?: () => void;
}) {
  const progress = resumeFraction(entry);
  return (
    <div className="group relative flex w-full min-w-0 flex-col gap-2.5">
      <button
        onClick={onOpen}
        disabled={pending}
        aria-busy={pending}
        className="flex min-w-0 flex-col gap-2.5 text-left disabled:cursor-progress"
      >
        <div
          className={`relative w-full transition-transform duration-300 ${SILK} group-hover:-translate-y-2`}
        >
          <Poster
            src={entry.poster}
            seed={entry.id}
            ratio={posterRatio(entry.posterShape)}
            className={`${CARD_SHADOW} ${pending ? "opacity-60" : ""}`}
          >
            {progress !== null && <PosterProgress value={progress} />}
          </Poster>
          {entry.watched && !EPISODIC.has(entry.type) && progress === null && (
            <span className="absolute left-2 top-2 flex items-center gap-1 rounded-md border border-edge-soft bg-canvas/95 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-ink">
              <Check size={11} strokeWidth={3} className="text-accent" />
              Watched
            </span>
          )}
          {pending && (
            <span className="absolute inset-0 flex items-center justify-center">
              <span className="h-6 w-6 animate-spin rounded-full border-2 border-edge border-t-accent" />
            </span>
          )}
        </div>
        <span className="flex min-w-0 flex-col gap-0.5">
          <span className="line-clamp-2 text-[13px] font-medium leading-snug text-ink">
            {entry.name}
          </span>
          {sub && <span className="truncate text-[12px] text-ink-subtle">{sub}</span>}
        </span>
      </button>
      {onDismiss && (
        <button
          onClick={onDismiss}
          aria-label={`Remove ${entry.name} from Continue Watching`}
          title="Remove from Continue Watching"
          className={`absolute right-1.5 top-1.5 flex h-7 w-7 items-center justify-center rounded-full border border-edge-soft bg-canvas/90 text-ink-muted opacity-0 shadow-[0_3px_8px_rgba(0,0,0,0.45)] transition-[opacity,translate,color] duration-300 ${SILK} hover:text-ink focus-visible:opacity-100 group-hover:-translate-y-2 group-hover:opacity-100 ${NO_HOVER_SHOW}`}
        >
          <X size={14} strokeWidth={2.4} />
        </button>
      )}
    </div>
  );
}
