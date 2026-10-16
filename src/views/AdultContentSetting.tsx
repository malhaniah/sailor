import { useState } from "react";
import { Modal, SectionCard, ToggleRow } from "../components/ui";
import { useAddons, useSettings } from "../lib/store";

/** Settings → Content: the one adult switch (catalogs, genres, titles; the marketplace
 * uses it too). Turning it on asks the user to confirm their age. */
export function AdultContentSetting() {
  const { settings, save } = useSettings();
  const { reload } = useAddons();
  const [confirming, setConfirming] = useState(false);
  if (!settings) return null;

  // the addon list Rust sends depends on it (adult genre options are left out)
  const apply = async (on: boolean) => {
    await save({ showAdult: on });
    await reload();
  };

  return (
    <SectionCard
      title="Adult content"
      description="Hidden by default: catalogs of addons marked adult, adult genres and the titles filed under them."
    >
      <ToggleRow
        label="Show adult content"
        sub="Zeiler hides adult material by its addon's own flag and by genre names, which can miss some."
        on={settings.showAdult}
        onChange={(on) => (on ? setConfirming(true) : void apply(false))}
      />
      {confirming && (
        <Modal title="Show adult content?" onClose={() => setConfirming(false)}>
          <p className="text-[14px] leading-relaxed text-ink-muted">
            Adult catalogs and genres from your addons will show in every room, in Browse and
            in search. Confirm that you are 18 or older, or the age of majority where you
            live.
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <button
              onClick={() => setConfirming(false)}
              className="h-10 rounded-full border border-edge-soft px-5 text-[13px] font-medium text-ink-muted hover:border-ink hover:text-ink"
            >
              Cancel
            </button>
            <button
              onClick={() => {
                setConfirming(false);
                void apply(true);
              }}
              className="h-10 rounded-full bg-ink px-5 text-[13px] font-semibold text-canvas"
            >
              I am 18 or older
            </button>
          </div>
        </Modal>
      )}
    </SectionCard>
  );
}
