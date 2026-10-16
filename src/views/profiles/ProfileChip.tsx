import { useProfiles } from "../../lib/profiles";
import { useView } from "../../lib/view";
import { openSettingsSection } from "../Settings";
import { ProfileAvatar } from "./ProfileAvatar";

/** The active profile at the bottom of the sidebar: opens "Who's watching?" when there
 * are several profiles, else the profile settings. */
export function ProfileChip() {
  const { state, setPicking } = useProfiles();
  const { setView } = useView();
  const active = state?.profiles.find((p) => p.id === state.activeId);
  if (!state || !active) return null;
  const several = state.profiles.length > 1;
  return (
    <button
      onClick={() => {
        if (several) setPicking(true);
        else {
          openSettingsSection("account");
          setView("settings");
        }
      }}
      title={several ? "Switch profile" : "Profiles"}
      className="mx-4 mb-3 flex h-14 items-center justify-center gap-3 rounded-xl px-2 text-left text-ink-muted transition-colors hover:bg-elevated/50 hover:text-ink lg:justify-start lg:px-3"
    >
      <ProfileAvatar profile={active} size={32} />
      <span className="hidden min-w-0 flex-1 flex-col lg:flex">
        <span className="truncate text-[14px] font-medium text-ink">{active.name}</span>
        <span className="text-[11.5px] text-ink-subtle">{several ? "Switch profile" : "Profiles"}</span>
      </span>
    </button>
  );
}
