pub mod auth;
pub mod client;
pub mod types;

use thiserror::Error;

/// Extract provider name from a model ID (e.g., "openai/gpt-4o" → "openai").
pub fn extract_provider(model_id: &str) -> &str {
    model_id.split('/').next().unwrap_or(model_id)
}

/// Strip `<think>...</think>` blocks from text.
///
/// Reasoning models may embed chain-of-thought in these tags within the
/// content field. Handles missing close tags gracefully.
pub fn strip_think_tags(text: &str) -> String {
    if !text.contains("<think>") {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("<think>") {
        result.push_str(&rest[..start]);
        rest = &rest[start + "<think>".len()..];

        if let Some(end) = rest.find("</think>") {
            rest = &rest[end + "</think>".len()..];
        } else {
            return result;
        }
    }
    result.push_str(rest);
    result
}

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
