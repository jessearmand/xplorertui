pub mod auth;
pub mod client;
pub mod types;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpenRouterError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error (status {status}): {detail}")]
    ApiError { status: u16, detail: String },
    #[error("auth error: {0}")]
    Auth(String),
    #[error("no API key available")]
    NoApiKey,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("embedding dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: usize, got: usize },
}
