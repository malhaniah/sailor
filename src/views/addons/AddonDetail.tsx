import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { ErrorNote, Spinner } from "../../components/ui";
import { errorMessage, marketplaceDetail } from "../../lib/ipc";
import { useAddons } from "../../lib/store";
import type { DirectorySource, MarketDetail } from "../../lib/types";
import { ListingAction } from "./BrowseTab";
import { AddonHeading, CapabilityBadges, MaskedUrl, SMALL_BUTTON, StarBadge, resourceLabel, resourceNames } from "./common";

const SOURCE_NAMES: Record<DirectorySource, string> = {
  stremioAddonsNet: "stremio-addons.net",
  stremioCommunity: "Stremio community catalog",
  stremioOfficial: "Stremio official catalog",
};

/** A directory listing's page (frame `addon-detail`). Everything here is the listing's
 * own, unreviewed data, shown as text. */
export function AddonDetailView({ listingKey }: { listingKey: string }) {
  const { addons } = useAddons();
  const [detail, setDetail] = useState<MarketDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  // re-read after an install so the action shows Installed
  useEffect(() => {
    let cancelled = false;
    marketplaceDetail(listingKey)
      .then((d) => !cancelled && setDetail(d))
      .catch((e) => !cancelled && setError(errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [listingKey, addons]);

  return (
    <main className="absolute inset-0 z-30 overflow-y-auto bg-canvas px-12 pb-20 pt-28">
      <div className="mx-auto flex max-w-3xl flex-col gap-6">
        {error && <ErrorNote message={error} />}
        {!detail && !error && <Spinner label="Loading…" />}
        {detail && <Detail detail={detail} />}
      </div>
    </main>
  );
}

function Detail({ detail }: { detail: MarketDetail }) {
  const { entry, documentation } = detail;
  const m = entry.manifest;
  const resources = resourceNames(m).map(resourceLabel);
  const facts: [string, ReactNode][] = [
    ["Version", m.version ?? "—"],
    ["Serves", resources.length ? resources.join(", ") : "—"],
    ["Types", m.types.length ? m.types.join(", ") : "—"],
    ["ID prefixes", m.idPrefixes?.length ? m.idPrefixes.join(", ") : "any"],
    ["Catalogs", m.catalogs.length ? String(m.catalogs.length) : "none"],
    ["P2P", m.behaviorHints.p2p ? "declared: your IP is visible to peers" : "not declared"],
    ["Listed by", entry.sources.map((s) => SOURCE_NAMES[s]).join(", ")],
  ];
  if (entry.categories.length) facts.push(["Categories", entry.categories.map((c) => c.name).join(", ")]);
  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-4">
        <AddonHeading manifest={m} size={64} />
        <div className="flex items-center gap-3">
          <StarBadge stars={entry.stars} listedAt={entry.listedAt} />
          <ListingAction entry={entry} />
        </div>
      </div>
      <p className="rounded-xl border border-edge-soft bg-canvas/40 px-4 py-3 text-[12.5px] leading-relaxed text-ink-muted">
        Not reviewed by Zeiler. This page shows what the listing says; installing fetches the
        addon itself and asks you to confirm what it really serves.
      </p>
      {m.description && (
        <p className="whitespace-pre-line text-[14px] leading-relaxed text-ink-muted">{m.description}</p>
      )}
      <CapabilityBadges manifest={m} adult={entry.adult} />
      <dl className="grid grid-cols-[140px_1fr] gap-x-6 gap-y-2 rounded-2xl border border-edge-soft bg-elevated/40 px-5 py-4 text-[13px]">
        {facts.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-ink-subtle">{label}</dt>
            <dd className="text-ink">{value}</dd>
          </div>
        ))}
        <dt className="text-ink-subtle">Address</dt>
        <dd className="min-w-0">
          <MaskedUrl url={entry.transportUrl} />
        </dd>
      </dl>
      {entry.siteUrl && (
        <button
          className={`${SMALL_BUTTON} w-fit`}
          onClick={() => void openUrl(entry.siteUrl!).catch(() => {})}
        >
          <ExternalLink size={14} /> View on stremio-addons.net
        </button>
      )}
      {documentation && (
        <section className="flex flex-col gap-2">
          <h2 className="text-[15px] font-medium text-ink">Documentation</h2>
          <p className="text-[12px] text-ink-subtle">
            From stremio-addons.net, shown as plain text.
          </p>
          <pre className="max-h-[480px] overflow-auto whitespace-pre-wrap break-words rounded-2xl border border-edge-soft bg-canvas/60 px-5 py-4 font-sans text-[13px] leading-relaxed text-ink-muted">
            {documentation}
          </pre>
        </section>
      )}
      <p className="text-[11.5px] text-ink-subtle">
        Stars and categories come from stremio-addons.net.
      </p>
    </>
  );
}
