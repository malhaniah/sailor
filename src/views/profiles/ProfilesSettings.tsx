import { ImagePlus, Lock, Plus, Trash2 } from "lucide-react";
import { useRef, useState } from "react";
import { ErrorNote, Modal, SectionCard } from "../../components/ui";
import {
  errorKind,
  errorMessage,
  profileCreate,
  profileDelete,
  profileSetAvatar,
  profileSetPin,
  profileUpdate,
} from "../../lib/ipc";
import { useProfiles } from "../../lib/profiles";
import type { ProfileView } from "../../lib/types";
import { PRIMARY_BUTTON, SECONDARY_BUTTON, SMALL_BUTTON } from "../addons/common";
import { PinEntry } from "./PinEntry";
import { COLOR_IDS, PROFILE_COLORS, ProfileAvatar, resizeImage } from "./ProfileAvatar";

/** Settings → Profiles & account: the local profiles. */
export function ProfilesSettings() {
  const { state, setPicking } = useProfiles();
  const [adding, setAdding] = useState(false);
  if (!state) return null;
  return (
    <SectionCard
      title="Profiles"
      description="Each profile has its own addons, library, Continue Watching and preferences (theme, rooms, subtitles and audio). Device settings such as the player and the streaming engine are shared."
    >
      <div className="flex flex-col gap-3">
        {state.profiles.map((profile) => (
          <ProfileRow key={profile.id} profile={profile} />
        ))}
      </div>
      <div className="flex flex-wrap gap-3">
        <button className={SMALL_BUTTON} onClick={() => setAdding(true)} disabled={state.profiles.length >= 8}>
          <Plus size={14} /> Add profile
        </button>
        {state.profiles.length > 1 && (
          <button className={SMALL_BUTTON} onClick={() => setPicking(true)}>
            Switch profile
          </button>
        )}
      </div>
      <p className="text-[12px] leading-relaxed text-ink-subtle">
        A PIN keeps others from casually opening a profile. It is not encryption: anyone
        with access to this computer's files can still read them.
      </p>
      {adding && <AddProfile onClose={() => setAdding(false)} />}
    </SectionCard>
  );
}

/** Runs `action`; when Rust says the profile is locked, asks for its PIN and runs it
 * again with the PIN. */
function useWithPin(profile: ProfileView) {
  const [prompt, setPrompt] = useState<((pin: string) => Promise<void>) | null>(null);
  const [error, setError] = useState<string | null>(null);
  async function run(action: (pin?: string) => Promise<unknown>) {
    setError(null);
    try {
      await action();
    } catch (e) {
      if (errorKind(e) === "locked" && profile.locked) {
        setPrompt(() => async (pin: string) => {
          await action(pin);
          setPrompt(null);
        });
      } else {
        setError(errorMessage(e));
      }
    }
  }
  const pinDialog = prompt && (
    <PinEntry title={`PIN for ${profile.name}`} onClose={() => setPrompt(null)} onSubmit={prompt} />
  );
  return { run, error, pinDialog };
}

function ProfileRow({ profile }: { profile: ProfileView }) {
  const { run, error, pinDialog } = useWithPin(profile);
  const [name, setName] = useState(profile.name);
  const [pinDialogOpen, setPinDialogOpen] = useState(false);
  const [removingPin, setRemovingPin] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const file = useRef<HTMLInputElement>(null);

  return (
    <div className="flex flex-col gap-3 rounded-2xl border border-edge-soft bg-canvas/40 p-4">
      <div className="flex flex-wrap items-center gap-4">
        <button
          title="Change picture"
          onClick={() => file.current?.click()}
          className="relative rounded-xl focus-visible:outline-2"
        >
          <ProfileAvatar profile={profile} size={56} />
          <span className="absolute -bottom-1 -right-1 flex h-6 w-6 items-center justify-center rounded-full bg-elevated text-ink-muted shadow">
            <ImagePlus size={13} />
          </span>
        </button>
        <input
          ref={file}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/gif"
          className="hidden"
          onChange={async (e) => {
            const picked = e.target.files?.[0];
            e.target.value = "";
            if (!picked) return;
            await run(async (pin) => profileSetAvatar(profile.id, await resizeImage(picked), pin));
          }}
        />
        <form
          className="flex min-w-0 flex-1 items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim() && name.trim() !== profile.name) {
              void run((pin) => profileUpdate(profile.id, { name }, pin));
            }
          }}
        >
          <input
            aria-label="Profile name"
            value={name}
            maxLength={32}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => {
              if (name.trim() && name.trim() !== profile.name) {
                void run((pin) => profileUpdate(profile.id, { name }, pin));
              } else setName(profile.name);
            }}
            className="h-10 min-w-0 flex-1 rounded-xl border border-edge bg-canvas px-3 text-[14px] text-ink outline-none focus:border-ink"
          />
        </form>
        <div className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-wider text-ink-subtle">
          {profile.active && <span className="rounded-full bg-accent/15 px-2.5 py-1 text-accent">In use</span>}
          {profile.isPrimary && <span className="rounded-full bg-raised px-2.5 py-1">Main</span>}
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        {COLOR_IDS.map((color) => (
          <button
            key={color}
            aria-label={`Colour ${color}`}
            aria-pressed={profile.color === color}
            onClick={() => void run((pin) => profileUpdate(profile.id, { color }, pin))}
            className={`h-6 w-6 rounded-full transition-transform hover:scale-110 ${
              profile.color === color ? "ring-2 ring-ink ring-offset-2 ring-offset-canvas" : ""
            }`}
            style={{ background: PROFILE_COLORS[color] }}
          />
        ))}
        <span className="flex-1" />
        <button className={SMALL_BUTTON} onClick={() => setPinDialogOpen(true)}>
          <Lock size={14} /> {profile.locked ? "Change PIN" : "Add PIN"}
        </button>
        {profile.locked && (
          <button className={SMALL_BUTTON} onClick={() => setRemovingPin(true)}>
            Remove PIN
          </button>
        )}
        {profile.avatar && (
          <button className={SMALL_BUTTON} onClick={() => void run((pin) => profileSetAvatar(profile.id, null, pin))}>
            Remove picture
          </button>
        )}
        {!profile.isPrimary && !profile.active && (
          <button
            className={`${SMALL_BUTTON} hover:border-danger/40 hover:text-[oklch(0.75_0.12_25)]`}
            onClick={() => setConfirmDelete(true)}
          >
            <Trash2 size={14} /> Delete
          </button>
        )}
      </div>
      {error && <ErrorNote message={error} />}
      {pinDialog}
      {pinDialogOpen && <PinDialog profile={profile} onClose={() => setPinDialogOpen(false)} />}
      {removingPin && (
        <PinEntry
          title={`Current PIN for ${profile.name}`}
          note="Enter it to remove it."
          onClose={() => setRemovingPin(false)}
          onSubmit={async (pin) => {
            await profileSetPin(profile.id, pin, null);
            setRemovingPin(false);
          }}
        />
      )}
      {confirmDelete && (
        <Modal title={`Delete ${profile.name}?`} onClose={() => setConfirmDelete(false)}>
          <p className="text-[13.5px] leading-relaxed text-ink-muted">
            Its addons, library, Continue Watching and preferences are removed from this
            computer. This can't be undone.
          </p>
          <div className="mt-6 flex justify-end gap-3">
            <button className={SECONDARY_BUTTON} onClick={() => setConfirmDelete(false)}>
              Cancel
            </button>
            <button
              className={PRIMARY_BUTTON}
              onClick={() => {
                setConfirmDelete(false);
                void run((pin) => profileDelete(profile.id, pin));
              }}
            >
              Delete profile
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}

/** Set, change or remove a PIN: the current one first (when there is one), then the
 * new one twice. */
function PinDialog({ profile, onClose }: { profile: ProfileView; onClose: () => void }) {
  const [current, setCurrent] = useState<string | null>(profile.locked ? null : "");
  const [first, setFirst] = useState<string | null>(null);
  if (current === null) {
    return (
      <PinEntry
        title={`Current PIN for ${profile.name}`}
        note="Enter it to change it."
        onClose={onClose}
        onSubmit={async (pin) => {
          // checked now (an edit that changes nothing), not after the new one twice
          await profileUpdate(profile.id, {}, pin);
          setCurrent(pin);
        }}
      />
    );
  }
  if (first === null) {
    return (
      <PinEntry
        title={profile.locked ? "New PIN" : `A PIN for ${profile.name}`}
        note="4 to 8 digits, asked when someone switches to this profile or edits it."
        onClose={onClose}
        onSubmit={async (pin) => setFirst(pin)}
      />
    );
  }
  return (
    <PinEntry
      title="Enter it again"
      onClose={onClose}
      onSubmit={async (pin) => {
        if (pin !== first) {
          setFirst(null);
          throw new Error("The two PINs differ. Start again.");
        }
        await profileSetPin(profile.id, current || null, pin);
        onClose();
      }}
    />
  );
}

function AddProfile({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  const [color, setColor] = useState(COLOR_IDS[(Date.now() / 1000) % COLOR_IDS.length | 0]);
  const [error, setError] = useState<string | null>(null);
  return (
    <Modal title="Add a profile" onClose={onClose}>
      <form
        className="flex flex-col gap-4"
        onSubmit={async (e) => {
          e.preventDefault();
          setError(null);
          try {
            await profileCreate(name, color);
            onClose();
          } catch (err) {
            setError(errorMessage(err));
          }
        }}
      >
        <div className="flex items-center gap-4">
          <ProfileAvatar profile={{ name: name || "?", color, avatar: null }} size={56} />
          <input
            autoFocus
            aria-label="Name"
            placeholder="Name"
            maxLength={32}
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="h-11 min-w-0 flex-1 rounded-xl border border-edge bg-canvas px-3 text-[14px] text-ink outline-none focus:border-ink"
          />
        </div>
        <div className="flex flex-wrap gap-2">
          {COLOR_IDS.map((c) => (
            <button
              type="button"
              key={c}
              aria-label={`Colour ${c}`}
              aria-pressed={color === c}
              onClick={() => setColor(c)}
              className={`h-7 w-7 rounded-full ${color === c ? "ring-2 ring-ink ring-offset-2 ring-offset-elevated" : ""}`}
              style={{ background: PROFILE_COLORS[c] }}
            />
          ))}
        </div>
        <p className="text-[12.5px] leading-relaxed text-ink-muted">
          A new profile starts empty: no addons, an empty library, and default preferences.
        </p>
        {error && <ErrorNote message={error} />}
        <div className="flex justify-end gap-3">
          <button type="button" className={SECONDARY_BUTTON} onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className={PRIMARY_BUTTON} disabled={!name.trim()}>
            Add profile
          </button>
        </div>
      </form>
    </Modal>
  );
}
