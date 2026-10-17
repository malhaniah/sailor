import { useEffect, useState } from "react";
import { ErrorNote, SectionCard, ToggleRow } from "../../components/ui";
import { deeplinkSetStremio, deeplinkStremioStatus, errorMessage } from "../../lib/ipc";
import { useSettings } from "../../lib/store";
import type { DeepLinkStatus } from "../../lib/types";
import { DIRECTORY_NOTICE } from "./BrowseTab";

/** Settings → Content: the addon directory, adult content and stremio:// links. */
export function AddonDirectorySettings() {
  const { settings, save } = useSettings();
  const [link, setLink] = useState<DeepLinkStatus | null>(null);
  const [linkError, setLinkError] = useState<string | null>(null);
  const [linkBusy, setLinkBusy] = useState(false);

  useEffect(() => {
    deeplinkStremioStatus()
      .then(setLink)
      .catch(() => setLink(null));
  }, []);

  if (!settings) return null;
  const sources = settings.marketplaceSources;
  return (
    <SectionCard title="Addon directory" description={DIRECTORY_NOTICE}>
      <ToggleRow
        label="Browse the addon directory"
        sub="Lists third-party addons in the Browse tab of the Addons page. Nothing is fetched from a directory while this is off."
        on={settings.marketplaceEnabled}
        onChange={(on) => void save({ marketplaceEnabled: on })}
      />
      {settings.marketplaceEnabled && (
        <>
          <ToggleRow
            label="stremio-addons.net"
            sub="Community listings with stars, categories and documentation."
            on={sources.stremioAddonsNet}
            onChange={(on) => void save({ marketplaceSources: { stremioAddonsNet: on } })}
          />
          <ToggleRow
            label="Stremio's addon catalog"
            sub="Stremio's community and official addon lists."
            on={sources.stremioCatalog}
            onChange={(on) => void save({ marketplaceSources: { stremioCatalog: on } })}
          />
          <ToggleRow
            label="Hide P2P addons"
            sub="Addons that say they use peer-to-peer streams are flagged either way. Not every one says so."
            on={settings.hideP2pAddons}
            onChange={(on) => void save({ hideP2pAddons: on })}
          />
        </>
      )}
      <ToggleRow
        label="Open stremio:// links in Zeiler"
        sub={
          link && !link.supported
            ? undefined
            : "Addon setup pages end with a stremio:// install link. Zeiler opens it in the install confirmation; nothing installs on its own. This takes those links over from a Stremio app on this computer."
        }
        lockReason={link && !link.supported ? "Not available on this system yet." : undefined}
        on={settings.stremioDeepLink}
        onChange={async (on) => {
          if (linkBusy) return;
          setLinkBusy(true);
          setLinkError(null);
          try {
            setLink(await deeplinkSetStremio(on));
          } catch (e) {
            setLinkError(errorMessage(e));
          } finally {
            setLinkBusy(false);
          }
        }}
      />
      {linkError && <ErrorNote message={linkError} />}
    </SectionCard>
  );
}
