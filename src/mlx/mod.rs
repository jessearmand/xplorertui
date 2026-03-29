pub mod client;
pub mod media;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MlxError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("MLX server error (status {status}): {detail}")]
    ServerError { status: u16, detail: String },
}
