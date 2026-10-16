import { Lock, Settings2 } from "lucide-react";
import { useState } from "react";
import { ErrorNote } from "../../components/ui";
import { errorMessage, profileSelect } from "../../lib/ipc";
import { useProfiles } from "../../lib/profiles";
import type { ProfileView } from "../../lib/types";
import { PinEntry } from "./PinEntry";
import { ProfileAvatar } from "./ProfileAvatar";

/** "Who's watching?": shown at launch when there is more than one profile or the
 * active one has a PIN, and from the sidebar's profile menu. */
export function ProfilePicker({ onManage }: { onManage: () => void }) {
  const { state, picking, setPicking } = useProfiles();
  const [pinFor, setPinFor] = useState<ProfileView | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (!state) return null;
  // always shown while the active profile is locked (the shell isn't mounted then)
  if (!picking && state.unlocked) return null;
  // closable only once the active profile may be used
  const canClose = state.unlocked;

  async function choose(profile: ProfileView, pin?: string) {
    setError(null);
    await profileSelect(profile.id, pin);
    setPinFor(null);
    setPicking(false);
  }

  return (
    <div
      className="fixed inset-0 z-[130] flex flex-col items-center justify-center gap-10 bg-canvas/95 px-8 backdrop-blur-md"
      onKeyDown={(e) => {
        if (e.key === "Escape" && canClose) setPicking(false);
      }}
    >
      <h1 className="font-display text-[40px] font-medium tracking-tight text-ink">
        Who's watching?
      </h1>
      <div className="flex max-w-4xl flex-wrap justify-center gap-8">
        {state.profiles.map((profile) => (
          <button
            key={profile.id}
            onClick={() => {
              const needsPin = profile.locked && !(profile.active && state.unlocked);
              if (needsPin) setPinFor(profile);
              else choose(profile).catch((e) => setError(errorMessage(e)));
            }}
            className="group flex w-32 flex-col items-center gap-3 rounded-2xl p-3 transition-colors hover:bg-elevated/60 focus-visible:bg-elevated/60"
          >
            <ProfileAvatar profile={profile} size={96} />
            <span className="flex max-w-full items-center gap-1.5 text-[14.5px] font-medium text-ink-muted group-hover:text-ink">
              <span className="truncate">{profile.name}</span>
              {profile.locked && <Lock size={13} className="shrink-0 text-ink-subtle" aria-label="PIN" />}
            </span>
          </button>
        ))}
      </div>
      {error && (
        <div className="w-full max-w-md">
          <ErrorNote message={error} />
        </div>
      )}
      <div className={`flex gap-3 ${canClose ? "" : "invisible"}`}>
        <button
          onClick={() => {
            if (!canClose) return;
            setPicking(false);
            onManage();
          }}
          disabled={!canClose}
          title={canClose ? undefined : "Choose a profile first"}
          className="flex h-10 items-center gap-2 rounded-xl border border-edge px-4 text-[13px] font-medium text-ink-muted hover:text-ink disabled:opacity-40"
        >
          <Settings2 size={15} /> Manage profiles
        </button>
      </div>
      {pinFor && (
        <PinEntry
          title={`PIN for ${pinFor.name}`}
          onClose={() => setPinFor(null)}
          onSubmit={(pin) => choose(pinFor, pin)}
        />
      )}
    </div>
  );
}
