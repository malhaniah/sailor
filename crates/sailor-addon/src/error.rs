#[derive(Debug, thiserror::Error)]
pub enum AddonError {
    #[error("invalid addon URL: {0}")]
    InvalidTransportUrl(String),
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("addon returned HTTP {status}")]
    BadStatus { status: u16, url: String },
    #[error("addon response too large (over {limit_bytes} bytes)")]
    TooLarge { limit_bytes: usize },
    #[error("addon returned invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}
