import { listen } from "@tauri-apps/api/event";
import { Download, X } from "lucide-react";
import { useEffect, useState } from "react";
import { errorMessage, updateInstallNow, updateStatus } from "../lib/ipc";

/**
 * Update UI. The Rust updater does the work; this shows a full-screen notice while an
 * update installs at launch, and a small banner when one has downloaded mid-session
 * (it installs on the next launch, or now on request).
 */
export function UpdateBanner() {
  const [ready, setReady] = useState<string | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    updateStatus()
      .then((s) => setReady(s.ready ?? null))
      .catch(() => {});
    const unlisten = [
      listen<string>("update://ready", (e) => {
        setReady(e.payload);
        setDismissed(false);
      }),
      listen<string>("update://installing", (e) => setInstalling(e.payload)),
      listen<string>("update://failed", (e) => {
        setInstalling(null);
        setError(e.payload);
      }),
    ];
    return () => {
      unlisten.forEach((p) => void p.then((off) => off()));
    };
  }, []);

  if (installing) {
    return (
      <div className="animate-fade-in fixed inset-0 z-[300] flex flex-col items-center justify-center gap-5 bg-canvas">
        <div className="h-8 w-8 animate-spin rounded-full border-[3px] border-edge border-t-accent" />
        <div className="font-display text-[32px] font-medium text-ink">Updating Sailor</div>
        <div className="text-[11px] font-semibold uppercase tracking-[0.32em] text-ink-subtle">
          Installing v{installing} · Sailor will reopen
        </div>
      </div>
    );
  }

  if ((!ready && !error) || dismissed) return null;

  return (
    <div className="animate-nudge-in fixed bottom-5 right-5 z-[150] flex max-w-sm items-start gap-3 rounded-2xl border border-edge-soft bg-elevated/95 px-4 py-3.5 shadow-[0_18px_50px_-20px_rgba(0,0,0,0.65)] backdrop-blur-md">
      <Download size={17} className="mt-0.5 shrink-0 text-accent" />
      <div className="flex min-w-0 flex-col gap-1">
        {error ? (
          <>
            <span className="text-[13px] font-semibold text-ink">Update failed</span>
            <span className="text-[12px] leading-snug text-ink-subtle">{error}</span>
          </>
        ) : (
          <>
            <span className="text-[13px] font-semibold text-ink">Sailor v{ready} is ready</span>
            <span className="text-[12px] leading-snug text-ink-subtle">
              It installs next time you open Sailor.
            </span>
            <button
              onClick={() =>
                updateInstallNow().catch((e) => {
                  setError(errorMessage(e));
                  setInstalling(null);
                })
              }
              className="mt-1.5 w-fit rounded-lg bg-ink px-3 py-1.5 text-[12px] font-semibold text-canvas transition-transform hover:scale-[1.02]"
            >
              Restart and update now
            </button>
          </>
        )}
      </div>
      <button
        aria-label="Dismiss"
        onClick={() => {
          setDismissed(true);
          setError(null);
        }}
        className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-ink-subtle hover:bg-raised hover:text-ink"
      >
        <X size={14} />
      </button>
    </div>
  );
}
