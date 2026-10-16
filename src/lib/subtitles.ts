/**
 * Subtitle and audio choices for the player: language codes and names, the candidates
 * every source offers, their ranking and the automatic pick. Pure functions; the
 * player's hook (src/pages/player/useTracks.ts) feeds them mpv's track list, the
 * stream's own subtitles and the `subtitles_get` fan-out.
 *
 * Languages are kept as ISO 639-1 codes, with a BCP-47 region where it matters
 * (`pt-BR`). Files and addons use ISO 639-2 (`eng`, `fre`/`fra`), OpenSubtitles' own
 * codes (`pob`, `zht`) or names; `normalizeLang` maps them all. The 639-1 → 639-2 table
 * is `languages.json`, which Rust uses for mpv's `alang`/`slang` too.
 *
 * Ranking: preferred language first (an exact region before the bare language), then
 * the source (subtitles the stream lists, then addons in their order, which puts the
 * built-in OpenSubtitles v3 first), then how well a label matches the stream's file name
 * (release group, source, resolution; hearing-impaired variants last).
 */
import table from "./languages.json";
import type { MpvTrack } from "./player-ipc";
import type { AddonSubtitles, Subtitle } from "./types";

// --- languages ------------------------------------------------------------------------------

const ISO_639_1 = table as Record<string, string[]>;
const FROM_639_2 = new Map<string, string>();
for (const [one, twos] of Object.entries(ISO_639_1)) {
  for (const two of twos) FROM_639_2.set(two, one);
}

/** OpenSubtitles' codes and other spellings addons use. */
const SPECIAL: Record<string, string> = {
  pob: "pt-BR",
  pb: "pt-BR",
  "pt-br": "pt-BR",
  zht: "zh-TW",
  zhe: "zh",
  ze: "zh",
  "zh-tw": "zh-TW",
  "zh-hk": "zh-HK",
  "zh-cn": "zh",
  scc: "sr",
  scr: "hr",
  spn: "es",
  ea: "es",
  mne: "sr",
  iw: "he",
  in: "id",
  jw: "jv",
};

const NO_LANGUAGE = new Set(["und", "unk", "mul", "zxx", "mis", "none", "unknown", "xx"]);

let namesToCodes: Map<string, string> | null = null;

/** English language names ("Arabic") → codes, for addons that send names. */
function codeForName(name: string): string | null {
  if (!namesToCodes) {
    namesToCodes = new Map();
    const names = displayNames();
    for (const code of Object.keys(ISO_639_1)) {
      const display = names?.of(code);
      if (display) namesToCodes.set(display.toLowerCase(), code);
    }
  }
  return namesToCodes.get(name.toLowerCase()) ?? null;
}

let names: Intl.DisplayNames | null | undefined;
function displayNames(): Intl.DisplayNames | null {
  if (names === undefined) {
    try {
      names = new Intl.DisplayNames(["en"], { type: "language" });
    } catch {
      names = null;
    }
  }
  return names;
}

/** A language as Zeiler compares it, or null for none/unknown. */
export function normalizeLang(raw: string | null | undefined): string | null {
  const text = raw?.trim().replace(/_/g, "-");
  if (!text) return null;
  const lower = text.toLowerCase();
  if (lower in SPECIAL) return SPECIAL[lower];
  if (NO_LANGUAGE.has(lower)) return null;
  const [primary, ...rest] = lower.split("-");
  if (!/^[a-z]{2,3}$/.test(primary)) return codeForName(text);
  const base = primary.length === 3 ? (FROM_639_2.get(primary) ?? primary) : primary;
  if (rest.length === 0) return base;
  const subtags = rest.map((part) =>
    part.length === 2 ? part.toUpperCase() : part.length === 4 ? part[0].toUpperCase() + part.slice(1) : part,
  );
  return [base, ...subtags].join("-");
}

export function baseLang(code: string): string {
  return code.split("-")[0];
}

/** "Arabic", "Portuguese (Brazil)"; the code itself when the name is unknown. */
export function languageName(code: string | null): string {
  if (!code) return "Unknown language";
  try {
    return displayNames()?.of(code) ?? code.toUpperCase();
  } catch {
    return code.toUpperCase();
  }
}

/** The languages a list of settings codes stands for; empty means the system's language,
 * then English. */
export function effectiveLanguages(langs: string[]): string[] {
  const chosen = langs.map(normalizeLang).filter((l): l is string => l !== null);
  if (chosen.length > 0) return dedupe(chosen);
  const system = normalizeLang(navigator.language);
  const systemLang = system && (system === "pt-BR" ? system : baseLang(system));
  return dedupe([systemLang, "en"].filter((l): l is string => !!l));
}

function dedupe(langs: string[]): string[] {
  return langs.filter((l, i) => langs.indexOf(l) === i);
}

/** How well `lang` fits the preferences: 0 is the first one exactly; Infinity is none. */
export function langRank(lang: string | null, prefs: string[]): number {
  if (!lang) return Infinity;
  let best = Infinity;
  prefs.forEach((pref, i) => {
    if (lang === pref) best = Math.min(best, i * 2);
    else if (baseLang(lang) === baseLang(pref)) best = Math.min(best, i * 2 + 1);
  });
  return best;
}

/** A small set of the languages people pick, for the settings picker (codes). */
export const COMMON_LANGUAGES = [
  "en", "ar", "zh", "zh-TW", "cs", "da", "nl", "fi", "fr", "de", "el", "he", "hi", "hu",
  "id", "it", "ja", "ko", "ms", "no", "fa", "pl", "pt", "pt-BR", "ro", "ru", "sr", "es",
  "sv", "th", "tr", "uk", "vi", "bg", "hr", "sk", "sl", "et", "lv", "lt", "bn", "ta",
  "te", "ur", "ca", "eu", "gl", "is", "mk", "sq", "bs", "ka", "hy", "az", "kk", "uz",
  "tl", "sw", "af",
];

// --- candidates -----------------------------------------------------------------------------

export interface SubtitleCandidate {
  /** stable across renders: source and URL */
  key: string;
  url: string;
  lang: string | null;
  /** the addon's own label, if it gave one */
  label: string | null;
  source: "stream" | "addon";
  /** the addon that listed it (stream subtitles: the stream's addon, if known) */
  addonName: string | null;
  hearingImpaired: boolean;
  /** how well the label matches the stream's file name */
  match: number;
  /** stream subtitles first, then each addon in order */
  sourceOrder: number;
  order: number;
}

const HEARING_IMPAIRED = /(^|[^a-z])(sdh|cc|hi|hearing[ ._-]?impaired)([^a-z]|$)/i;

/** Every subtitle the stream and the addons offer, deduplicated and ranked. */
export function buildCandidates({
  streamSubtitles,
  streamAddon,
  groups,
  filename,
  prefs,
}: {
  streamSubtitles: Subtitle[];
  streamAddon: string | null;
  groups: AddonSubtitles[];
  filename: string | null;
  prefs: string[];
}): SubtitleCandidate[] {
  const seen = new Set<string>();
  const out: SubtitleCandidate[] = [];
  let order = 0;
  const push = (sub: Subtitle, source: "stream" | "addon", addonName: string | null, sourceOrder: number) => {
    if (!/^https?:\/\//i.test(sub.url)) return;
    const lang = normalizeLang(sub.lang);
    const key = `${lang ?? "?"}|${sub.url}`;
    const idKey = sub.id ? `${sourceOrder}|id|${sub.id}` : null;
    if (seen.has(key) || (idKey && seen.has(idKey))) return;
    seen.add(key);
    if (idKey) seen.add(idKey);
    const label = sub.label?.trim() || null;
    const text = `${label ?? ""} ${sub.url}`;
    out.push({
      key: `${source}|${sub.url}`,
      url: sub.url,
      lang,
      label,
      source,
      addonName,
      hearingImpaired: HEARING_IMPAIRED.test(label ?? ""),
      match: releaseMatch(text, filename),
      sourceOrder,
      order: order++,
    });
  };
  for (const sub of streamSubtitles) push(sub, "stream", streamAddon, -1);
  groups.forEach((group, i) => {
    for (const sub of group.subtitles) push(sub, "addon", group.addonName, i);
  });
  // hearing-impaired variants carry extra text: last among equals
  const fit = (c: SubtitleCandidate) => c.match - (c.hearingImpaired ? 25 : 0);
  return out.sort(
    (a, b) =>
      langRank(a.lang, prefs) - langRank(b.lang, prefs) ||
      a.sourceOrder - b.sourceOrder ||
      fit(b) - fit(a) ||
      a.order - b.order,
  );
}

const SOURCES = ["bluray", "bdrip", "brrip", "web-dl", "webdl", "webrip", "web", "hdtv", "dvdrip", "hdrip", "remux"];
const RESOLUTIONS = ["2160p", "1080p", "720p", "576p", "480p"];

/** How well a subtitle's label matches the video's file name: the release group counts
 * most, then the source, then the resolution. */
export function releaseMatch(label: string, filename: string | null): number {
  if (!filename || !label.trim()) return 0;
  const tokens = (s: string) =>
    s.toLowerCase().replace(/\.[a-z0-9]{2,4}$/, "").split(/[^a-z0-9-]+/).filter(Boolean);
  const file = tokens(filename);
  const sub = new Set(tokens(label));
  let score = 0;
  // the release group: the part after the last "-" of the name
  const group = filename.replace(/\.[a-z0-9]{2,4}$/i, "").match(/-([a-z0-9]+)$/i)?.[1]?.toLowerCase();
  if (group && sub.has(group)) score += 120;
  if (SOURCES.some((s) => file.includes(s) && sub.has(s))) score += 40;
  if (RESOLUTIONS.some((r) => file.includes(r) && sub.has(r))) score += 8;
  return score;
}

// --- the automatic pick ---------------------------------------------------------------------

/** What the automatic pick chose, and in which preferred language (its index). */
export type AutomaticPick =
  | { kind: "keep"; lang: string | null; prefIndex: number }
  | { kind: "track"; id: number; lang: string | null; prefIndex: number }
  | { kind: "candidate"; candidate: SubtitleCandidate; lang: string | null; prefIndex: number }
  | { kind: "none" };

/**
 * What to show when a file starts, language by language in preference order: for the
 * first language that has a full (not forced) subtitle anywhere, what mpv picked if it is
 * in that language, else a full embedded track in it, else its best candidate.
 */
export function pickAutomatic({
  tracks,
  sid,
  candidates,
  prefs,
}: {
  tracks: MpvTrack[];
  sid: string;
  candidates: SubtitleCandidate[];
  prefs: string[];
}): AutomaticPick {
  const subs = tracks.filter((t) => t.type === "sub");
  const selected = subs.find((t) => String(t.id) === sid || (t.selected === true && sid !== "no"));
  for (const [prefIndex, pref] of prefs.entries()) {
    const fits = (lang: string | null) => !!lang && baseLang(lang) === baseLang(pref);
    if (selected && !selected.forced && fits(normalizeLang(selected.lang))) {
      return { kind: "keep", lang: normalizeLang(selected.lang), prefIndex };
    }
    const embedded = subs
      .filter((t) => !t.forced && !t.external && fits(normalizeLang(t.lang)))
      .sort((a, b) => Number(!!a["hearing-impaired"]) - Number(!!b["hearing-impaired"]))[0];
    if (embedded) {
      return { kind: "track", id: embedded.id, lang: normalizeLang(embedded.lang), prefIndex };
    }
    const candidate = candidates.find((c) => fits(c.lang));
    if (candidate) return { kind: "candidate", candidate, lang: candidate.lang, prefIndex };
  }
  return { kind: "none" };
}


// --- labels ---------------------------------------------------------------------------------

/** "Stereo", "5.1", "7.1" from mpv's channel count. */
export function channelLabel(track: MpvTrack): string | null {
  const count = track["demux-channel-count"];
  if (!count) return null;
  if (count === 1) return "Mono";
  if (count === 2) return "Stereo";
  if (count === 6) return "5.1";
  if (count === 8) return "7.1";
  return `${count} ch`;
}

/** Codec names as people know them. */
export function codecLabel(codec: string | undefined): string | null {
  if (!codec) return null;
  const known: Record<string, string> = {
    subrip: "SRT",
    ass: "ASS",
    ssa: "SSA",
    webvtt: "WebVTT",
    mov_text: "Text",
    hdmv_pgs_subtitle: "PGS",
    dvd_subtitle: "VobSub",
    dvb_subtitle: "DVB",
    aac: "AAC",
    ac3: "AC-3",
    eac3: "E-AC-3",
    dts: "DTS",
    truehd: "TrueHD",
    opus: "Opus",
    flac: "FLAC",
    mp3: "MP3",
    vorbis: "Vorbis",
  };
  return known[codec] ?? codec.toUpperCase();
}

/** The text encodings the subtitle menu offers (an encoding_rs label, and its name). */
export const ENCODINGS: { label: string; name: string }[] = [
  { label: "utf-8", name: "UTF-8" },
  { label: "windows-1256", name: "Arabic (Windows-1256)" },
  { label: "windows-1251", name: "Cyrillic (Windows-1251)" },
  { label: "windows-1252", name: "Western (Windows-1252)" },
  { label: "iso-8859-2", name: "Central European (ISO-8859-2)" },
  { label: "iso-8859-7", name: "Greek (ISO-8859-7)" },
  { label: "iso-8859-8", name: "Hebrew (ISO-8859-8)" },
  { label: "iso-8859-9", name: "Turkish (ISO-8859-9)" },
  { label: "gb18030", name: "Chinese (GB18030)" },
  { label: "big5", name: "Traditional Chinese (Big5)" },
  { label: "shift_jis", name: "Japanese (Shift_JIS)" },
  { label: "euc-kr", name: "Korean (EUC-KR)" },
];
