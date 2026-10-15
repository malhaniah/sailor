import {
  Check,
  Compass,
  Info,
  Languages,
  MonitorPlay,
  Palette,
  PlugZap,
  Waves,
} from "lucide-react";
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  ChipPicker,
  ComingLaterBlock,
  SectionCard,
  Spinner,
  ToggleRow,
} from "../components/ui";
import { playerStatus } from "../lib/ipc";
import { useSettings } from "../lib/store";
import { THEME_PRESETS } from "../lib/theme";
import type { PlayerStatus } from "../lib/types";

type SectionId = "player" | "engine" | "theme" | "content" | "integrations" | "about";

const SECTIONS: {
  group: string;
  items: { id: SectionId; label: string; icon: typeof MonitorPlay; sub: string }[];
}[] = [
  {
    group: "Playback",
    items: [
      {
        id: "player",
        label: "Player & quality",
        icon: MonitorPlay,
        sub: "How Sailor plays what your addons find.",
      },
      {
        id: "engine",
        label: "Streaming engine",
        icon: Waves,
        sub: "The built-in torrent engine: ports, limits, cache.",
      },
    ],
  },
  {
    group: "Appearance",
    items: [
      {
        id: "theme",
        label: "Theme & appearance",
        icon: Palette,
        sub: "Color presets Sailor renders in.",
      },
      {
        id: "content",
        label: "Content",
        icon: Compass,
        sub: "Which rooms show up in the sidebar.",
      },
    ],
  },
  {
    group: "Connections",
    items: [
      {
        id: "integrations",
        label: "Integrations",
        icon: PlugZap,
        sub: "Debrid, Trakt and friends — not in Sailor yet.",
      },
    ],
  },
  {
    group: "System",
    items: [
      {
        id: "about",
        label: "About",
        icon: Info,
        sub: "Version, license, and what Sailor is (not).",
      },
    ],
  },
];

export function SettingsView() {
  const [active, setActive] = useState<SectionId>("player");
  const meta = SECTIONS.flatMap((g) => g.items).find((i) => i.id === active)!;

  return (
    <div className="flex h-full bg-canvas">
      <nav className="relative flex w-72 shrink-0 flex-col gap-5 overflow-y-auto bg-surface px-4 pb-8 pt-24 shadow-[1px_0_0_var(--color-edge)]">
        {SECTIONS.map((group) => (
          <div key={group.group} className="flex flex-col gap-0.5">
            <div className="px-3.5 pb-1.5 pt-1 text-[10.5px] font-semibold uppercase tracking-[0.12em] text-ink-subtle/80">
              {group.group}
            </div>
            {group.items.map((item) => {
              const Icon = item.icon;
              const isActive = item.id === active;
              return (
                <button
                  key={item.id}
                  onClick={() => setActive(item.id)}
                  className={`group flex h-14 w-full items-center gap-3 rounded-xl px-2.5 text-left transition-colors ${
                    isActive
                      ? "bg-raised text-ink shadow-[inset_0_0_0_1px_var(--color-edge)]"
                      : "text-ink-muted hover:bg-elevated/70 hover:text-ink"
                  }`}
                >
                  <span
                    className={`flex h-10 w-10 items-center justify-center rounded-xl ${
                      isActive
                        ? "bg-elevated text-ink shadow-[inset_0_0_0_1px_var(--color-edge-soft)]"
                        : "bg-canvas/60 text-ink-subtle group-hover:text-ink-muted"
                    }`}
                  >
                    <Icon size={20} strokeWidth={1.6} />
                  </span>
                  <span className="flex-1 truncate text-[14.5px] font-medium">{item.label}</span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>

      <main className="flex-1 overflow-y-auto pb-16 pt-28">
        <div data-tauri-drag-region className="mx-auto flex max-w-3xl flex-col gap-10 px-12">
          <header className="flex flex-col gap-2">
            <h1 className="font-display text-[44px] font-medium leading-[1.05] tracking-tight text-ink">
              {meta.label}
            </h1>
            <p className="text-[15px] text-ink-muted">{meta.sub}</p>
          </header>

          {active === "player" && <PlayerSection />}
          {active === "engine" && <EngineSection />}
          {active === "theme" && <ThemeSection />}
          {active === "content" && <ContentSection />}
          {active === "integrations" && <IntegrationsSection />}
          {active === "about" && <AboutSection />}
        </div>
      </main>
    </div>
  );
}

// ---- sections -------------------------------------------------------------------------

function PlayerSection() {
  const { settings, save } = useSettings();
  const [player, setPlayer] = useState<PlayerStatus | null>(null);
  const [path, setPath] = useState<string | null>(null);

  useEffect(() => {
    playerStatus().then(setPlayer).catch(() => {});
  }, []);
  useEffect(() => {
    if (settings) setPath(settings.mpvPath ?? "");
  }, [settings]);

  if (!settings || path === null) return <Spinner label="Loading…" />;

  return (
    <>
      <SectionCard
        title="mpv"
        description="Sailor plays through mpv. Until the embedded player lands, a system-installed mpv window is used."
      >
        <div className="flex items-center gap-2 text-[13.5px]">
          <span className="text-ink-muted">Status:</span>
          {player?.available ? (
            <span className="flex items-center gap-1.5 font-medium text-accent">
              <span className="h-1.5 w-1.5 rounded-full bg-accent shadow-[0_0_6px_rgba(200,160,80,0.6)]" />
              found — <span className="font-mono text-[11.5px]">{player.path}</span>
            </span>
          ) : (
            <span className="font-medium text-[oklch(0.75_0.12_25)]">
              not found — install mpv or set a path below
            </span>
          )}
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
            mpv binary path (empty = search PATH)
          </span>
          <div className="flex gap-2">
            <input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="C:\\tools\\mpv\\mpv.exe"
              className="h-12 w-full rounded-xl border border-edge bg-canvas px-4 font-mono text-[12.5px] text-ink outline-none placeholder:text-ink-subtle focus:border-ink"
            />
            <button
              onClick={async () => {
                await save({ mpvPath: path.trim() || null });
                setPlayer(await playerStatus().catch(() => null));
              }}
              className="flex h-12 shrink-0 items-center rounded-xl bg-ink px-5 text-[13.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97]"
            >
              Save
            </button>
          </div>
        </div>
      </SectionCard>

      <SectionCard title="Play button behavior">
        <ToggleRow
          label="Instant play"
          sub="Pressing Play starts the first playable stream instead of always showing the picker. (The picker is always one click away.)"
          on={settings.instantPlay}
          onChange={(on) => void save({ instantPlay: on })}
          lockReason={settings.instantPlay ? undefined : undefined}
        />
      </SectionCard>
    </>
  );
}

function EngineSection() {
  const { settings, save } = useSettings();
  if (!settings) return <Spinner label="Loading…" />;

  const limits = [0, 128, 512, 1024, 5120] as const;
  const limitLabel = (v: number) => (v === 0 ? "No limit" : v >= 1024 ? `${v / 1024} MiB/s` : `${v} KiB/s`);

  return (
    <>
      <SectionCard
        title="Network"
        description="Changes apply the next time the engine starts (it launches on first torrent playback)."
      >
        <div className="flex flex-col gap-1.5">
          <span className="text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
            Listen port (0 = random)
          </span>
          <input
            type="number"
            min={0}
            max={65535}
            value={settings.engineListenPort}
            onChange={(e) => void save({ engineListenPort: Number(e.target.value) || 0 })}
            className="h-12 w-40 rounded-xl border border-edge bg-canvas px-4 font-mono text-[13px] text-ink outline-none focus:border-ink"
          />
        </div>
        <ToggleRow
          label="UPnP port forwarding"
          sub="Ask your router to open the listen port for better swarm connectivity."
          on={settings.engineUpnp}
          onChange={(on) => void save({ engineUpnp: on })}
        />
      </SectionCard>

      <SectionCard title="Speed limits">
        <div className="flex flex-col gap-1.5">
          <span className="text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
            Upload limit
          </span>
          <ChipPicker
            options={[...limits]}
            value={(settings.uploadLimitKibps ?? 0) as (typeof limits)[number]}
            onChange={(v) => void save({ uploadLimitKibps: v === 0 ? null : v })}
            format={limitLabel}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
            Download limit
          </span>
          <ChipPicker
            options={[...limits]}
            value={(settings.downloadLimitKibps ?? 0) as (typeof limits)[number]}
            onChange={(v) => void save({ downloadLimitKibps: v === 0 ? null : v })}
            format={limitLabel}
          />
        </div>
      </SectionCard>

      <SectionCard title="Cache">
        <ToggleRow
          label="Delete cached data when stopping playback"
          sub="Keeps disk usage bounded. Turn off to keep downloaded torrents for instant replays."
          on={settings.deleteOnStop}
          onChange={(on) => void save({ deleteOnStop: on })}
        />
      </SectionCard>
    </>
  );
}

function ThemeSection() {
  const { settings, save } = useSettings();
  if (!settings) return <Spinner label="Loading…" />;

  return (
    <SectionCard title="Theme" description="Color presets. The whole app re-tints instantly.">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
        {THEME_PRESETS.map((preset) => {
          const activePreset = settings.theme === preset.id;
          return (
            <button
              key={preset.id}
              onClick={() => void save({ theme: preset.id })}
              className={`relative h-[150px] overflow-hidden rounded-2xl border p-4 text-left transition-colors ${
                activePreset ? "border-ink" : "border-edge-soft hover:border-edge"
              }`}
              style={{ background: preset.swatch[0] }}
            >
              <div
                className="absolute inset-x-0 top-0 h-2/5"
                style={{
                  background: `linear-gradient(to bottom, ${preset.swatch[1]}, ${preset.swatch[0]})`,
                }}
              />
              <span
                className="absolute right-3 top-3 flex h-7 w-7 items-center justify-center rounded-full"
                style={{ background: preset.swatch[2], color: preset.swatch[0] }}
              >
                {activePreset && <Check size={15} strokeWidth={3} />}
              </span>
              <div className="absolute inset-x-4 bottom-3.5">
                <div className="text-[14.5px] font-semibold" style={{ color: preset.swatch[2] }}>
                  {preset.name}
                </div>
                <div className="text-[11.5px] opacity-70" style={{ color: preset.swatch[2] }}>
                  {preset.blurb}
                </div>
              </div>
            </button>
          );
        })}
      </div>
    </SectionCard>
  );
}

function ContentSection() {
  const { settings, save } = useSettings();
  if (!settings) return <Spinner label="Loading…" />;
  return (
    <SectionCard title="Rooms" description="Hide sidebar rooms you don't use.">
      <ToggleRow
        label="Hide Anime"
        sub="Removes the Anime room from the sidebar."
        on={settings.hideAnime}
        onChange={(on) => void save({ hideAnime: on })}
      />
      <ToggleRow
        label="Hide Live TV"
        sub="Removes the (not-yet-functional) Live TV entry."
        on={settings.hideLiveTv}
        onChange={(on) => void save({ hideLiveTv: on })}
      />
    </SectionCard>
  );
}

function IntegrationsSection() {
  return (
    <>
      <SectionCard
        title="Debrid services"
        description="Real-Debrid, TorBox, AllDebrid, Premiumize — planned, not in Sailor yet."
      >
        <ComingLaterBlock>
          <div className="flex flex-col gap-2">
            {["Real-Debrid", "TorBox", "AllDebrid"].map((name) => (
              <div
                key={name}
                className="flex h-14 items-center gap-3 rounded-2xl border border-edge-soft bg-elevated px-4"
              >
                <Languages size={18} className="text-ink-subtle" />
                <span className="text-[14px] text-ink-muted">{name} API token</span>
              </div>
            ))}
          </div>
        </ComingLaterBlock>
      </SectionCard>
      <SectionCard title="Trakt" description="Scrobbling and watchlist sync — planned.">
        <ComingLaterBlock>
          <div className="h-20 rounded-2xl border border-edge-soft bg-elevated" />
        </ComingLaterBlock>
      </SectionCard>
      <SectionCard title="Watch together" description="Synced watch parties — someday.">
        <ComingLaterBlock label="On the horizon">
          <div className="h-20 rounded-2xl border border-edge-soft bg-elevated" />
        </ComingLaterBlock>
      </SectionCard>
    </>
  );
}

function AboutSection() {
  const [version, setVersion] = useState("…");
  useEffect(() => {
    getVersion()
      .then((v) => setVersion(import.meta.env.DEV ? `${v} (dev)` : v))
      .catch(() => setVersion("unknown"));
  }, []);
  return (
    <>
      <SectionCard title="Sailor">
        <dl className="flex flex-col gap-3">
          {[
            ["Version", version],
            ["Build", "Desktop (Tauri 2 / WebView2)"],
            ["License", "GPL-3.0-or-later — free and open source"],
          ].map(([label, value]) => (
            <div key={label} className="flex items-baseline justify-between gap-6">
              <dt className="text-[12px] font-medium uppercase tracking-[0.18em] text-ink-subtle">
                {label}
              </dt>
              <dd className="text-[14px] text-ink">{value}</dd>
            </div>
          ))}
        </dl>
      </SectionCard>
      <div className="rounded-2xl bg-canvas/30 p-5">
        <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-subtle">
          Legal
        </div>
        <p className="mt-2 text-[13px] leading-relaxed text-ink-muted">
          Sailor hosts no media, indexes no media, and ships no content addon. All catalogs,
          metadata and streams come from third-party addons you choose to install. Sailor is an
          independent project and is not affiliated with or endorsed by Stremio (Smart Code
          Ltd).
        </p>
      </div>
    </>
  );
}
