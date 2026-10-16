import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ClipboardPaste, ExternalLink, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { ErrorNote, Modal, Spinner } from "../../components/ui";
import {
  STREMIO_LINK,
  addonAdd,
  addonPreview,
  deeplinkTakePending,
  errorMessage,
} from "../../lib/ipc";
import {
  type InstallRequest,
  closeInstall,
  requestInstall,
  useInstallRequest,
} from "../../lib/install-request";
import { useAddons } from "../../lib/store";
import type { InstallPreview } from "../../lib/types";
import {
  AddonHeading,
  CapabilityBadges,
  MaskedUrl,
  PRIMARY_BUTTON,
  SECONDARY_BUTTON,
} from "./common";

/** The install confirmation, mounted once in the shell, plus the stremio:// link
 * listener that feeds it. */
export function InstallHost() {
  const request = useInstallRequest();

  useEffect(() => {
    // a link that started the app arrived before this listener existed (taken once:
    // the request store is global, so a remount in between doesn't lose it)
    deeplinkTakePending()
      .then((url) => {
        if (url) requestInstall({ url, source: "link" });
      })
      .catch(() => {});
    const unlisten = listen<string>(STREMIO_LINK, (e) => {
      requestInstall({ url: e.payload, source: "link" });
    });
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  if (!request) return null;
  // keyed by the request: a new one starts from scratch
  return <InstallDialog key={`${request.source}|${request.url}|${request.replace ?? ""}`} request={request} />;
}

const SOURCE_LINE: Record<InstallRequest["source"], string> = {
  pasted: "Pasted by you.",
  directory: "From the addon directory: listed by others, not reviewed by Zeiler.",
  link: "Opened from a stremio:// link. Only install it if you just clicked that link yourself.",
  update: "A newer version of an addon you installed.",
  reconfigure: "A new configuration of an addon you installed.",
};

type Phase =
  | { kind: "setup" }
  | { kind: "loading" }
  | { kind: "preview"; preview: InstallPreview }
  | { kind: "failed"; message: string };

function InstallDialog({ request }: { request: InstallRequest }) {
  const { reload } = useAddons();
  const [phase, setPhase] = useState<Phase>(request.url ? { kind: "loading" } : { kind: "setup" });
  // the setup page and the addon being set up, from the request or the first preview
  const [setup, setSetup] = useState<{ configureUrl: string | null; name: string | null }>({
    configureUrl: request.configureUrl ?? null,
    name: request.name ?? null,
  });
  const [pasted, setPasted] = useState("");
  const [replace, setReplace] = useState<string | null>(request.replace ?? null);
  const [p2pOk, setP2pOk] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // the link came back from the addon's setup page: it is configured, not the default
  const [configured, setConfigured] = useState(false);

  async function preview(url: string, listingKey?: string) {
    setPhase({ kind: "loading" });
    setError(null);
    try {
      const result = await addonPreview(url, listingKey);
      const hints = result.descriptor.manifest.behaviorHints;
      setSetup((s) => ({
        configureUrl: result.configureUrl ?? s.configureUrl,
        name: s.name ?? result.descriptor.manifest.name,
      }));
      if (hints.configurationRequired) {
        // the listing or link points at the unconfigured addon: set it up first
        setPhase({ kind: "setup" });
        return;
      }
      if (!request.replace) {
        const existing = result.existing;
        // a reconfigured copy (same id) replaces the old one unless the user unticks it
        setReplace(existing?.kind === "sameId" ? existing.transportUrl : null);
      }
      setP2pOk(false);
      setPhase({ kind: "preview", preview: result });
    } catch (e) {
      setPhase({ kind: "failed", message: errorMessage(e) });
    }
  }

  useEffect(() => {
    // once per request: the component is keyed by it
    if (request.url) void preview(request.url, request.listingKey);
  }, []);

  async function install(p: InstallPreview) {
    setInstalling(true);
    setError(null);
    try {
      const target = p.existing?.kind === "sameUrl" ? null : replace;
      await addonAdd(p.descriptor.transportUrl, target);
      await reload();
      closeInstall();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setInstalling(false);
    }
  }

  const title =
    phase.kind === "setup"
      ? `Set up ${setup.name ?? "this addon"}`
      : phase.kind === "preview" && phase.preview.existing?.kind === "sameUrl"
        ? "Update this addon?"
        : "Install this addon?";

  return (
    <Modal title={title} onClose={() => !installing && closeInstall()} wide>
      {phase.kind === "loading" && (
        <div className="py-6">
          <Spinner label="Fetching the addon's manifest…" />
        </div>
      )}

      {phase.kind === "failed" && (
        <div className="flex flex-col gap-4">
          <ErrorNote message={phase.message} />
          <div className="flex justify-end">
            <button className={SECONDARY_BUTTON} onClick={closeInstall}>
              Close
            </button>
          </div>
        </div>
      )}

      {phase.kind === "setup" && (
        <SetupStep
          configureUrl={setup.configureUrl}
          pasted={pasted}
          setPasted={setPasted}
          onSubmit={(url) => {
            setConfigured(true);
            void preview(url);
          }}
          onCancel={closeInstall}
        />
      )}

      {phase.kind === "preview" && (
        <PreviewStep
          request={request}
          configured={configured}
          preview={phase.preview}
          replace={replace}
          setReplace={setReplace}
          p2pOk={p2pOk}
          setP2pOk={setP2pOk}
          installing={installing}
          error={error}
          onConfigure={() => setPhase({ kind: "setup" })}
          onInstall={() => void install(phase.preview)}
        />
      )}
    </Modal>
  );
}

function SetupStep({
  configureUrl,
  pasted,
  setPasted,
  onSubmit,
  onCancel,
}: {
  configureUrl: string | null;
  pasted: string;
  setPasted: (s: string) => void;
  onSubmit: (url: string) => void;
  onCancel: () => void;
}) {
  const [clipboardError, setClipboardError] = useState<string | null>(null);
  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        if (pasted.trim()) onSubmit(pasted.trim());
      }}
    >
      <ol className="flex list-decimal flex-col gap-2 pl-5 text-[13.5px] leading-relaxed text-ink-muted">
        <li>
          Open the addon's setup page in your browser and choose your options there.
          {configureUrl && (
            <button
              type="button"
              onClick={() => void openUrl(configureUrl).catch(() => {})}
              className="mt-2 flex items-center gap-2 rounded-lg border border-edge px-3 py-1.5 text-[12.5px] font-semibold text-ink hover:border-ink/40"
            >
              <ExternalLink size={14} /> Open setup page
            </button>
          )}
        </li>
        <li>
          Copy the install link it gives you (a stremio:// link or one ending in
          /manifest.json) and paste it here. It goes through this confirmation before anything
          is installed.
        </li>
      </ol>
      <div className="flex gap-2">
        <input
          autoFocus
          value={pasted}
          onChange={(e) => setPasted(e.target.value)}
          placeholder="stremio://… or https://…/manifest.json"
          className="h-11 w-full rounded-xl border border-edge bg-canvas px-3 font-mono text-[12px] text-ink outline-none placeholder:text-ink-subtle focus:border-ink"
        />
        <button
          type="button"
          className={SECONDARY_BUTTON}
          onClick={async () => {
            setClipboardError(null);
            try {
              const text = (await navigator.clipboard.readText()).trim();
              if (text) {
                setPasted(text);
                onSubmit(text);
              }
            } catch {
              setClipboardError("Zeiler can't read the clipboard here: paste the link into the field.");
            }
          }}
        >
          <ClipboardPaste size={14} /> From clipboard
        </button>
      </div>
      {clipboardError && <p className="text-[12.5px] text-ink-muted">{clipboardError}</p>}
      <div className="flex justify-end gap-3">
        <button type="button" className={SECONDARY_BUTTON} onClick={onCancel}>
          Cancel
        </button>
        <button type="submit" className={PRIMARY_BUTTON} disabled={!pasted.trim()}>
          Continue
        </button>
      </div>
    </form>
  );
}

function PreviewStep({
  request,
  configured,
  preview,
  replace,
  setReplace,
  p2pOk,
  setP2pOk,
  installing,
  error,
  onConfigure,
  onInstall,
}: {
  request: InstallRequest;
  configured: boolean;
  preview: InstallPreview;
  replace: string | null;
  setReplace: (url: string | null) => void;
  p2pOk: boolean;
  setP2pOk: (ok: boolean) => void;
  installing: boolean;
  error: string | null;
  onConfigure: () => void;
  onInstall: () => void;
}) {
  const m = preview.descriptor.manifest;
  const hints = m.behaviorHints;
  const existing = preview.existing;
  const blocked = hints.p2p && !p2pOk;
  return (
    <div className="flex flex-col gap-4">
      <AddonHeading manifest={m} />
      {m.description && (
        <p className="whitespace-pre-line text-[13.5px] leading-relaxed text-ink-muted">{m.description}</p>
      )}
      <CapabilityBadges manifest={m} />
      {m.types.length > 0 && (
        <p className="text-[12.5px] text-ink-subtle">
          Types: {m.types.join(", ")}
          {m.catalogs.length > 0 && ` · ${m.catalogs.length} catalogs`}
        </p>
      )}

      {request.changes && request.changes.length > 0 && (
        <div className="rounded-xl border border-edge-soft bg-canvas/40 px-4 py-3 text-[12.5px] leading-relaxed text-ink-muted">
          <div className="mb-1 font-semibold text-ink">What changed</div>
          <ul className="list-disc pl-5">
            {request.changes.map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
        </div>
      )}

      {preview.listingChanges.length > 0 && (
        <div className="rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-[12.5px] leading-relaxed text-ink">
          <div className="mb-1 flex items-center gap-2 font-semibold">
            <TriangleAlert size={14} />
            {preview.listingRequiresReview
              ? "The addon differs from its listing in ways that matter"
              : "The addon differs from its listing"}
          </div>
          <ul className="list-disc pl-5">
            {preview.listingChanges.map((c) => (
              <li key={c}>{c}</li>
            ))}
          </ul>
        </div>
      )}

      {request.replace ? (
        <p className="text-[12.5px] text-ink-muted">
          Replaces {request.name ?? "the addon"} where it is in your list.
        </p>
      ) : existing?.kind === "sameUrl" ? (
        <p className="text-[12.5px] text-ink-muted">
          You have this addon installed already; installing updates it in place.
        </p>
      ) : (
        existing && (
          <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-edge-soft bg-canvas/40 px-4 py-3 text-[12.5px] leading-relaxed text-ink-muted">
            <input
              type="checkbox"
              className="mt-0.5"
              checked={replace === existing.transportUrl}
              onChange={(e) => setReplace(e.target.checked ? existing.transportUrl : null)}
            />
            <span>
              Replace <span className="font-semibold text-ink">{existing.name}</span> (
              <span className="font-mono">{existing.maskedUrl}</span>), keeping its place in your
              list.{" "}
              {existing.kind === "sameId"
                ? "It is the same addon with another configuration."
                : "It comes from the same site."}
            </span>
          </label>
        )
      )}

      {hints.p2p && (
        <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-danger/30 bg-danger/10 px-4 py-3 text-[12.5px] leading-relaxed text-ink">
          <input type="checkbox" className="mt-0.5" checked={p2pOk} onChange={(e) => setP2pOk(e.target.checked)} />
          <span>
            This addon says it uses peer-to-peer (BitTorrent) streams: while you watch, your IP
            address is visible to other peers. I understand.
          </span>
        </label>
      )}

      <div className="rounded-xl bg-canvas/60 px-3 py-2">
        <MaskedUrl url={preview.descriptor.transportUrl} masked={preview.maskedUrl} />
      </div>
      <p className="text-[12px] leading-relaxed text-ink-subtle">
        {SOURCE_LINE[request.source]} Addons are third-party services: Zeiler only talks to this
        address on your behalf. Install it only if you trust its operator.
      </p>

      {error && <ErrorNote message={error} />}

      <div className="mt-2 flex flex-wrap justify-end gap-3">
        <button className={SECONDARY_BUTTON} onClick={closeInstall} disabled={installing}>
          Cancel
        </button>
        {hints.configurable && preview.configureUrl && (
          <button className={SECONDARY_BUTTON} onClick={onConfigure} disabled={installing}>
            Configure…
          </button>
        )}
        <button className={PRIMARY_BUTTON} disabled={installing || blocked} onClick={onInstall}>
          {installing ? (
            <Spinner />
          ) : existing?.kind === "sameUrl" ? (
            "Update"
          ) : replace ? (
            "Replace"
          ) : hints.configurable && !configured ? (
            "Install default"
          ) : (
            "Install"
          )}
        </button>
      </div>
    </div>
  );
}
