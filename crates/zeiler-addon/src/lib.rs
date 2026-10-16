//! Stremio addon-protocol client for Zeiler.
//!
//! The protocol is a stateless, GET-only, JSON-over-HTTP(S) protocol. An addon is
//! identified by a *transport URL* ending in `/manifest.json`; every resource is fetched
//! by replacing that suffix with `/{resource}/{type}/{id}[/{extra}].json`.
//!
//! Parsing is deliberately lenient — real-world addons return nulls, numbers where strings
//! are documented, unknown fields, and aliased field names. The semantics intentionally
//! mirror Stremio's reference implementation (stremio-core, MIT) without depending on it.

mod client;
mod error;
mod fanout;
mod manifest;
mod request;
mod resource;
mod subfile;

pub use client::{AddonClient, MAX_RESPONSE_BYTES};
// encoding overrides are named by encoding_rs labels (`Encoding::for_label`)
pub use encoding_rs::Encoding;
pub use error::AddonError;
pub use fanout::{
    AddonStreams, AddonSubtitles, MetaLookup, aggregate_streams, aggregate_subtitles, find_meta,
    first_meta, meta_types_to_try,
};
pub use manifest::{
    Descriptor, DescriptorFlags, ExtraProp, Manifest, ManifestBehaviorHints, ManifestCatalog,
    ManifestExtra, ManifestResource,
};
pub use request::{
    ExtraValue, MANIFEST_PATH, ResourcePath, build_resource_url, encode_component,
    normalize_transport_input, validate_transport_url,
};
pub use resource::{
    AddonCatalogResponse, CatalogResponse, MetaBehaviorHints, MetaItem, MetaLink, MetaPreview,
    MetaResponse, PosterShape, ProxyHeaders, Stream, StreamBehaviorHints, StreamSource,
    StreamsResponse, Subtitle, SubtitlesResponse, Video,
};
pub use subfile::{
    MAX_SUBTITLE_BYTES, SubtitleError, SubtitleFile, SubtitleFormat, decode_subtitle,
    fetch_subtitle,
};
