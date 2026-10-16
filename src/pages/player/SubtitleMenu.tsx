/**
 * The subtitle menu: every subtitle the file, the stream, the subtitle addons and the
 * user's own files offer, by language (preferred first) and by source, plus Off/On,
 * "Load from file…", the encoding of an added subtitle, the delay, and a quick style
 * (size, position, background) that applies at once and is saved. Every action goes
 * through the player API, so it works the same for the built-in player and external
 * mpv. Keys: s next subtitle, z/x delay −/+0.1 s.
 */
import { AlertTriangle, Captions, FileUp } from "lucide-react";
import { useMemo, useState } from "react";
import { playerSetProperty } from "../../lib/player-ipc";
import { backgroundOf, backgroundPatch } from "../../lib/subtitle-style";
import { ENCODINGS, languageName } from "../../lib/subtitles";
import type { SubtitleStyle } from "../../lib/types";
import { Choice, DelayRow, PanelHeading, PopupMenu } from "./menu";
import type { MpvState } from "./mpv-state";
import type { SubtitleEntry, Tracks } from "./useTracks";

type Tab = "all" | "embedded" | "addons" | "files";

const TABS: { id: Tab; label: string; has: (e: SubtitleEntry) => boolean }[] = [
  { id: "all", label: "All", has: () => true },
  { id: "embedded", label: "In the video", has: (e) => e.kind === "embedded" },
  { id: "addons", label: "Addons", has: (e) => e.kind === "addon" || e.kind === "stream" },
  { id: "files", label: "Files", has: (e) => e.kind === "local" },
];

export function SubtitleMenu({ tracks, mpv }: { tracks: Tracks; mpv: MpvState }) {
  return (
    <PopupMenu
      label={
        <>
          <Captions size={15} />
          Subtitles
        </>
      }
      title="Subtitles (s next, z/x delay)"
      width="w-[25rem]"
      onOpen={tracks.ensureListed}
    >
      {() => <SubtitlePanel tracks={tracks} mpv={mpv} />}
    </PopupMenu>
  );
}

function SubtitlePanel({ tracks, mpv }: { tracks: Tracks; mpv: MpvState }) {
  const [lang, setLang] = useState<string | null | "all">("all");
  const [tab, setTab] = useState<Tab>("all");
  const entries = tracks.subtitles;
  const selected = entries.find((e) => e.selected) ?? null;

  // the language rail, in list order (preferred languages first)
  const languages = useMemo(() => {
    const counts = new Map<string | null, number>();
    for (const e of entries) counts.set(e.lang, (counts.get(e.lang) ?? 0) + 1);
    return [...counts.entries()];
  }, [entries]);
  const tabs = TABS.filter((t) => t.id === "all" || entries.some(t.has));
  const activeTab = tabs.find((t) => t.id === tab) ?? TABS[0];
  const shown = entries.filter((e) => (lang === "all" || e.lang === lang) && activeTab.has(e));

  return (
    <div className="flex flex-col pb-1">
      <PanelHeading
        aside={
          <button
            role="switch"
            aria-checked={tracks.enabled}
            onClick={tracks.enabled ? tracks.subtitlesOff : tracks.subtitlesOn}
            className={`rounded-md px-2 py-0.5 text-[11.5px] font-semibold ${
              tracks.enabled ? "bg-accent/20 text-accent" : "bg-raised text-ink-muted hover:text-ink"
            }`}
          >
            {tracks.enabled ? "On" : "Off"}
          </button>
        }
      >
        Subtitles
      </PanelHeading>

      {languages.length > 1 && (
        <div className="flex flex-wrap gap-1 px-3 pb-1.5" role="group" aria-label="Language">
          <RailChip active={lang === "all"} onClick={() => setLang("all")}>
            All <span className="text-ink-subtle">{entries.length}</span>
          </RailChip>
          {languages.map(([code, count]) => (
            <RailChip key={code ?? "?"} active={lang === code} onClick={() => setLang(code)}>
              {languageName(code)} <span className="text-ink-subtle">{count}</span>
            </RailChip>
          ))}
        </div>
      )}
      {tabs.length > 2 && (
        <div className="flex gap-3 border-b border-edge-soft px-3 text-[11.5px]" role="tablist">
          {tabs.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={t.id === activeTab.id}
              onClick={() => setTab(t.id)}
              className={`-mb-px border-b-2 pb-1 pt-0.5 ${
                t.id === activeTab.id
                  ? "border-accent text-ink"
                  : "border-transparent text-ink-subtle hover:text-ink-muted"
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>
      )}

      <div className="flex flex-col py-1" role="listbox" aria-label="Subtitles">
        {shown.map((entry) => (
          <EntryRow key={entry.key} entry={entry} onPick={() => tracks.selectSubtitle(entry)} />
        ))}
        {tracks.listing === "loading" && (
          <div className="flex items-center gap-2 px-3 py-1.5 text-xs text-ink-subtle">
            <span className="h-3 w-3 animate-spin rounded-full border-2 border-edge border-t-accent" />
            Asking your subtitle addons…
          </div>
        )}
        {shown.length === 0 && tracks.listing !== "loading" && (
          <div className="px-3 py-2 text-xs text-ink-subtle">
            {entries.length === 0
              ? "No subtitles found. Add a subtitle addon, or load a file."
              : "None here."}
          </div>
        )}
        {tracks.listErrors.length > 0 && (
          <div className="px-3 pt-1 text-[11px] leading-snug text-ink-subtle">
            {tracks.listErrors.join(" · ")}
          </div>
        )}
      </div>

      <div className="flex flex-col border-t border-edge-soft pt-1">
        <div className="flex items-center gap-2 px-3 py-1">
          <button
            onClick={tracks.pickLocal}
            className="flex h-7 items-center gap-1.5 rounded-md bg-raised/70 px-2 text-[11.5px] text-ink-muted hover:bg-elevated hover:text-ink"
          >
            <FileUp size={13} />
            Load from file…
          </button>
          {selected?.file && <EncodingPicker entry={selected} onPick={tracks.reencode} />}
        </div>
        <DelayRow
          label="Delay"
          value={mpv.subDelay}
          onChange={(v) => void playerSetProperty("sub-delay", v).catch(() => undefined)}
          hint="z / x: −0.1 s / +0.1 s. Positive shows subtitles later."
        />
        {tracks.style && <QuickStyle style={tracks.style} onChange={tracks.setStyle} />}
      </div>
    </div>
  );
}

function RailChip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      aria-pressed={active}
      onClick={onClick}
      className={`rounded-full px-2 py-0.5 text-[11.5px] ${
        active ? "bg-ink text-canvas" : "bg-raised/70 text-ink-muted hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}

function EntryRow({ entry, onPick }: { entry: SubtitleEntry; onPick: () => void }) {
  return (
    <button
      role="option"
      aria-selected={entry.selected}
      onClick={onPick}
      disabled={entry.status === "loading"}
      className={`flex w-full items-start gap-2 px-3 py-1.5 text-left text-sm hover:bg-elevated ${
        entry.selected ? "text-accent" : "text-ink-muted"
      }`}
    >
      <span className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center">
        {entry.status === "loading" ? (
          <span className="h-3 w-3 animate-spin rounded-full border-2 border-edge border-t-accent" />
        ) : entry.status === "error" ? (
          <AlertTriangle size={13} className="text-[oklch(0.75_0.12_25)]" />
        ) : entry.selected ? (
          "✓"
        ) : null}
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-baseline gap-2">
          <span className="truncate">{entry.title}</span>
          <span className="ml-auto shrink-0 truncate text-[10.5px] text-ink-subtle">
            {entry.tags.join(" · ")}
          </span>
        </span>
        {entry.error && (
          <span className="text-[11px] leading-snug text-[oklch(0.75_0.12_25)]">
            {entry.error} Click to try again.
          </span>
        )}
      </span>
    </button>
  );
}

/** Read the showing subtitle again as another encoding (for garbled text). */
function EncodingPicker({ entry, onPick }: { entry: SubtitleEntry; onPick: (label: string | null) => void }) {
  const current = ENCODINGS.find((e) => e.label.toLowerCase() === entry.encoding?.toLowerCase());
  return (
    <label className="ml-auto flex items-center gap-1.5 text-[11.5px] text-ink-subtle" title="Garbled letters? Read the subtitle as another encoding.">
      <select
        aria-label="Subtitle encoding"
        value=""
        onChange={(e) => onPick(e.target.value === "auto" ? null : e.target.value)}
        className="h-7 max-w-[12rem] rounded-md border border-edge bg-canvas px-1.5 text-[11.5px] text-ink outline-none"
      >
        <option value="" disabled>
          Encoding: {current?.name ?? entry.encoding ?? "detected"}
        </option>
        <option value="auto">Detect again</option>
        {ENCODINGS.map((e) => (
          <option key={e.label} value={e.label}>
            {e.name}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Size, position and background, applied at once and saved for every video. */
function QuickStyle({
  style,
  onChange,
}: {
  style: SubtitleStyle;
  onChange: (patch: Partial<SubtitleStyle>) => void;
}) {
  const box = backgroundOf(style) === "box";
  return (
    <div className="flex flex-col gap-1.5 px-3 py-1.5">
      <StyleLine label="Size">
        <Choice
          label="Size"
          value={style.scale ?? 1}
          options={[
            { value: 0.8, label: "S" },
            { value: 1, label: "M" },
            { value: 1.25, label: "L" },
            { value: 1.5, label: "XL" },
          ]}
          onChange={(scale) => onChange({ scale })}
        />
      </StyleLine>
      <StyleLine label="Position">
        <Choice
          label="Position"
          value={style.pos ?? 100}
          options={[
            { value: 100, label: "Bottom" },
            { value: 92, label: "Raised" },
            { value: 82, label: "Higher" },
          ]}
          onChange={(pos) => onChange({ pos })}
        />
      </StyleLine>
      <StyleLine label="Background">
        <Choice
          label="Background"
          value={box ? "box" : "none"}
          options={[
            { value: "none", label: "None" },
            { value: "box", label: "Box" },
          ]}
          onChange={(v) => onChange(backgroundPatch(v === "box" ? "box" : "outline"))}
        />
      </StyleLine>
    </div>
  );
}

function StyleLine({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-[4.5rem] shrink-0 text-xs text-ink-subtle">{label}</span>
      {children}
    </div>
  );
}
