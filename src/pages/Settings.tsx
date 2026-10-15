import { useEffect, useState } from "react";
import { Badge, ErrorNote, Spinner } from "../components/ui";
import { errorMessage, playerStatus, settingsGet, settingsSet } from "../lib/ipc";
import type { PlayerStatus, Settings } from "../lib/types";

export function SettingsPage() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [player, setPlayer] = useState<PlayerStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    settingsGet().then(setSettings).catch((e) => setError(errorMessage(e)));
    playerStatus().then(setPlayer).catch(() => {});
  }, []);

  if (!settings) {
    return (
      <div className="p-8">{error ? <ErrorNote message={error} /> : <Spinner label="Loading…" />}</div>
    );
  }

  async function save() {
    if (!settings) return;
    setSaving(true);
    setError(null);
    try {
      const updated = await settingsSet(settings);
      setSettings(updated);
      setPlayer(await playerStatus());
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="flex max-w-2xl flex-col gap-8 p-8">
      <header>
        <h1 className="text-2xl font-bold">Settings</h1>
      </header>

      {error && <ErrorNote message={error} />}

      <section className="flex flex-col gap-3">
        <h2 className="text-sm font-semibold uppercase tracking-wider text-mist">Player</h2>
        <div className="flex items-center gap-2 text-sm">
          <span className="text-mist">mpv:</span>
          {player?.available ? (
            <>
              <Badge tone="good">found</Badge>
              <span className="truncate text-xs text-mist">{player.path}</span>
            </>
          ) : (
            <Badge tone="warn">not found — install mpv or set a path below</Badge>
          )}
        </div>
        <label className="flex flex-col gap-1 text-sm">
          <span className="text-mist">mpv binary path (empty = search PATH)</span>
          <input
            value={settings.mpvPath ?? ""}
            onChange={(e) =>
              setSettings({ ...settings, mpvPath: e.target.value.trim() || null })
            }
            placeholder="C:\\tools\\mpv\\mpv.exe"
            className="rounded-lg border border-rig bg-hull px-3 py-2 text-sm focus:border-buoy focus:outline-none"
          />
        </label>
      </section>

      <section className="flex flex-col gap-3">
        <h2 className="text-sm font-semibold uppercase tracking-wider text-mist">
          Torrent engine
        </h2>
        <p className="text-xs text-mist/70">
          The engine starts on first torrent playback. Changes here apply the next time it
          starts.
        </p>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-mist">Listen port (0 = random)</span>
            <input
              type="number"
              min={0}
              max={65535}
              value={settings.engineListenPort}
              onChange={(e) =>
                setSettings({ ...settings, engineListenPort: Number(e.target.value) || 0 })
              }
              className="rounded-lg border border-rig bg-hull px-3 py-2 text-sm focus:border-buoy focus:outline-none"
            />
          </label>
          <label className="flex flex-col gap-1 text-sm">
            <span className="text-mist">Upload limit (KiB/s, empty = unlimited)</span>
            <input
              type="number"
              min={1}
              value={settings.uploadLimitKibps ?? ""}
              onChange={(e) =>
                setSettings({
                  ...settings,
                  uploadLimitKibps: e.target.value ? Number(e.target.value) : null,
                })
              }
              className="rounded-lg border border-rig bg-hull px-3 py-2 text-sm focus:border-buoy focus:outline-none"
            />
          </label>
        </div>
        <label className="flex items-center gap-2 text-sm text-mist">
          <input
            type="checkbox"
            checked={settings.engineUpnp}
            onChange={(e) => setSettings({ ...settings, engineUpnp: e.target.checked })}
          />
          UPnP port forwarding
        </label>
        <label className="flex items-center gap-2 text-sm text-mist">
          <input
            type="checkbox"
            checked={settings.deleteOnStop}
            onChange={(e) => setSettings({ ...settings, deleteOnStop: e.target.checked })}
          />
          Delete cached torrent data when stopping playback
        </label>
      </section>

      <div className="flex items-center gap-3">
        <button
          onClick={() => void save()}
          disabled={saving}
          className="w-fit rounded-lg bg-buoy px-6 py-2.5 text-sm font-semibold text-abyss hover:bg-buoy/80 disabled:opacity-50"
        >
          {saving ? "Saving…" : "Save"}
        </button>
        {saved && <span className="text-sm text-buoy">Saved ✓</span>}
      </div>

      <footer className="border-t border-rig/50 pt-4 text-xs leading-relaxed text-mist/70">
        Sailor is free software licensed under GPL-3.0-or-later. It hosts no media, indexes no
        media, and ships no content addon. Sailor is not affiliated with or endorsed by Stremio
        (Smart Code Ltd).
      </footer>
    </div>
  );
}
