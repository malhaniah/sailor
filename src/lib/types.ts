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
  builtinCinemeta: boolean;
  builtinKitsu: boolean;
  builtinOpensubtitles: boolean;
  autoUpdate: boolean;
  /** Which player `player_start` uses for "auto" (Rust `zeiler_player::PlayerBackend`). */
  playerBackend: PlayerBackend;
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
