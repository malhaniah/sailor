import { Eye, EyeOff, Star } from "lucide-react";
import { useState } from "react";
import { AddonLogo } from "../../components/cards";
import { Badge, Modal } from "../../components/ui";
import type { Manifest, ManifestResource } from "../../lib/types";

export function resourceName(resource: ManifestResource): string {
  return typeof resource === "string" ? resource : resource.name;
}

const RESOURCE_LABELS: Record<string, string> = {
  stream: "streams",
  subtitles: "subtitles",
  catalog: "catalogs",
  meta: "details",
  addon_catalog: "addon catalogs",
};

export function resourceLabel(name: string): string {
  return RESOURCE_LABELS[name] ?? name;
}

/** Resource names a manifest serves, with `catalog` when it lists catalogs. */
export function resourceNames(m: Manifest): string[] {
  const names = new Set(m.resources.map(resourceName));
  if (m.catalogs.length > 0) names.add("catalog");
  return [...names];
}

/** What the UI shows of a transport URL by default (Rust `mask_transport_url` does the
 * same): host and the manifest suffix, without the path or query where configured
 * addons keep their keys. */
export function maskUrl(raw: string): string {
  try {
    const url = new URL(raw);
    const middle = url.pathname !== "/manifest.json" || url.search !== "";
    return `${url.protocol}//${url.host}${middle ? "/…" : ""}/manifest.json`;
  } catch {
    return "…";
  }
}

/** A transport URL, masked until the user reveals it. */
export function MaskedUrl({ url, masked }: { url: string; masked?: string }) {
  const [shown, setShown] = useState(false);
  return (
    <div className="flex min-w-0 items-center gap-2">
      <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-ink-subtle" title={shown ? url : undefined}>
        {shown ? url : (masked ?? maskUrl(url))}
      </span>
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          setShown((s) => !s);
        }}
        aria-label={shown ? "Hide the full address" : "Show the full address"}
        title={shown ? "Hide" : "Reveal: configured addresses can contain your keys"}
        className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-ink-subtle hover:bg-raised hover:text-ink"
      >
        {shown ? <EyeOff size={13} /> : <Eye size={13} />}
      </button>
    </div>
  );
}

export function StarBadge({ stars, listedAt }: { stars: number | null; listedAt: string | null }) {
  const listed = listedAt ? Date.parse(listedAt) : NaN;
  const isNew = !Number.isNaN(listed) && Date.now() - listed < 14 * 24 * 60 * 60 * 1000;
  return (
    <span className="inline-flex items-center gap-1.5">
      {stars !== null && (
        <span
          title="Stars on stremio-addons.net"
          className="inline-flex items-center gap-1 text-[11.5px] font-medium text-ink-muted"
        >
          <Star size={12} className="fill-current" /> {stars}
        </span>
      )}
      {isNew && <Badge tone="accent">New</Badge>}
    </span>
  );
}

/** The capability badges every addon view shares. */
export function CapabilityBadges({
  manifest,
  adult,
  compact = false,
}: {
  manifest: Manifest;
  adult?: boolean;
  compact?: boolean;
}) {
  const hints = manifest.behaviorHints;
  return (
    <div className="flex flex-wrap gap-1.5">
      {resourceNames(manifest).map((r) => (
        <Badge key={r} tone={r === "stream" ? "default" : "good"}>
          {resourceLabel(r)}
        </Badge>
      ))}
      {hints.p2p && (
        <Badge tone="warn">{compact ? "P2P" : "P2P (declared): your IP is visible to peers"}</Badge>
      )}
      {(adult || hints.adult) && <Badge tone="warn">adult</Badge>}
      {hints.configurationRequired ? (
        <Badge>needs setup</Badge>
      ) : (
        hints.configurable && <Badge>configurable</Badge>
      )}
    </div>
  );
}

export function AddonHeading({ manifest, size = 48 }: { manifest: Manifest; size?: number }) {
  return (
    <div className="flex min-w-0 items-center gap-4">
      <AddonLogo name={manifest.name} logo={manifest.logo} size={size} />
      <div className="min-w-0">
        <div className="truncate text-[16px] font-semibold text-ink">
          {manifest.name}{" "}
          {manifest.version && (
            <span className="text-[12px] font-normal text-ink-subtle">v{manifest.version}</span>
          )}
        </div>
        <div className="truncate text-[12px] text-ink-subtle">{manifest.id}</div>
      </div>
    </div>
  );
}

/** The 18+ confirmation in front of the single adult-content setting. */
export function AdultGate({ onConfirm, onClose }: { onConfirm: () => void; onClose: () => void }) {
  return (
    <Modal title="Show adult content?" onClose={onClose}>
      <p className="text-[13.5px] leading-relaxed text-ink-muted">
        Adult addons stay hidden unless you turn this on. Only continue if you are 18 or older
        and adult content is legal where you are. Detection is automatic and imperfect, so some
        listings may be missed or mislabelled. You can turn it off again in Settings → Content.
      </p>
      <div className="mt-6 flex justify-end gap-3">
        <button className={SECONDARY_BUTTON} onClick={onClose}>
          Cancel
        </button>
        <button className={PRIMARY_BUTTON} onClick={onConfirm}>
          I'm 18 or older
        </button>
      </div>
    </Modal>
  );
}

export const PRIMARY_BUTTON =
  "flex h-10 items-center justify-center gap-2 rounded-xl bg-ink px-5 text-[13.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97] disabled:opacity-50 disabled:hover:scale-100";
export const SECONDARY_BUTTON =
  "flex h-10 items-center justify-center gap-2 rounded-xl border border-edge-soft px-4 text-[12.5px] font-medium text-ink-muted transition-colors hover:border-edge hover:text-ink disabled:opacity-50";
export const SMALL_BUTTON =
  "flex h-9 shrink-0 items-center gap-1.5 rounded-lg border border-edge px-3 text-[12.5px] font-semibold text-ink-muted transition-colors hover:border-ink/40 hover:text-ink disabled:opacity-50";
