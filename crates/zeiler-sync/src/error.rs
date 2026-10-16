/// What went wrong talking to Stremio. Messages never carry a URL or the authKey.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// No connection or no answer in time.
    #[error("Stremio could not be reached ({0})")]
    Network(String),
    /// Stremio answered with an HTTP error or something unreadable.
    #[error("Stremio answered unexpectedly: {0}")]
    Unexpected(String),
    /// The API refused the request (`{"error": {code, message}}`).
    #[error("{message}")]
    Api { code: i64, message: String },
    /// Wrong email or password.
    #[error("{0}")]
    Auth(String),
    /// The authKey is no longer accepted: sign in again.
    #[error("Your Stremio session has ended. Sign in again.")]
    SessionExpired,
    /// The link code has not been confirmed in the browser yet.
    #[error("Waiting for you to confirm in the browser.")]
    LinkPending,
}

impl SyncError {
    pub(crate) fn from_http(error: reqwest::Error) -> Self {
        let error = error.without_url();
        if error.is_connect() || error.is_timeout() {
            SyncError::Network(error.to_string())
        } else {
            SyncError::Unexpected(error.to_string())
        }
    }
}
