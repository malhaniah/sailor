import { useState } from "react";
import { AddonLogo } from "../components/cards";
import { Badge, EmptyState, ErrorNote, Modal, Spinner } from "../components/ui";
import { addonAdd, addonPreview, addonRemove, errorMessage } from "../lib/ipc";
import { useAddons } from "../lib/store";
import type { Descriptor, ManifestResource } from "../lib/types";

function resourceName(resource: ManifestResource): string {
  return typeof resource === "string" ? resource : resource.name;
}

export function AddonsView() {
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
    <main className="flex-1 overflow-y-auto px-12 pb-20 pt-28">
      <div className="mx-auto flex max-w-4xl flex-col gap-8">
        <header className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.42em] text-ink-subtle">
            Collections
          </span>
          <h1 className="font-display text-[44px] font-medium leading-[1.05] tracking-tight text-ink">
            Addons
          </h1>
          <p className="max-w-2xl text-[15px] leading-relaxed text-ink-muted">
            Zeiler ships no addons. Paste the manifest URL of an addon you trust — it ends in{" "}
            <code className="rounded-md bg-elevated px-1.5 py-0.5 font-mono text-[12.5px]">
              /manifest.json
            </code>{" "}
            (stremio:// links work too).
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
            className="h-12 w-full rounded-xl border border-edge bg-canvas px-4 text-[14px] text-ink outline-none placeholder:text-ink-subtle focus:border-ink"
          />
          <button
            type="submit"
            disabled={busy || !url.trim()}
            className="flex h-12 shrink-0 items-center rounded-xl bg-ink px-6 text-[13.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97] disabled:opacity-50"
          >
            {busy ? "Fetching…" : "Add addon"}
          </button>
        </form>

        {error && <ErrorNote message={error} />}

        {addons.length === 0 ? (
          <EmptyState title="No addons installed">
            Catalogs, metadata, streams and subtitles all come from addons you choose to
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
      </div>

      {preview && (
        <Modal title="Install this addon?" onClose={() => setPreview(null)} wide>
          <AddonSummary addon={preview} />
          <div className="mt-6 flex justify-end gap-3">
            <button
              className="flex h-10 items-center rounded-xl border border-edge-soft px-4 text-[12.5px] font-medium text-ink-muted transition-colors hover:border-edge hover:text-ink"
              onClick={() => setPreview(null)}
            >
              Cancel
            </button>
            <button
              className="flex h-10 items-center rounded-xl bg-ink px-5 text-[13.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97] disabled:opacity-50"
              disabled={installing}
              onClick={() => void install()}
            >
              {installing ? <Spinner /> : "Install"}
            </button>
          </div>
        </Modal>
      )}
    </main>
  );
}

function AddonSummary({ addon }: { addon: Descriptor }) {
  const m = addon.manifest;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-4">
        <AddonLogo name={m.name} logo={m.logo} size={48} />
        <div>
          <div className="text-[16px] font-semibold text-ink">
            {m.name}{" "}
            {m.version && <span className="text-[12px] font-normal text-ink-subtle">v{m.version}</span>}
          </div>
          <div className="text-[12px] text-ink-subtle">{m.id}</div>
        </div>
      </div>
      {m.description && (
        <p className="text-[13.5px] leading-relaxed text-ink-muted">{m.description}</p>
      )}
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
          {m.behaviorHints.p2p && <Badge tone="warn">P2P — your IP is visible to peers</Badge>}
          {m.behaviorHints.adult && <Badge tone="warn">adult content</Badge>}
          {m.behaviorHints.configurable && <Badge>configurable</Badge>}
        </div>
      )}
      <div className="break-all rounded-xl bg-canvas/60 px-3 py-2 font-mono text-[11.5px] text-ink-subtle">
        {addon.transportUrl}
      </div>
      <p className="text-[12px] leading-relaxed text-ink-subtle">
        Addons are third-party services. Zeiler only talks to this URL on your behalf; install
        it only if you trust its operator.
      </p>
    </div>
  );
}

function AddonCard({ addon, onRemove }: { addon: Descriptor; onRemove: () => Promise<void> }) {
  const [removing, setRemoving] = useState(false);
  const m = addon.manifest;
  return (
    <div className="flex items-center gap-4 rounded-2xl border border-edge-soft bg-elevated/40 px-5 py-4">
      <AddonLogo name={m.name} logo={m.logo} size={44} />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[14.5px] font-medium text-ink">
          {m.name}{" "}
          {m.version && <span className="text-[11.5px] font-normal text-ink-subtle">v{m.version}</span>}
        </div>
        <div className="truncate font-mono text-[11px] text-ink-subtle">{addon.transportUrl}</div>
      </div>
      <div className="hidden flex-wrap justify-end gap-1.5 md:flex">
        {[...new Set(m.resources.map(resourceName))].map((r) => (
          <Badge key={r}>{r}</Badge>
        ))}
        {m.behaviorHints.p2p && <Badge tone="warn">p2p</Badge>}
      </div>
      {addon.flags.protected ? (
        <span
          title="Metadata only. Turn it off in Settings → Content."
          className="shrink-0 rounded-full bg-accent/15 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wider text-accent"
        >
          Built-in
        </span>
      ) : (
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
      )}
    </div>
  );
}
