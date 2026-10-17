//! Opt-in Stremio account sync for Zeiler: a client for the account API (sign-in,
//! the addon collection, the library datastore) and the pure rules that decide what
//! to pull, what to push and how an addon change lands in the account's collection.
//!
//! Kept apart from `zeiler-addon` (the stateless, credential-free addon protocol) and
//! from `zeiler-account` (pure library data): this is the only crate that holds a
//! Stremio `authKey` or talks to api.strem.io and link.stremio.com. Request and
//! response shapes follow stremio-core (MIT, Copyright (c) 2019 SmartCode OOD); the
//! endpoints that only feed Stremio's own telemetry or premium features (events,
//! seekLog, getSkipGaps, dataExport, register, …) are deliberately not implemented.

mod api;
mod error;
pub mod sync;
mod types;

pub use api::{API_BASE, LINK_BASE, StremioApi};
pub use error::SyncError;
pub use types::{AuthKey, CollectionResponse, LinkCode, RawDescriptor, UserSummary};
