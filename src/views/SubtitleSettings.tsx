/**
 * Settings → Subtitles & audio: preferred subtitle and audio languages (in order),
 * whether a subtitle shows by itself, and how text subtitles look, with a preview. The
 * values are the `subtitles{}` and `audio{}` settings groups; every change patches just
 * the field it changes (Rust merges it into the group).
 */
import { ArrowUp, X } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { SectionCard, Spinner, ToggleRow } from "../components/ui";
import { useSettings } from "../lib/store";
import {
  backgroundOf,
  backgroundPatch,
  type Background,
  NO_STYLE,
  previewStyle,
  type Shadow,
  shadowOf,
  shadowPatch,
  STYLE_CHOICES,
  SUBTITLE_FONTS,
} from "../lib/subtitle-style";
import { COMMON_LANGUAGES, effectiveLanguages, languageName } from "../lib/subtitles";
import type { SubtitleStyle } from "../lib/types";

/** Most languages a list holds (Rust refuses more). */
const MAX_LANGUAGES = 12;

export function SubtitlesSection() {
  const { settings, save } = useSettings();
  if (!settings) return <Spinner label="Loading…" />;
  const { subtitles, audio } = settings;
  const [system, ...fallback] = effectiveLanguages([]).map(languageName);
  const automatic = [`${system} (your system's language)`, ...fallback].join(", then ");
  const setStyle = (patch: Partial<SubtitleStyle>) => void save({ subtitles: { style: patch } });

  return (
    <>
      <SectionCard
        title="Subtitle languages"
        description="The first language that has a subtitle wins: one in the video, else one your subtitle addons find."
      >
        <LanguageList
          label="Subtitle languages"
          value={subtitles.langs}
          empty={`Automatic: ${automatic}.`}
          onChange={(langs) => void save({ subtitles: { langs } })}
        />
        <ToggleRow
          label="Show subtitles automatically"
          sub="When a video starts, show the best subtitle in your languages. Off: none shows until you pick one in the player's Subtitles menu. What you pick for a show (or turning subtitles off) is remembered for its next episodes."
          on={subtitles.autoLoad}
          onChange={(on) => void save({ subtitles: { autoLoad: on } })}
        />
        <p className="text-[12.5px] leading-relaxed text-ink-subtle">
          Your subtitle addons, the built-in OpenSubtitles v3 among them, are asked for subtitles
          when a video starts — unless you turned subtitles off for that show, or automatic
          subtitles are off and you have not picked one for the show before; then only once you
          open the Subtitles menu. They get the title's id and, if the stream names them, the
          file's name, size and hash. A subtitle is downloaded only when it is shown.
        </p>
      </SectionCard>

      <SectionCard
        title="Audio languages"
        description="When a video has more than one audio track, play the first of these it has."
      >
        <LanguageList
          label="Audio languages"
          value={audio.langs}
          empty="The video's own choice."
          onChange={(langs) => void save({ audio: { langs } })}
        />
        <div className="flex flex-col gap-2">
          <span className="text-[12px] font-medium uppercase tracking-[0.14em] text-ink-subtle">
            For anime
          </span>
          <LanguageList
            label="Anime audio languages"
            value={audio.animeLangs}
            empty="The video's own choice."
            onChange={(animeLangs) => void save({ audio: { animeLangs } })}
          />
        </div>
        <ToggleRow
          label="Normalize the volume"
          sub="Quiet dialogue louder, loud scenes quieter. Also in the player's Audio menu."
          on={audio.normalize}
          onChange={(on) => void save({ audio: { normalize: on } })}
        />
        <StyleRow label="Loudest volume">
          <Options
            choices={[100, 130, 150, 200].map((v) => ({ value: v, label: `${v}%` }))}
            value={audio.volumeMax}
            onChange={(volumeMax) => void save({ audio: { volumeMax } })}
          />
        </StyleRow>
        <p className="text-[12.5px] leading-relaxed text-ink-subtle">
          Above 100% the sound is amplified, which can distort. Applies from the next video.
        </p>
      </SectionCard>

      <SectionCard
        title="Subtitle look"
        description="For text subtitles. Styled (ASS) subtitles, common for anime, keep their own look unless you turn on the last switch. With external mpv, only what you set here replaces your mpv settings."
      >
        <StylePreview style={subtitles.style} />
        <StyleRow label="Size">
          <Options
            choices={STYLE_CHOICES.scale}
            value={subtitles.style.scale}
            onChange={(scale) => setStyle({ scale })}
          />
        </StyleRow>
        <StyleRow label="Font size">
          <Options
            choices={STYLE_CHOICES.fontSize}
            value={subtitles.style.fontSize}
            onChange={(fontSize) => setStyle({ fontSize })}
          />
        </StyleRow>
        <StyleRow label="Font">
          <select
            aria-label="Subtitle font"
            value={subtitles.style.font ?? ""}
            onChange={(e) => setStyle({ font: e.target.value || null })}
            className="h-9 rounded-xl border border-edge bg-canvas px-3 text-[13px] text-ink outline-none focus:border-ink"
          >
            <option value="">Default</option>
            {SUBTITLE_FONTS.map((font) => (
              <option key={font} value={font}>
                {font}
              </option>
            ))}
          </select>
        </StyleRow>
        <StyleRow label="Color">
          <Options
            choices={STYLE_CHOICES.color}
            value={subtitles.style.color}
            onChange={(color) => setStyle({ color })}
          />
        </StyleRow>
        <StyleRow label="Outline">
          <Options
            choices={STYLE_CHOICES.outlineSize}
            value={subtitles.style.outlineSize}
            onChange={(outlineSize) => setStyle({ outlineSize })}
          />
        </StyleRow>
        <StyleRow label="Background">
          <Options<Background>
            choices={[
              { value: "default", label: "Default" },
              { value: "outline", label: "None" },
              { value: "box", label: "Box" },
            ]}
            value={backgroundOf(subtitles.style)}
            onChange={(background) => setStyle(backgroundPatch(background))}
          />
        </StyleRow>
        <StyleRow label="Shadow">
          <Options<Shadow>
            choices={[
              { value: "default", label: "Default" },
              { value: "none", label: "None" },
              { value: "soft", label: "Soft" },
              { value: "strong", label: "Strong" },
            ]}
            value={shadowOf(subtitles.style)}
            onChange={(shadow) => setStyle(shadowPatch(shadow, subtitles.style))}
          />
        </StyleRow>
        <StyleRow label="Position">
          <Options
            choices={STYLE_CHOICES.pos}
            value={subtitles.style.pos}
            onChange={(pos) => setStyle({ pos })}
          />
        </StyleRow>
        <StyleRow label="Alignment">
          <Options
            choices={STYLE_CHOICES.alignX}
            value={subtitles.style.alignX}
            onChange={(alignX) => setStyle({ alignX })}
          />
        </StyleRow>
        <StyleRow label="Placement">
          <Options
            choices={STYLE_CHOICES.alignY}
            value={subtitles.style.alignY}
            onChange={(alignY) => setStyle({ alignY })}
          />
        </StyleRow>
        <ToggleRow
          label="Bold"
          on={subtitles.style.bold === true}
          onChange={(on) => setStyle({ bold: on ? true : null })}
        />
        <ToggleRow
          label="Use this look for styled (ASS) subtitles too"
          sub="Replaces their fonts, colours and positions, which can break signs and karaoke in anime."
          on={subtitles.style.assOverride === "force"}
          onChange={(on) => setStyle({ assOverride: on ? "force" : null })}
        />
        <div>
          <button
            onClick={() => setStyle(NO_STYLE)}
            className="flex h-10 items-center rounded-xl border border-edge-soft px-4 text-[12.5px] font-medium text-ink-muted transition-colors hover:border-edge hover:text-ink"
          >
            Reset to the player's own look
          </button>
        </div>
      </SectionCard>
    </>
  );
}

function StyleRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <span className="w-24 shrink-0 text-[13px] text-ink-muted">{label}</span>
      {children}
    </div>
  );
}

/** Chips for one of `choices` (null: the player's default). */
function Options<T extends string | number | null>({
  choices,
  value,
  onChange,
}: {
  choices: readonly { value: T; label: string }[];
  value: string | number | null;
  onChange: (value: T) => void;
}) {
  return (
    <div className="flex flex-wrap gap-2">
      {choices.map((choice) => (
        <button
          key={String(choice.value)}
          aria-pressed={choice.value === value}
          onClick={() => onChange(choice.value)}
          className={`rounded-full px-3.5 py-1.5 text-[12px] font-semibold transition-colors ${
            choice.value === value
              ? "bg-ink text-canvas"
              : "bg-raised text-ink-muted hover:bg-elevated hover:text-ink"
          }`}
        >
          {choice.label}
        </button>
      ))}
    </div>
  );
}

/** A still "frame" with a line of subtitle text in roughly the chosen look. */
function StylePreview({ style }: { style: SubtitleStyle }) {
  const height = 200;
  return (
    <div
      aria-hidden
      className="relative flex h-[200px] items-end justify-center overflow-hidden rounded-xl"
      style={{
        background:
          "radial-gradient(circle at 30% 30%, oklch(0.55 0.08 60), transparent 55%), linear-gradient(160deg, oklch(0.42 0.06 230), oklch(0.2 0.03 260))",
      }}
    >
      <span className="absolute max-w-[90%] text-center leading-tight" style={previewStyle(style, height)}>
        Where were you last night?
      </span>
    </div>
  );
}

/** An ordered list of languages: add, move up, remove. */
function LanguageList({
  label,
  value,
  empty,
  onChange,
}: {
  label: string;
  value: string[];
  empty: string;
  onChange: (langs: string[]) => void;
}) {
  const [adding, setAdding] = useState("");
  const options = useMemo(
    () =>
      COMMON_LANGUAGES.filter((code) => !value.includes(code))
        .map((code) => ({ code, name: languageName(code) }))
        .sort((a, b) => a.name.localeCompare(b.name)),
    [value],
  );
  return (
    <div className="flex flex-col gap-2" aria-label={label} role="group">
      {value.length === 0 ? (
        <span className="text-[13px] text-ink-muted">{empty}</span>
      ) : (
        <ol className="flex flex-wrap gap-2">
          {value.map((code, i) => (
            <li
              key={code}
              className="flex h-9 items-center gap-1.5 rounded-full border border-edge-soft bg-canvas/40 pl-3 pr-1 text-[13px] text-ink"
            >
              <span className="text-[11px] text-ink-subtle">{i + 1}</span>
              {languageName(code)}
              {i > 0 && (
                <button
                  aria-label={`Prefer ${languageName(code)} earlier`}
                  title="Earlier"
                  onClick={() => onChange([...value.slice(0, i - 1), code, value[i - 1], ...value.slice(i + 1)])}
                  className="flex h-7 w-7 items-center justify-center rounded-full text-ink-subtle hover:bg-elevated hover:text-ink"
                >
                  <ArrowUp size={13} />
                </button>
              )}
              <button
                aria-label={`Remove ${languageName(code)}`}
                title="Remove"
                onClick={() => onChange(value.filter((c) => c !== code))}
                className="flex h-7 w-7 items-center justify-center rounded-full text-ink-subtle hover:bg-elevated hover:text-ink"
              >
                <X size={13} />
              </button>
            </li>
          ))}
        </ol>
      )}
      {value.length < MAX_LANGUAGES && (
        <select
          aria-label={`Add to ${label.toLowerCase()}`}
          value={adding}
          onChange={(e) => {
            const code = e.target.value;
            setAdding("");
            if (code) onChange([...value, code]);
          }}
          className="h-9 w-56 rounded-xl border border-edge bg-canvas px-3 text-[13px] text-ink-muted outline-none focus:border-ink"
        >
          <option value="">Add a language…</option>
          {options.map((o) => (
            <option key={o.code} value={o.code}>
              {o.name}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}
