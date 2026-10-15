/// Errors crossing the IPC boundary. Serialized as `{ kind, message }` so the UI can
/// branch on kind and show message verbatim.
#[derive(Debug, thiserror::Error, serde::Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum IpcError {
    #[error("{0}")]
    InvalidInput(String),
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
}

impl From<sailor_addon::AddonError> for IpcError {
    fn from(error: sailor_addon::AddonError) -> Self {
        match error {
            sailor_addon::AddonError::InvalidTransportUrl(message) => {
                IpcError::InvalidInput(message)
            }
            other => IpcError::Addon(other.to_string()),
        }
    }
}

impl From<sailor_player::PlayerError> for IpcError {
    fn from(error: sailor_player::PlayerError) -> Self {
        IpcError::Player(error.to_string())
    }
}
