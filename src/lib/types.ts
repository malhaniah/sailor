// Mirrors of the Rust IPC types (serde rename_all = camelCase).

export interface ExtraProp {
  name: string;
  isRequired: boolean;
  options: string[];
  optionsLimit: number;
}

export interface ManifestCatalog {
  type: string;
  id: string;
  name?: string | null;
  // flattened ManifestExtra: either the modern form…
  extra?: ExtraProp[];
  // …or the legacy short form
  extraRequired?: string[];
  extraSupported?: string[];
}

export type ManifestResource =
  | string
  | { name: string; types?: string[] | null; idPrefixes?: string[] | null };

export interface ManifestBehaviorHints {
  adult: boolean;
  p2p: boolean;
  configurable: boolean;
  configurationRequired: boolean;
  [key: string]: unknown;
}

export interface Manifest {
  id: string;
  name: string;
  version?: string | null;
  description?: string | null;
  contactEmail?: string | null;
  logo?: string | null;
  background?: string | null;
  types: string[];
  resources: ManifestResource[];
  idPrefixes?: string[] | null;
  catalogs: ManifestCatalog[];
  addonCatalogs: ManifestCatalog[];
  behaviorHints: ManifestBehaviorHints;
}

export interface Descriptor {
  manifest: Manifest;
  transportUrl: string;
  flags: { official: boolean; protected: boolean };
}

export type PosterShape = "poster" | "square" | "landscape";

export interface MetaPreview {
  id: string;
  type: string;
  name: string;
  poster?: string | null;
  background?: string | null;
  logo?: string | null;
  posterShape: PosterShape;
  description?: string | null;
  releaseInfo?: string | null;
  imdbRating?: string | null;
  genres: string[];
}

export interface Video {
  id: string;
  title?: string | null;
  season?: number | null;
  episode?: number | null;
  released?: string | null;
  overview?: string | null;
  thumbnail?: string | null;
  streams: Stream[];
}

export interface MetaLink {
  name: string;
  category: string;
  url: string;
}

export interface MetaItem {
  id: string;
  type: string;
  name: string;
  poster?: string | null;
  background?: string | null;
  logo?: string | null;
  posterShape: PosterShape;
  description?: string | null;
  releaseInfo?: string | null;
  imdbRating?: string | null;
  runtime?: string | null;
  released?: string | null;
  genres: string[];
  website?: string | null;
  links: MetaLink[];
  videos: Video[];
  behaviorHints: {
    defaultVideoId?: string | null;
    isLive: boolean;
    hasScheduledVideos: boolean;
    [key: string]: unknown;
  };
}

export interface Subtitle {
  id?: string | null;
  url: string;
  lang: string;
  label?: string | null;
}

export interface StreamBehaviorHints {
  notWebReady: boolean;
  bingeGroup?: string | null;
  filename?: string | null;
  videoHash?: string | null;
  videoSize?: number | null;
  proxyHeaders?: {
    request: Record<string, string>;
    response: Record<string, string>;
  } | null;
  [key: string]: unknown;
}

// StreamSource is flattened into the stream object: exactly one source group is present.
export interface Stream {
  name?: string | null;
  description?: string | null;
  subtitles: Subtitle[];
  behaviorHints: StreamBehaviorHints;
  // Url
  url?: string;
  // YouTube
  ytId?: string;
  // Torrent
  infoHash?: string;
  fileIdx?: number | null;
  announce?: string[];
  fileMustInclude?: string[];
  // PlayerFrame / External
  playerFrameUrl?: string;
  externalUrl?: string;
}

export interface MetaLookup {
  meta: MetaItem | null;
  /** addons that serve meta for this type/id; empty = none installed */
  asked: string[];
  /** "<addon>: <reason>" per failed addon */
  errors: string[];
}

export interface CatalogResponse {
  metas: MetaPreview[];
  /** how many titles the addon sent before adult ones were taken out (the next skip) */
  fetched: number;
}

export interface AddonStreams {
  transportUrl: string;
  addonName: string;
  streams: Stream[];
  error?: string | null;
}

// Mirrors zeiler_engine::ResolvedStream (crates/zeiler-engine/src/engine.rs).
export interface ResolvedStream {
  streamUrl: string;
  infoHash: string;
  fileIdx: number;
  fileName: string;
  fileSize: number;
}

// Device settings (Rust `state::Settings`). Keys reserved for later features are listed
// in docs/ARCHITECTURE.md ("Settings schema"); add them here only when they ship.
export interface Settings {
  mpvPath?: string | null;
  engineListenPort: number;
  engineUpnp: boolean;
  uploadLimitKibps?: number | null;
  downloadLimitKibps?: number | null;
  deleteOnStop: boolean;
  theme: string;
  instantPlay: boolean;
  hideAnime: boolean;
  hideLiveTv: boolean;
  /** Show adult catalogs, genres and titles (Rust `zeiler_addon::is_adult_text`). */
  showAdult: boolean;
  /** Home's catalog rows as the user arranged them (Rust `home_rows::HomeRows`). */
  homeRows: HomeRows;
  builtinCinemeta: boolean;
  builtinKitsu: boolean;
  builtinOpensubtitles: boolean;
  autoUpdate: boolean;
  /** Which player `player_start` uses for "auto" (Rust `zeiler_player::PlayerBackend`). */
  playerBackend: PlayerBackend;
  /** Rust `settings_groups::SubtitleSettings`. A patch names only the fields it changes. */
  subtitles: SubtitleSettings;
  /** Rust `settings_groups::AudioSettings`. */
  audio: AudioSettings;
  /** Rust `settings_groups::SkipSettings`. */
  skip: SkipSettings;
  /** Play the next episode by itself when one ends. */
  bingeWatching: boolean;
  /** Seconds before the end the Up Next card shows, 5–60. */
  upNextSeconds: number;
  /** Browse third-party addon directories on the Addons page (opt-in). */
  marketplaceEnabled: boolean;
  marketplaceSources: MarketplaceSources;
  /** Hide addons that declare P2P from the directory (they're flagged either way). */
  hideP2pAddons: boolean;
  /** Read-only here: change it with `deeplinkSetStremio`, which also registers with the OS. */
  stremioDeepLink: boolean;
}

export interface MarketplaceSources {
  stremioAddonsNet: boolean;
  stremioCatalog: boolean;
}

export interface SkipSettings {
  /** Show a Skip button during intros, recaps and credits. */
  showButton: boolean;
  /** Skip these by themselves (with an Undo); off by default. */
  autoSkipIntro: boolean;
  autoSkipRecap: boolean;
  autoSkipCredits: boolean;
  /** Ask AniSkip and TheIntroDB where they are (else only the file's chapter names). */
  onlineLookup: boolean;
}

/** Row keys are `catalogKey`s (src/lib/catalogs.ts), never transport URLs. Each list is
 * replaced whole by a patch. */
export interface HomeRows {
  order: string[];
  hidden: string[];
  renamed: { key: string; name: string }[];
}

export interface SubtitleSettings {
  /** Preferred subtitle languages (ISO 639-1 or BCP-47), most preferred first; empty =
   * the system language, then English. */
  langs: string[];
  /** Show the best subtitle in a preferred language when a file starts. */
  autoLoad: boolean;
  style: SubtitleStyle;
}

/** How text subtitles look; null = mpv's default (or external mpv's own config). Values
 * are mpv's, checked by Rust against the player's property rules. */
export interface SubtitleStyle {
  /** sub-scale, 0.3–3 */
  scale: number | null;
  /** sub-font-size, 10–120 (mpv's 55 by default; scale multiplies it) */
  fontSize: number | null;
  font: string | null;
  bold: boolean | null;
  /** #RRGGBB or #AARRGGBB (alpha first, as mpv reads it) */
  color: string | null;
  outlineColor: string | null;
  /** 0–10 */
  outlineSize: number | null;
  borderStyle: "outline-and-shadow" | "opaque-box" | "background-box" | null;
  backColor: string | null;
  shadowOffset: number | null;
  shadowColor: string | null;
  /** sub-pos, 0–150 (100 = the bottom of the video) */
  pos: number | null;
  marginY: number | null;
  alignX: "left" | "center" | "right" | null;
  alignY: "top" | "center" | "bottom" | null;
  /** "force" applies the style to styled (ASS) subtitles too */
  assOverride: "no" | "yes" | "scale" | "force" | "strip" | null;
}

export interface AudioSettings {
  /** Preferred audio languages; empty = the file's default track. */
  langs: string[];
  /** Used instead of `langs` for anime (Kitsu, MyAnimeList, AniList, AniDB titles). */
  animeLangs: string[];
  /** Even out the volume (quiet dialogue up, loud scenes down). */
  normalize: boolean;
  /** The loudest the volume goes, in percent (100–200). */
  volumeMax: number;
}

/** A settings update: top-level keys, and for the groups only the fields that change. */
export type SettingsPatch = {
  [K in keyof Settings]?: Settings[K] extends unknown[]
    ? Settings[K]
    : Settings[K] extends object
      ? DeepPartial<Settings[K]>
      : Settings[K];
};

type DeepPartial<T> = {
  [K in keyof T]?: T[K] extends unknown[] ? T[K] : T[K] extends object ? DeepPartial<T[K]> : T[K];
};

/** One addon's answer to `subtitles_get` (Rust `zeiler_addon::AddonSubtitles`). */
export interface AddonSubtitles {
  transportUrl: string;
  addonName: string;
  subtitles: Subtitle[];
  error?: string | null;
}

/** Player backend preference. */
export type PlayerBackend = "auto" | "embedded" | "external";

/** `kind` mirrors the Rust `IpcError` variants (src-tauri/src/error.rs). */
export type IpcErrorKind =
  | "invalidInput"
  | "addon"
  | "notFound"
  | "engine"
  | "player"
  | "storage"
  // reserved for later milestones (marketplace, account, profiles, player backends)
  | "network"
  | "auth"
  | "sessionExpired"
  | "locked"
  | "conflict"
  | "unsupported";

export interface IpcError {
  kind: IpcErrorKind;
  /** a full sentence, safe to show as-is */
  message: string;
}

// ---- marketplace (Rust commands/addons.rs, commands/marketplace.rs) ------------------

export type MatchKind = "sameUrl" | "sameId" | "sameHost";

export interface ExistingMatch {
  kind: MatchKind;
  name: string;
  transportUrl: string;
  maskedUrl: string;
}

/** `addon_preview`: what the install confirmation shows. */
export interface InstallPreview {
  descriptor: Descriptor;
  maskedUrl: string;
  existing: ExistingMatch | null;
  configureUrl: string | null;
  /** where the live manifest differs from the directory listing it was picked from */
  listingChanges: string[];
  listingRequiresReview: boolean;
}

export interface AddonUpdate {
  transportUrl: string;
  maskedUrl: string;
  name: string;
  currentVersion: string | null;
  latestVersion: string | null;
  changes: string[];
  capabilitiesChanged: boolean;
  requiresReview: boolean;
}

export type DirectorySource = "stremioCommunity" | "stremioOfficial" | "stremioAddonsNet";

export interface DirectoryCategory {
  name: string;
  slug: string;
}

export type DirectorySort = "stars" | "newest" | "name";

export interface DirectoryQuery {
  search?: string | null;
  types: string[];
  resources: string[];
  category?: string | null;
  sort: DirectorySort;
}

export interface MarketEntry {
  key: string;
  /** the listing's copy: display only, untrusted */
  manifest: Manifest;
  transportUrl: string;
  sources: DirectorySource[];
  stars: number | null;
  categories: DirectoryCategory[];
  listedAt: string | null;
  updatedAt: string | null;
  slug: string | null;
  adult: boolean;
  p2p: boolean;
  configurable: boolean;
  configurationRequired: boolean;
  installState: "builtIn" | "installed" | "available";
  siteUrl: string | null;
}

export interface SourceFailure {
  source: DirectorySource;
  message: string;
}

export interface MarketPage {
  entries: MarketEntry[];
  total: number;
  hasMore: boolean;
  sourcesOk: DirectorySource[];
  sourcesFailed: SourceFailure[];
  fetchedAt: number;
}

export interface MarketDetail {
  entry: MarketEntry;
  /** plain text only (may contain markdown or HTML source; never rendered as HTML) */
  documentation: string | null;
}

export interface DeepLinkStatus {
  supported: boolean;
  enabled: boolean;
  registered: boolean;
}
