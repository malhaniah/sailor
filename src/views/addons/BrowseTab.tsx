import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { AddonLogo } from "../../components/cards";
import { ChipPicker, EmptyState, ErrorNote, Spinner, Toggle } from "../../components/ui";
import { errorMessage, marketplaceCategories, marketplaceList, marketplaceRefresh } from "../../lib/ipc";
import { requestInstall } from "../../lib/install-request";
import { useAddons, useSettings } from "../../lib/store";
import type {
  DirectoryCategory,
  DirectoryQuery,
  DirectorySort,
  MarketEntry,
  MarketPage,
} from "../../lib/types";
import { useView } from "../../lib/view";
import { AdultGate, CapabilityBadges, PRIMARY_BUTTON, SMALL_BUTTON, StarBadge } from "./common";

const PAGE_SIZE = 40;

const TYPES = ["", "movie", "series", "channel", "tv", "other"];
const TYPE_LABELS: Record<string, string> = {
  "": "All types",
  movie: "Movies",
  series: "Series",
  channel: "Channels",
  tv: "TV",
  other: "Other",
};
const RESOURCES = ["", "stream", "subtitles", "catalog", "meta"];
const RESOURCE_LABELS: Record<string, string> = {
  "": "Everything",
  stream: "Streams",
  subtitles: "Subtitles",
  catalog: "Catalogs",
  meta: "Metadata",
};
const SORTS: DirectorySort[] = ["stars", "newest", "name"];
const SORT_LABELS: Record<DirectorySort, string> = {
  stars: "Most starred",
  newest: "Newest",
  name: "A–Z",
};

export const DIRECTORY_NOTICE =
  "Listings come from stremio-addons.net and Stremio's public addon catalog. Zeiler doesn't review, rank or endorse them: install only what you trust.";

export function BrowseTab() {
  const { settings, save } = useSettings();
  if (!settings) return <Spinner label="Loading…" />;
  if (!settings.marketplaceEnabled) {
    return (
      <EmptyState title="Browse the addon directory">
        <span className="flex flex-col items-center gap-4">
          <span>
            {DIRECTORY_NOTICE} Turning this on lets Zeiler ask those directories for their
            listings, which shows them your IP address.
          </span>
          <button className={PRIMARY_BUTTON} onClick={() => void save({ marketplaceEnabled: true })}>
            Turn on the addon directory
          </button>
        </span>
      </EmptyState>
    );
  }
  return <Directory />;
}

function Directory() {
  const { settings, save } = useSettings();
  const { reload } = useAddons();
  const { openAddonDetail } = useView();
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [type, setType] = useState("");
  const [resource, setResource] = useState("");
  const [category, setCategory] = useState("");
  const [sort, setSort] = useState<DirectorySort>("stars");
  const [categories, setCategories] = useState<DirectoryCategory[]>([]);
  const [page, setPage] = useState<MarketPage | null>(null);
  const [entries, setEntries] = useState<MarketEntry[]>([]);
  const [pageNo, setPageNo] = useState(0);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [gate, setGate] = useState(false);
  const [refreshTick, setRefreshTick] = useState(0);
  const request = useRef(0);
  const showAdult = settings?.showAdult ?? false;
  const hideP2p = settings?.hideP2pAddons ?? false;

  useEffect(() => {
    const t = setTimeout(() => setDebounced(search), 250);
    return () => clearTimeout(t);
  }, [search]);

  useEffect(() => {
    marketplaceCategories()
      .then(setCategories)
      .catch(() => setCategories([]));
  }, [showAdult, refreshTick]);

  // a new query starts again at page 0
  useEffect(() => {
    setPageNo(0);
  }, [debounced, type, resource, category, sort, showAdult, hideP2p, refreshTick]);

  useEffect(() => {
    const query: DirectoryQuery = {
      search: debounced || null,
      types: type ? [type] : [],
      resources: resource ? [resource] : [],
      category: category || null,
      sort,
    };
    const id = ++request.current;
    setLoading(true);
    setError(null);
    marketplaceList(query, pageNo, PAGE_SIZE)
      .then((result) => {
        if (id !== request.current) return;
        setPage(result);
        setEntries((prev) => (pageNo === 0 ? result.entries : [...prev, ...result.entries]));
      })
      .catch((e) => {
        if (id === request.current) setError(errorMessage(e));
      })
      .finally(() => {
        if (id === request.current) setLoading(false);
      });
  }, [debounced, type, resource, category, sort, pageNo, showAdult, hideP2p, refreshTick]);

  const sourcesLine = page
    ? [
        page.sourcesOk.length === 0 && page.sourcesFailed.length > 0
          ? "Showing the last listings Zeiler fetched."
          : null,
        ...page.sourcesFailed.map((f) => f.message),
      ]
        .filter(Boolean)
        .join(" ")
    : "";

  return (
    <div className="flex flex-col gap-5">
      <div className="flex items-start justify-between gap-4 rounded-xl border border-edge-soft bg-canvas/40 px-4 py-3 text-[12.5px] leading-relaxed text-ink-muted">
        <span>
          Directory by stremio-addons.net and Stremio's community catalog · not reviewed by Zeiler.
          {sourcesLine && <span className="block text-[oklch(0.75_0.12_25)]">{sourcesLine}</span>}
        </span>
        <button
          className={SMALL_BUTTON}
          disabled={loading}
          onClick={async () => {
            setLoading(true);
            try {
              await marketplaceRefresh();
            } catch (e) {
              setError(errorMessage(e));
            }
            setRefreshTick((t) => t + 1);
          }}
        >
          <RefreshCw size={14} /> Refresh
        </button>
      </div>

      <input
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder="Search the directory"
        className="h-11 rounded-xl border border-edge bg-canvas px-4 text-[13.5px] text-ink outline-none placeholder:text-ink-subtle focus:border-ink"
      />
      <div className="flex flex-col gap-3">
        <ChipPicker options={RESOURCES} value={resource} onChange={setResource} format={(r) => RESOURCE_LABELS[r]} />
        <ChipPicker options={TYPES} value={type} onChange={setType} format={(t) => TYPE_LABELS[t]} />
        {categories.length > 0 && (
          <ChipPicker
            options={["", ...categories.map((c) => c.slug)]}
            value={category}
            onChange={setCategory}
            format={(slug) => (slug === "" ? "All categories" : (categories.find((c) => c.slug === slug)?.name ?? slug))}
          />
        )}
        <div className="flex flex-wrap items-center justify-between gap-4">
          <ChipPicker options={SORTS} value={sort} onChange={setSort} format={(s) => SORT_LABELS[s]} />
          <div className="flex items-center gap-5 text-[12.5px] text-ink-muted">
            <label className="flex items-center gap-2">
              <Toggle on={hideP2p} onChange={(on) => void save({ hideP2pAddons: on })} /> Hide P2P
            </label>
            <label className="flex items-center gap-2">
              <Toggle
                on={showAdult}
                onChange={(on) => (on ? setGate(true) : void save({ showAdult: false }).then(reload))}
              />{" "}
              Show adult
            </label>
          </div>
        </div>
      </div>

      {error && <ErrorNote message={error} />}

      {page && (
        <p className="text-[12px] text-ink-subtle">
          {page.total} {page.total === 1 ? "addon" : "addons"}
        </p>
      )}
      <div className="flex flex-col gap-3">
        {entries.map((entry) => (
          <DirectoryRow key={entry.key} entry={entry} onOpen={() => openAddonDetail(entry.key)} />
        ))}
      </div>
      {loading && <Spinner label="Loading the directory…" />}
      {!loading && page && entries.length === 0 && !error && (
        <EmptyState title="Nothing matches">Try another search or fewer filters.</EmptyState>
      )}
      {!loading && page?.hasMore && (
        <button className={`${SMALL_BUTTON} self-center`} onClick={() => setPageNo((n) => n + 1)}>
          Show more
        </button>
      )}

      {gate && (
        <AdultGate
          onClose={() => setGate(false)}
          onConfirm={() => {
            setGate(false);
            // the addon list Rust sends depends on it (as in Settings → Content)
            void save({ showAdult: true }).then(reload);
          }}
        />
      )}
    </div>
  );
}

export function installListing(entry: MarketEntry) {
  requestInstall({ url: entry.transportUrl, source: "directory", listingKey: entry.key });
}

/** The listing's install state against the addons in effect now (the page's own
 * `installState` is as of when it was fetched). */
function useInstallState(entry: MarketEntry): MarketEntry["installState"] {
  const { addons } = useAddons();
  const match = addons.find((a) => a.manifest.id === entry.key || a.transportUrl === entry.transportUrl);
  return match ? (match.flags.protected ? "builtIn" : "installed") : "available";
}

export function ListingAction({ entry }: { entry: MarketEntry }) {
  const state = useInstallState(entry);
  if (state !== "available") {
    return (
      <span className="shrink-0 rounded-full bg-raised px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wider text-ink-muted">
        {state === "builtIn" ? "Built-in" : "Installed"}
      </span>
    );
  }
  return (
    <button
      className={SMALL_BUTTON}
      onClick={(e) => {
        e.stopPropagation();
        installListing(entry);
      }}
    >
      {entry.configurationRequired ? "Set up…" : "Install"}
    </button>
  );
}

function DirectoryRow({ entry, onOpen }: { entry: MarketEntry; onOpen: () => void }) {
  const m = entry.manifest;
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (e.key === "Enter") onOpen();
      }}
      className="flex cursor-pointer items-center gap-4 rounded-2xl border border-edge-soft bg-elevated/40 px-5 py-4 text-left transition-colors hover:border-edge"
    >
      <AddonLogo name={m.name} logo={m.logo} size={44} />
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="truncate text-[14.5px] font-medium text-ink">{m.name}</span>
          {m.version && <span className="text-[11.5px] text-ink-subtle">v{m.version}</span>}
          <StarBadge stars={entry.stars} listedAt={entry.listedAt} />
        </div>
        {m.description && (
          <p className="line-clamp-2 text-[12.5px] leading-snug text-ink-muted">{m.description}</p>
        )}
        <CapabilityBadges manifest={m} adult={entry.adult} compact />
      </div>
      <ListingAction entry={entry} />
    </div>
  );
}
