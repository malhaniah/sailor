import {
  Captions,
  Check,
  Compass,
  Info,
  Languages,
  MonitorPlay,
  Palette,
  PlugZap,
  Waves,
} from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  ChipPicker,
  ComingLaterBlock,
  SectionCard,
  Spinner,
  ToggleRow,
} from "../components/ui";
import { errorMessage, updateCheck, updateInstallNow, updateStatus } from "../lib/ipc";
import { type PlayerBackends, playerBackends } from "../lib/player-ipc";
import { useAddons, useSettings } from "../lib/store";
import { THEME_PRESETS } from "../lib/theme";
import type { PlayerBackend } from "../lib/types";
import { AdultContentSetting } from "./AdultContentSetting";
import { SkipAndNextCards } from "./SkipSettings";
import { SubtitlesSection } from "./SubtitleSettings";

/** Ids reserved for later sections (docs/ARCHITECTURE.md, "Navigation names"). A
 * section shows only once it has a SECTIONS entry, so these stay hidden until then. */
type ReservedSectionId = "account" | "playback";

type SectionId =
  | "player"
  | "subtitles"
  | "engine"
  | "theme"
  | "content"
  | "integrations"
  | "about"
  | ReservedSectionId;

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
        sub: "How Zeiler plays what your addons find.",
      },
      {
        id: "subtitles",
        label: "Subtitles & audio",
        icon: Captions,
        sub: "Which subtitles and audio play, and how subtitles look.",
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
        sub: "Color presets Zeiler renders in.",
      },
      {
        id: "content",
        label: "Content",
        icon: Compass,
        sub: "Built-in addons, and which rooms show up in the sidebar.",
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
        sub: "Debrid, Trakt and friends — not in Zeiler yet.",
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
        sub: "Version, license, and what Zeiler is (not).",
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
          {active === "subtitles" && <SubtitlesSection />}
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

const IS_LINUX = /linux/i.test(navigator.userAgent) && !/android/i.test(navigator.userAgent);
const IS_MAC = /mac os x|macintosh/i.test(navigator.userAgent);
/** An example mpv location for this system (the path field's placeholder). */
const MPV_PLACEHOLDER = IS_LINUX
  ? "/usr/bin/mpv"
  : IS_MAC
    ? "/opt/homebrew/bin/mpv"
    : "C:\\Program Files\\mpv\\mpv.exe";

/** Arrow keys in a radio group: next (+1) or previous (-1) option. */
const RADIO_STEPS: Record<string, number | undefined> = {
  ArrowDown: 1,
  ArrowRight: 1,
  ArrowUp: -1,
  ArrowLeft: -1,
};

function PlayerSection() {
  const { settings, save } = useSettings();
  const [backends, setBackends] = useState<PlayerBackends | null>(null);
  const [path, setPath] = useState<string | null>(null);

  useEffect(() => {
    playerBackends().then(setBackends).catch(() => {});
  }, []);
  // follow the saved path only when it changes, so saving another setting here (the
  // backend radios) does not wipe an unsaved edit in the path field
  const loaded = settings !== null;
  const savedPath = settings?.mpvPath ?? "";
  useEffect(() => {
    if (loaded) setPath(savedPath);
  }, [loaded, savedPath]);

  if (!settings || path === null) return <Spinner label="Loading…" />;

  const embedded = backends?.embedded;
  const external = backends?.external;
  const builtInAvailable = embedded?.available ?? false;
  const choice = settings.playerBackend;
  // mpv matters when it is chosen, or when nothing else can play
  const mpvNeeded = choice === "external" || (backends !== null && !builtInAvailable);
  const options: { id: PlayerBackend; label: string; sub: string; unavailable?: string }[] = [
    {
      id: "auto",
      label: "Auto (recommended)",
      // neutral until the check answers, so it never states the wrong player first
      sub: !backends
        ? "Plays videos the best way this computer allows."
        : builtInAvailable
          ? "Plays inside Zeiler, and switches to mpv if it ever has to."
          : "Uses the best option on this computer — for now, mpv.",
    },
    {
      id: "embedded",
      label: "Built-in",
      sub: "Plays inside the Zeiler window.",
      // the one place the reason is shown
      unavailable: backends && !builtInAvailable ? (embedded?.reason ?? "Not available here.") : undefined,
    },
    {
      id: "external",
      label: "External mpv",
      sub: "Plays in its own mpv window, with your mpv settings. Zeiler's player screen controls it.",
    },
  ];

  return (
    <>
      <SectionCard
        title="Where videos play"
        description="Videos always start on Zeiler's player screen. Choose whether they play inside Zeiler or in a separate mpv window."
      >
        <div
          role="radiogroup"
          aria-label="Where videos play"
          className="flex flex-col gap-2"
          onKeyDown={(e) => {
            // the ARIA radio pattern: arrows move to the previous/next enabled option
            // and select it (one tab stop for the group)
            const step = RADIO_STEPS[e.key];
            if (!step) return;
            e.preventDefault();
            const enabled = options.filter((o) => !o.unavailable || o.id === choice);
            const at = enabled.findIndex((o) => o.id === choice);
            const next = enabled[(at + step + enabled.length) % enabled.length];
            if (!next) return;
            if (next.id !== choice) void save({ playerBackend: next.id });
            e.currentTarget
              .querySelector<HTMLElement>(`[data-backend="${next.id}"]`)
              ?.focus();
          }}
        >
          {options.map((option) => (
            <BackendOption
              key={option.id}
              id={option.id}
              label={option.label}
              sub={option.sub}
              selected={choice === option.id}
              unavailable={option.unavailable}
              onSelect={() => void save({ playerBackend: option.id })}
            />
          ))}
        </div>
        {choice === "embedded" && backends && !builtInAvailable && (
          <p className="text-[12.5px] text-[oklch(0.75_0.12_25)]">
            Videos can't play with this choice on this computer. Choose Auto or External mpv.
          </p>
        )}
      </SectionCard>

      <SectionCard
        title="mpv"
        description={
          !backends
            ? undefined
            : builtInAvailable
              ? "Only needed if you choose External mpv."
              : "Zeiler plays videos with mpv on this computer."
        }
      >
        <div className="text-[13.5px]">
          {!backends ? (
            <span className="text-ink-subtle">Looking for mpv…</span>
          ) : external?.available ? (
            <StatusOk>
              Found <span className="font-mono text-[11.5px] font-normal">{external.path}</span>
            </StatusOk>
          ) : mpvNeeded ? (
            <span className="font-medium text-[oklch(0.75_0.12_25)]">
              Not found. Install mpv from mpv.io, or set where it is below.
            </span>
          ) : (
            <span className="text-ink-muted">Not installed.</span>
          )}
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
            mpv location — optional
          </span>
          <div className="flex gap-2">
            <input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              aria-label="mpv location"
              placeholder={MPV_PLACEHOLDER}
              className="h-12 w-full rounded-xl border border-edge bg-canvas px-4 font-mono text-[12.5px] text-ink outline-none placeholder:text-ink-subtle focus:border-ink"
            />
            <button
              onClick={async () => {
                await save({ mpvPath: path.trim() || null });
                setBackends(await playerBackends().catch(() => null));
              }}
              className="flex h-12 shrink-0 items-center rounded-xl bg-ink px-5 text-[13.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97]"
            >
              Save
            </button>
          </div>
          <span className="text-[12px] text-ink-subtle">
            Leave empty and Zeiler finds mpv on its own.
          </span>
        </div>
      </SectionCard>

      <SectionCard title="Play button behavior">
        <ToggleRow
          label="Instant play"
          sub="Pressing Play starts the first playable stream instead of always showing the picker. (The picker is always one click away.)"
          on={settings.instantPlay}
          onChange={(on) => void save({ instantPlay: on })}
        />
      </SectionCard>

      <SkipAndNextCards />
    </>
  );
}

function StatusOk({ children }: { children: ReactNode }) {
  return (
    <span className="inline-flex items-center gap-1.5 font-medium text-accent">
      <span className="h-1.5 w-1.5 rounded-full bg-accent shadow-[0_0_6px_rgba(200,160,80,0.6)]" />
      <span>{children}</span>
    </span>
  );
}

/** One radio option, in the ToggleRow recipe. An unavailable option stays selectable
 * only if it is already selected (so the user can see and change it). Only the selected
 * option is a tab stop; the group's arrow keys move between them. */
function BackendOption({
  id,
  label,
  sub,
  selected,
  unavailable,
  onSelect,
}: {
  id: PlayerBackend;
  label: string;
  sub: string;
  selected: boolean;
  unavailable?: string;
  onSelect: () => void;
}) {
  const disabled = !!unavailable && !selected;
  return (
    <button
      role="radio"
      aria-checked={selected}
      data-backend={id}
      tabIndex={selected ? 0 : -1}
      disabled={disabled}
      onClick={onSelect}
      className={`flex items-start gap-3 rounded-xl border bg-canvas/40 px-4 py-3 text-left transition-colors ${
        selected ? "border-ink" : "border-edge-soft hover:border-edge"
      } ${disabled ? "cursor-not-allowed opacity-60 hover:border-edge-soft" : ""}`}
    >
      <span
        className={`mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full border-2 ${
          selected ? "border-ink" : "border-edge"
        }`}
      >
        {selected && <span className="h-2.5 w-2.5 rounded-full bg-ink" />}
      </span>
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className="text-[14px] font-medium text-ink">{label}</span>
        <span className="text-[12.5px] leading-snug text-ink-muted">{sub}</span>
        {unavailable && (
          <span className="text-[12.5px] leading-snug text-ink-muted">{unavailable}</span>
        )}
      </span>
    </button>
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

/** The built-in addons, mirroring `BUILTINS` in src-tauri/src/builtin.rs: the setting
 * that switches each one, and what it sends where (the disclosure under its switch). */
const BUILTIN_ADDONS: {
  setting: "builtinCinemeta" | "builtinKitsu" | "builtinOpensubtitles";
  name: string;
  /** a user install with this transport URL or manifest id replaces the built-in */
  transportUrl: string;
  manifestId: string;
  sub: string;
}[] = [
  {
    setting: "builtinCinemeta",
    name: "Cinemeta",
    transportUrl: "https://v3-cinemeta.strem.io/manifest.json",
    manifestId: "com.linvo.cinemeta",
    sub: "Metadata · Movie and series catalogs, plus details for IMDb titles from any catalog. Stremio's v3-cinemeta.strem.io sees your searches and the titles you browse and open.",
  },
  {
    setting: "builtinKitsu",
    name: "Anime Kitsu",
    transportUrl: "https://anime-kitsu.strem.fun/manifest.json",
    manifestId: "community.anime.kitsu",
    sub: "Metadata · Anime catalogs for Home and the Anime room, plus details for Kitsu and MyAnimeList titles. A community addon: anime-kitsu.strem.fun sees your searches and the anime you browse and open.",
  },
  {
    setting: "builtinOpensubtitles",
    name: "OpenSubtitles v3",
    transportUrl: "https://opensubtitles-v3.strem.io/manifest.json",
    manifestId: "org.stremio.opensubtitlesv3",
    sub: "Subtitles · Subtitles for movies and series in the player's Subtitles menu. When Zeiler looks for subtitles (as a video starts, see Subtitles & audio), Stremio's opensubtitles-v3.strem.io gets the title's IMDb id and, if the stream names them, the file's name, size and hash. A subtitle is downloaded from it only when it is shown.",
  },
];

function ContentSection() {
  const { settings, save } = useSettings();
  const { addons, reload } = useAddons();
  if (!settings) return <Spinner label="Loading…" />;
  return (
    <>
      <SectionCard
        title="Built-in addons"
        description="Metadata and subtitles only: none of them serves streams, so add your own addons on the Addons page to play anything. Each one is asked only while it's on."
      >
        {BUILTIN_ADDONS.map((builtin) => {
          const ownCopy = addons.some(
            (a) =>
              !a.flags.protected &&
              (a.transportUrl === builtin.transportUrl ||
                a.manifest.id === builtin.manifestId),
          );
          return (
            <ToggleRow
              key={builtin.setting}
              label={builtin.name}
              sub={
                ownCopy
                  ? `You installed ${builtin.name} yourself, so your copy is used whether this is on or off. Remove it on the Addons page to use the built-in one.`
                  : builtin.sub
              }
              on={settings[builtin.setting]}
              onChange={async (on) => {
                await save({ [builtin.setting]: on });
                await reload();
              }}
            />
          );
        })}
      </SectionCard>
      <AdultContentSetting />
      <SectionCard title="Rooms" description="Hide sidebar rooms you don't use.">
        <ToggleRow
          label="Hide Anime"
          sub="Removes the Anime room from the sidebar and anime rows from Home and Discover."
          on={settings.hideAnime}
          onChange={(on) => void save({ hideAnime: on })}
        />
        <ToggleRow
          label="Hide Live TV"
          sub="Removes the Live TV room from the sidebar; its TV and channel catalogs then show on Home and Discover, and TV catalogs under Shows."
          on={settings.hideLiveTv}
          onChange={(on) => void save({ hideLiveTv: on })}
        />
      </SectionCard>
    </>
  );
}

function IntegrationsSection() {
  return (
    <>
      <SectionCard
        title="Debrid services"
        description="Real-Debrid, TorBox, AllDebrid, Premiumize — planned, not in Zeiler yet."
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
      <SectionCard title="Zeiler">
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
      <UpdatesCard />
      <div className="rounded-2xl bg-canvas/30 p-5">
        <div className="text-[11px] font-semibold uppercase tracking-[0.14em] text-ink-subtle">
          Legal
        </div>
        <p className="mt-2 text-[13px] leading-relaxed text-ink-muted">
          Zeiler hosts no media, indexes no media, and ships no content addon. Streams come
          only from third-party addons you choose to install. Catalogs, metadata and subtitles
          come from those addons and from a few built-in metadata and subtitle addons (Cinemeta,
          Anime Kitsu, OpenSubtitles v3), none of which serves streams; you can turn each off in
          Settings → Content. Zeiler is an independent project and is not affiliated with or
          endorsed by Stremio (Smart Code Ltd).
        </p>
      </div>
    </>
  );
}

function UpdatesCard() {
  const { settings, save } = useSettings();
  const [state, setState] = useState<
    | { kind: "idle" }
    | { kind: "checking" }
    | { kind: "current" }
    | { kind: "ready"; version: string }
    | { kind: "error"; message: string }
  >({ kind: "idle" });

  useEffect(() => {
    updateStatus()
      .then((s) => s.ready && setState({ kind: "ready", version: s.ready }))
      .catch(() => {});
  }, []);

  if (!settings) return null;

  return (
    <SectionCard title="Updates" description="Signed updates from GitHub releases. Installs happen when you open Zeiler, never mid-movie.">
      <ToggleRow
        label="Install updates automatically"
        sub="Checks at launch and every few hours; a downloaded update installs the next time you open Zeiler."
        on={settings.autoUpdate}
        onChange={(on) => void save({ autoUpdate: on })}
      />
      <div className="flex flex-wrap items-center gap-3">
        <button
          disabled={state.kind === "checking"}
          onClick={async () => {
            setState({ kind: "checking" });
            try {
              const version = await updateCheck();
              setState(version ? { kind: "ready", version } : { kind: "current" });
            } catch (e) {
              setState({ kind: "error", message: errorMessage(e) });
            }
          }}
          className="flex h-10 items-center gap-2 rounded-xl border border-edge-soft px-4 text-[12.5px] font-medium text-ink-muted transition-colors hover:border-edge hover:text-ink disabled:opacity-50"
        >
          {state.kind === "checking" ? "Checking…" : "Check for updates"}
        </button>
        {state.kind === "current" && (
          <span className="text-[13px] text-ink-muted">You're on the latest version.</span>
        )}
        {state.kind === "error" && (
          <span className="text-[13px] text-[oklch(0.75_0.12_25)]">{state.message}</span>
        )}
        {state.kind === "ready" && (
          <>
            <span className="text-[13px] text-accent">v{state.version} is ready.</span>
            <button
              onClick={() =>
                updateInstallNow().catch((e) =>
                  setState({ kind: "error", message: errorMessage(e) }),
                )
              }
              className="flex h-10 items-center rounded-xl bg-ink px-4 text-[12.5px] font-semibold text-canvas transition-transform hover:scale-[1.02] active:scale-[0.97]"
            >
              Restart and update now
            </button>
          </>
        )}
      </div>
    </SectionCard>
  );
}
