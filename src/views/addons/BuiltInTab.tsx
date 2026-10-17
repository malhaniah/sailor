import { AddonLogo } from "../../components/cards";
import { EmptyState } from "../../components/ui";
import { useAddons } from "../../lib/store";
import { useView } from "../../lib/view";
import { CapabilityBadges } from "./common";

/** The built-ins in effect (metadata and subtitles only), switched in Settings. */
export function BuiltInTab() {
  const { addons } = useAddons();
  const { setView } = useView();
  const builtins = addons.filter((a) => a.flags.protected);
  return (
    <div className="flex flex-col gap-4">
      <p className="text-[12.5px] leading-relaxed text-ink-subtle">
        Built into Zeiler: metadata and subtitles only, never streams. Each one is asked only
        while it's on.{" "}
        <button className="font-semibold text-ink underline-offset-2 hover:underline" onClick={() => setView("settings")}>
          Turn them on or off in Settings → Content.
        </button>
      </p>
      {builtins.length === 0 ? (
        <EmptyState title="All built-ins are off">
          Turn them on again in Settings → Content.
        </EmptyState>
      ) : (
        builtins.map((addon) => (
          <div
            key={addon.transportUrl}
            className="flex items-center gap-4 rounded-2xl border border-edge-soft bg-elevated/40 px-5 py-4"
          >
            <AddonLogo name={addon.manifest.name} logo={addon.manifest.logo} size={44} />
            <div className="flex min-w-0 flex-1 flex-col gap-1.5">
              <div className="truncate text-[14.5px] font-medium text-ink">{addon.manifest.name}</div>
              <CapabilityBadges manifest={addon.manifest} compact />
            </div>
            <span className="shrink-0 rounded-full bg-accent/15 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wider text-accent">
              Built-in
            </span>
          </div>
        ))
      )}
    </div>
  );
}
