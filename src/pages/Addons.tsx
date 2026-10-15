import { useState } from "react";
import { useAddons } from "../App";
import { Badge, EmptyState, ErrorNote, Modal, Spinner } from "../components/ui";
import { addonAdd, addonPreview, addonRemove, errorMessage } from "../lib/ipc";
import type { Descriptor, ManifestResource } from "../lib/types";

function resourceName(resource: ManifestResource): string {
  return typeof resource === "string" ? resource : resource.name;
}

export function AddonsPage() {
  const { addons, reload } = useAddons();
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<Descriptor | null>(null);
  const [installing, setInstalling] = useState(false);

  async function fetchPreview() {
    const input = url.trim();
    if (!input) return;
    setBusy(true);
    setError(null);
    try {
      setPreview(await addonPreview(input));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function install() {
    if (!preview) return;
    setInstalling(true);
    setError(null);
    try {
      await addonAdd(preview.transportUrl);
      await reload();
      setPreview(null);
      setUrl("");
    } catch (e) {
      setError(errorMessage(e));
      setPreview(null);
    } finally {
      setInstalling(false);
    }
  }

  return (
    <div className="flex flex-col gap-6 p-8">
      <header className="flex flex-col gap-1">
        <h1 className="text-2xl font-bold">Addons</h1>
        <p className="text-sm text-mist">
          Sailor ships no addons. Paste the manifest URL of an addon you trust —
          it ends in <code className="rounded bg-hull px-1">/manifest.json</code> (stremio://
          links work too).
        </p>
      </header>

      <form
        className="flex gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          void fetchPreview();
        }}
      >
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://example.com/manifest.json"
          className="w-full max-w-2xl rounded-lg border border-rig bg-hull px-4 py-2.5 text-sm placeholder:text-mist/60 focus:border-buoy focus:outline-none"
        />
        <button
          type="submit"
          disabled={busy || !url.trim()}
          className="rounded-lg bg-buoy px-5 py-2.5 text-sm font-semibold text-abyss hover:bg-buoy/80 disabled:opacity-50"
        >
          {busy ? "Fetching…" : "Add"}
        </button>
      </form>

      {error && <ErrorNote message={error} />}

      {addons.length === 0 ? (
        <EmptyState title="No addons installed">
          Your catalogs, metadata, streams and subtitles all come from addons you choose to
          install.
        </EmptyState>
      ) : (
        <div className="flex flex-col gap-3">
          {addons.map((addon) => (
            <AddonCard
              key={addon.transportUrl}
              addon={addon}
              onRemove={async () => {
                await addonRemove(addon.transportUrl);
                await reload();
              }}
            />
          ))}
        </div>
      )}

      {preview && (
        <Modal title="Install this addon?" onClose={() => setPreview(null)}>
          <AddonSummary addon={preview} />
          <div className="mt-5 flex justify-end gap-3">
            <button
              className="rounded-lg bg-hull px-4 py-2 text-sm text-mist hover:bg-rig"
              onClick={() => setPreview(null)}
            >
              Cancel
            </button>
            <button
              className="rounded-lg bg-buoy px-4 py-2 text-sm font-semibold text-abyss hover:bg-buoy/80 disabled:opacity-50"
              disabled={installing}
              onClick={() => void install()}
            >
              {installing ? <Spinner /> : "Install"}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

function AddonSummary({ addon }: { addon: Descriptor }) {
  const m = addon.manifest;
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3">
        {m.logo ? (
          <img src={m.logo} alt="" className="h-12 w-12 rounded-lg object-contain" />
        ) : (
          <div className="flex h-12 w-12 items-center justify-center rounded-lg bg-hull text-xl">
            🧩
          </div>
        )}
        <div>
          <div className="font-semibold">
            {m.name} {m.version && <span className="text-xs text-mist">v{m.version}</span>}
          </div>
          <div className="text-xs text-mist">{m.id}</div>
        </div>
      </div>
      {m.description && <p className="text-sm text-mist">{m.description}</p>}
      <div className="flex flex-wrap gap-1.5">
        {[...new Set(m.resources.map(resourceName))].map((r) => (
          <Badge key={r} tone="good">
            {r}
          </Badge>
        ))}
        {m.types.map((t) => (
          <Badge key={t}>{t}</Badge>
        ))}
        {m.catalogs.length > 0 && <Badge>{m.catalogs.length} catalogs</Badge>}
      </div>
      {(m.behaviorHints.p2p || m.behaviorHints.adult || m.behaviorHints.configurable) && (
        <div className="flex flex-wrap gap-1.5">
          {m.behaviorHints.p2p && (
            <Badge tone="warn">P2P — your IP address is visible to peers</Badge>
          )}
          {m.behaviorHints.adult && <Badge tone="warn">adult content</Badge>}
          {m.behaviorHints.configurable && <Badge>configurable</Badge>}
        </div>
      )}
      <div className="break-all rounded-lg bg-hull px-3 py-2 text-xs text-mist">
        {addon.transportUrl}
      </div>
      <p className="text-xs text-mist/70">
        Addons are third-party services. Sailor only talks to this URL on your behalf; install
        it only if you trust its operator.
      </p>
    </div>
  );
}

function AddonCard({ addon, onRemove }: { addon: Descriptor; onRemove: () => Promise<void> }) {
  const [removing, setRemoving] = useState(false);
  const m = addon.manifest;
  return (
    <div className="flex items-center gap-4 rounded-xl border border-rig/50 bg-deep px-4 py-3">
      {m.logo ? (
        <img src={m.logo} alt="" className="h-10 w-10 rounded-lg object-contain" />
      ) : (
        <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-hull">🧩</div>
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">
          {m.name} {m.version && <span className="text-xs text-mist">v{m.version}</span>}
        </div>
        <div className="truncate text-xs text-mist">{addon.transportUrl}</div>
      </div>
      <div className="hidden flex-wrap gap-1.5 md:flex">
        {[...new Set(m.resources.map(resourceName))].map((r) => (
          <Badge key={r}>{r}</Badge>
        ))}
        {m.behaviorHints.p2p && <Badge tone="warn">p2p</Badge>}
      </div>
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
        className="shrink-0 rounded-lg bg-coral/15 px-3 py-1.5 text-sm font-medium text-coral hover:bg-coral/25 disabled:opacity-50"
      >
        Remove
      </button>
    </div>
  );
}
