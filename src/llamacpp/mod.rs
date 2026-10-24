pub mod client;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlamaCppError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("llama.cpp server error (status {status}): {detail}")]
    ServerError { status: u16, detail: String },
}
