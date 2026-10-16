/// Errors crossing the IPC boundary. Serialized as `{ kind, message }` so the UI can
/// branch on kind and show message verbatim (write full sentences). The `kind` strings
/// are mirrored by `IpcError` in src/lib/types.ts; add kinds there in the same change.
#[derive(Debug, thiserror::Error, serde::Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum IpcError {
    /// The caller sent something malformed or out of range.
    #[error("{0}")]
    InvalidInput(String),
    /// An addon answered badly (HTTP error, unparseable JSON, …).
    #[error("{0}")]
    Addon(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Engine(String),
    #[error("{0}")]
    Player(String),
    #[error("{0}")]
    Storage(String),
    // ---- reserved for later milestones (account, marketplace, profiles, player) ----
    /// A remote service could not be reached (offline, DNS, timeout) — worth a retry,
    /// unlike `Addon`, which means the service answered with something unusable.
    #[allow(dead_code, reason = "reserved; first used by marketplace/account")]
    #[error("{0}")]
    Network(String),
    /// Credentials were rejected (wrong email/password, HTTP 401/403).
    #[allow(dead_code, reason = "reserved; first used by account sign-in")]
    #[error("{0}")]
    Auth(String),
    /// A previously valid session is no longer accepted; the user must sign in again.
    #[allow(dead_code, reason = "reserved; first used by account sync")]
    #[error("{0}")]
    SessionExpired(String),
    /// Refused because something is locked: a PIN-protected profile (wrong PIN), or
    /// addon sync after a failed pull.
    #[allow(dead_code, reason = "reserved; first used by profiles/account sync")]
    #[error("{0}")]
    Locked(String),
    /// The request clashes with existing state (e.g. the same addon is already
    /// installed under another URL).
    #[allow(
        dead_code,
        reason = "reserved; first used by the marketplace install path"
    )]
    #[error("{0}")]
    Conflict(String),
    /// Not available on this platform or build (e.g. embedded playback without the
    /// `embedded` feature, a deep-link scheme on macOS).
    #[error("{0}")]
    Unsupported(String),
}

impl From<zeiler_addon::AddonError> for IpcError {
    fn from(error: zeiler_addon::AddonError) -> Self {
        match error {
            zeiler_addon::AddonError::InvalidTransportUrl(message) => {
                IpcError::InvalidInput(message)
            }
            other => IpcError::Addon(other.to_string()),
        }
    }
}

impl From<zeiler_player::PlayerError> for IpcError {
    fn from(error: zeiler_player::PlayerError) -> Self {
        IpcError::Player(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The UI branches on these strings (src/lib/types.ts `IpcError["kind"]`).
    #[test]
    fn kinds_serialize_as_the_ui_expects() {
        let cases = [
            (IpcError::InvalidInput("m".into()), "invalidInput"),
            (IpcError::Addon("m".into()), "addon"),
            (IpcError::NotFound("m".into()), "notFound"),
            (IpcError::Engine("m".into()), "engine"),
            (IpcError::Player("m".into()), "player"),
            (IpcError::Storage("m".into()), "storage"),
            (IpcError::Network("m".into()), "network"),
            (IpcError::Auth("m".into()), "auth"),
            (IpcError::SessionExpired("m".into()), "sessionExpired"),
            (IpcError::Locked("m".into()), "locked"),
            (IpcError::Conflict("m".into()), "conflict"),
            (IpcError::Unsupported("m".into()), "unsupported"),
        ];
        for (error, kind) in cases {
            assert_eq!(
                serde_json::to_value(&error).unwrap(),
                serde_json::json!({ "kind": kind, "message": "m" })
            );
        }
    }
}
