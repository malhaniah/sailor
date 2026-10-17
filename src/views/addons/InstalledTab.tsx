import { ArrowDown, ArrowUp, RefreshCw, Settings2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { AddonLogo } from "../../components/cards";
import { Badge, EmptyState, ErrorNote } from "../../components/ui";
import {
  addonCheckUpdates,
  addonRefresh,
  addonRemove,
  addonReorder,
  errorKind,
  errorMessage,
} from "../../lib/ipc";
import { requestInstall } from "../../lib/install-request";
import { useAddons } from "../../lib/store";
import type { AddonUpdate, Descriptor } from "../../lib/types";
import { CapabilityBadges, MaskedUrl, SMALL_BUTTON } from "./common";

function configureUrlOf(addon: Descriptor): string | null {
  const hints = addon.manifest.behaviorHints;
  if (!hints.configurable && !hints.configurationRequired) return null;
  try {
    const url = new URL(addon.transportUrl);
    url.pathname = url.pathname.replace(/\/manifest\.json$/, "") + "/configure";
    return url.toString();
  } catch {
    return null;
  }
}

export function InstalledTab() {
  const { addons, reload } = useAddons();
  const user = useMemo(() => addons.filter((a) => !a.flags.protected), [addons]);
  const [search, setSearch] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [updates, setUpdates] = useState<AddonUpdate[]>([]);
  const [checking, setChecking] = useState(false);
  const [checked, setChecked] = useState(false);

  const check = useCallback(async (force: boolean) => {
    setChecking(true);
    try {
      setUpdates(await addonCheckUpdates(force));
      setChecked(true);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setChecking(false);
    }
  }, []);

  // the last check is reused for six hours in Rust (and forgets an addon once it is
  // installed, updated or removed), so asking again whenever the list changes is cheap
  const installedKey = user.map((a) => `${a.transportUrl} ${a.manifest.version ?? ""}`).join("\n");
  useEffect(() => {
    if (installedKey) void check(false);
    else setUpdates([]);
  }, [installedKey, check]);

  const needle = search.trim().toLowerCase();
  const shown = user.filter(
    (a) =>
      !needle ||
      a.manifest.name.toLowerCase().includes(needle) ||
      a.manifest.id.toLowerCase().includes(needle),
  );

  async function move(index: number, by: -1 | 1) {
    const order = user.map((a) => a.transportUrl);
    const to = index + by;
    if (to < 0 || to >= order.length) return;
    [order[index], order[to]] = [order[to], order[index]];
    setBusy(true);
    setError(null);
    try {
      await addonReorder(order);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      await reload();
      setBusy(false);
    }
  }

  async function applyUpdate(update: AddonUpdate) {
    setError(null);
    const review = () =>
      requestInstall({ url: update.transportUrl, source: "update", changes: update.changes });
    if (update.capabilitiesChanged) {
      review();
      return;
    }
    try {
      await addonRefresh(update.transportUrl);
      setUpdates((list) => list.filter((u) => u.transportUrl !== update.transportUrl));
      await reload();
    } catch (e) {
      // it changed more meanwhile: review it in the full confirmation
      if (errorKind(e) === "conflict") review();
      else setError(errorMessage(e));
    }
  }

  if (user.length === 0) {
    return (
      <EmptyState title="No addons installed">
        Streams and extra catalogs come from addons you choose to install: paste an addon's
        address above, or browse the directory.
      </EmptyState>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search installed addons"
          className="h-10 min-w-0 flex-1 rounded-xl border border-edge bg-canvas px-3 text-[13px] text-ink outline-none placeholder:text-ink-subtle focus:border-ink"
        />
        <button className={SMALL_BUTTON} disabled={checking} onClick={() => void check(true)}>
          <RefreshCw size={14} className={checking ? "animate-spin" : ""} />
          {checking ? "Checking…" : "Check for updates"}
        </button>
      </div>
      <p className="text-[12.5px] leading-relaxed text-ink-subtle">
        Order is priority: details come from the first addon that has them, and stream groups
        appear in this order. Built-in addons always come first.
        {checked && updates.length === 0 && " All up to date."}
      </p>
      {error && <ErrorNote message={error} />}
      <div className="flex flex-col gap-3">
        {shown.map((addon) => {
          const index = user.indexOf(addon);
          return (
            <InstalledRow
              key={addon.transportUrl}
              addon={addon}
              update={updates.find((u) => u.transportUrl === addon.transportUrl)}
              canMoveUp={!needle && index > 0 && !busy}
              canMoveDown={!needle && index < user.length - 1 && !busy}
              onMove={(by) => void move(index, by)}
              onUpdate={applyUpdate}
              onRemove={async () => {
                setError(null);
                try {
                  await addonRemove(addon.transportUrl);
                } catch (e) {
                  setError(errorMessage(e));
                }
                await reload();
              }}
            />
          );
        })}
      </div>
    </div>
  );
}

function InstalledRow({
  addon,
  update,
  canMoveUp,
  canMoveDown,
  onMove,
  onUpdate,
  onRemove,
}: {
  addon: Descriptor;
  update?: AddonUpdate;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onMove: (by: -1 | 1) => void;
  onUpdate: (update: AddonUpdate) => Promise<void>;
  onRemove: () => Promise<void>;
}) {
  const [removing, setRemoving] = useState(false);
  const [updating, setUpdating] = useState(false);
  const m = addon.manifest;
  const configureUrl = configureUrlOf(addon);
  return (
    <div className="flex flex-col gap-3 rounded-2xl border border-edge-soft bg-elevated/40 px-5 py-4">
      <div className="flex items-center gap-4">
        <div className="flex flex-col gap-1">
          <button
            aria-label={`Move ${m.name} up`}
            disabled={!canMoveUp}
            onClick={() => onMove(-1)}
            className="flex h-6 w-6 items-center justify-center rounded-md text-ink-subtle hover:bg-raised hover:text-ink disabled:opacity-30 disabled:hover:bg-transparent"
          >
            <ArrowUp size={14} />
          </button>
          <button
            aria-label={`Move ${m.name} down`}
            disabled={!canMoveDown}
            onClick={() => onMove(1)}
            className="flex h-6 w-6 items-center justify-center rounded-md text-ink-subtle hover:bg-raised hover:text-ink disabled:opacity-30 disabled:hover:bg-transparent"
          >
            <ArrowDown size={14} />
          </button>
        </div>
        <AddonLogo name={m.name} logo={m.logo} size={44} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 truncate text-[14.5px] font-medium text-ink">
            <span className="truncate">{m.name}</span>
            {m.version && <span className="text-[11.5px] font-normal text-ink-subtle">v{m.version}</span>}
            {update && (
              <Badge tone={update.requiresReview ? "warn" : "accent"}>
                {update.requiresReview ? "Update: review needed" : "Update available"}
              </Badge>
            )}
          </div>
          <MaskedUrl url={addon.transportUrl} />
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {update && (
            <button
              className={SMALL_BUTTON}
              disabled={updating}
              onClick={async () => {
                setUpdating(true);
                try {
                  await onUpdate(update);
                } finally {
                  setUpdating(false);
                }
              }}
            >
              {update.capabilitiesChanged ? "Review update" : "Update"}
            </button>
          )}
          {configureUrl && (
            <button
              className={SMALL_BUTTON}
              title="Set it up again on the addon's page; the new configuration replaces this one"
              onClick={() =>
                requestInstall({
                  url: "",
                  source: "reconfigure",
                  replace: addon.transportUrl,
                  name: m.name,
                  configureUrl,
                })
              }
            >
              <Settings2 size={14} /> Reconfigure
            </button>
          )}
          <button
            disabled={removing}
            onClick={async () => {
              setRemoving(true);
              try {
                await onRemove();
              } finally {
                setRemoving(false);
              }
            }}
            className="flex h-9 shrink-0 items-center rounded-lg border border-edge px-3.5 text-[12.5px] font-semibold text-ink-muted transition-colors hover:border-danger/40 hover:bg-danger/10 hover:text-[oklch(0.75_0.12_25)] disabled:opacity-50"
          >
            Remove
          </button>
        </div>
      </div>
      <div className="pl-[84px]">
        <CapabilityBadges manifest={m} compact />
        {update && update.changes.length > 0 && (
          <ul className="mt-2 list-disc pl-5 text-[12px] leading-relaxed text-ink-muted">
            {update.changes.map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
