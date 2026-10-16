import { LayoutGrid } from "lucide-react";
import { useMemo, useState } from "react";
import { CatalogRows } from "../components/CatalogRows";
import { GenreTiles } from "../components/GenreTiles";
import { EmptyState } from "../components/ui";
import { catalogTypes, typeLabel } from "../lib/catalogs";
import { roomCatalogs } from "../lib/rooms";
import { useCatalogRows } from "../lib/rows";
import { useAddons, useSettings } from "../lib/store";
import { useView } from "../lib/view";
import { LivePlaylists } from "./LivePlaylists";

/**
 * Live TV, in two tabs. Channels: the `tv` and `channel` catalogs of the addons the user
 * installed; a channel opens its Detail page, which plays a live meta's default video
 * through the stream picker. Playlists: the user's own IPTV sources (M3U, Xtream Codes),
 * played straight from the channel grid. Zeiler ships no channels.
 */
export function LiveView() {
  const { addons } = useAddons();
  const { settings } = useSettings();
  const { openMeta, openBrowse, setView } = useView();
  const showAdult = settings?.showAdult ?? false;
  const catalogs = useMemo(
    () => roomCatalogs(addons, "live", { hideAnime: false, showAdult, hideLiveTv: false }),
    [addons, showAdult],
  );
  const { rows, loadMore, request, version } = useCatalogRows(catalogs);
  const types = catalogTypes(catalogs);
  const [tab, setTab] = useState<"channels" | "playlists">("channels");

  return (
    <main className="relative h-full overflow-y-auto bg-canvas">
      <div className="relative mx-auto flex max-w-[1700px] flex-col gap-12 px-12 pb-32 pt-32">
        <header className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.42em] text-ink-subtle">
            Channels
          </span>
          <h1 className="font-display text-[44px] font-medium leading-[1.05] tracking-tight text-ink">
            Live TV
          </h1>
          <p className="max-w-2xl text-[15px] leading-relaxed text-ink-muted">
            Channels from the addons you installed and the playlists you add. Zeiler ships
            none of its own.
          </p>
          <div role="tablist" aria-label="Live TV" className="mt-4 flex gap-2">
            {(
              [
                ["channels", "Addon channels"],
                ["playlists", "Your playlists"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                role="tab"
                aria-selected={tab === id}
                onClick={() => setTab(id)}
                className={`h-10 rounded-full border px-5 text-[13.5px] font-medium transition-colors ${
                  tab === id
                    ? "border-ink bg-ink text-canvas"
                    : "border-edge-soft text-ink-muted hover:border-ink hover:text-ink"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </header>

        {tab === "playlists" ? (
          <LivePlaylists />
        ) : catalogs.length === 0 ? (
          <EmptyState title="No channels yet">
            None of your installed addons provides TV channels. Install one that does and
            its channels show up here, or add a playlist of your own.
            <div className="mt-5 flex justify-center gap-3">
              <button
                onClick={() => setTab("playlists")}
                className="h-11 rounded-full border border-edge px-6 text-[14px] font-semibold text-ink-muted transition-colors hover:border-ink hover:text-ink"
              >
                Your playlists
              </button>
              <button
                onClick={() => setView("addons")}
                className="h-11 rounded-full bg-ink px-6 text-[14px] font-semibold text-canvas transition-transform hover:scale-[1.03]"
              >
                Open Addons
              </button>
            </div>
          </EmptyState>
        ) : (
          <>
            <CatalogRows
              refs={catalogs}
              rows={rows}
              request={request}
              loadMore={loadMore}
              version={version}
              reserved={new Set()}
              onOpen={openMeta}
              kicker={(ref) => `${ref.addon.manifest.name} · ${typeLabel(ref.catalog.type)}`}
              dedup={false}
            />
            {/* many TV addons file channels by genre: their categories */}
            {types.map((type) => (
              <GenreTiles key={type} type={type} refs={catalogs} title={`${typeLabel(type)} by category`} />
            ))}
            <div className="flex justify-center">
              <button
                onClick={() =>
                  openBrowse({ type: types.includes("tv") ? "tv" : types[0], extra: [] })
                }
                className="flex h-10 items-center gap-2 rounded-full border border-edge-soft px-5 text-[13px] font-medium text-ink-muted transition-colors hover:border-ink hover:text-ink"
              >
                <LayoutGrid size={15} />
                Browse all channels
              </button>
            </div>
          </>
        )}
      </div>
    </main>
  );
}
