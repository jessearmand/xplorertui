use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Auth types
// ---------------------------------------------------------------------------

/// Request body for `POST /api/v1/auth/keys` (PKCE code exchange).
#[derive(Debug, Serialize)]
pub struct AuthKeysRequest {
    pub code: String,
    pub code_verifier: String,
    pub code_challenge_method: String,
}

/// Response from `POST /api/v1/auth/keys`.
#[derive(Debug, Deserialize)]
pub struct AuthKeysResponse {
    pub key: String,
    #[serde(default)]
    pub user_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Model types
// ---------------------------------------------------------------------------

/// A model returned by `GET /api/v1/models`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub pricing: Option<ModelPricing>,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub architecture: Option<ModelArchitecture>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub completion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelArchitecture {
    #[serde(default)]
    pub modality: Option<String>,
    /// For example `["text", "image"]`.
    #[serde(default)]
    pub input_modalities: Option<Vec<String>>,
    #[serde(default)]
    pub tokenizer: Option<String>,
}

/// Response from `GET /api/v1/models`.
#[derive(Debug, Deserialize)]
pub struct ModelsResponse {
    pub data: Vec<Model>,
}

// ---------------------------------------------------------------------------
// Embedding types
// ---------------------------------------------------------------------------

/// Request body for `POST /api/v1/embeddings`.
#[derive(Debug, Serialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: Vec<String>,
}

/// Response from `POST /api/v1/embeddings`.
#[derive(Debug, Deserialize)]
pub struct EmbeddingResponse {
    pub data: Vec<EmbeddingData>,
    pub model: String,
    #[serde(default)]
    pub usage: Option<EmbeddingUsage>,
}

/// A single embedding vector in the response.
#[derive(Debug, Deserialize)]
pub struct EmbeddingData {
    pub embedding: Vec<f64>,
    pub index: usize,
}

/// Token usage information for an embeddings request.
#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u64,
    pub total_tokens: u64,
}

// ---------------------------------------------------------------------------
// Chat completion types (OpenAI-compatible)
// ---------------------------------------------------------------------------

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Controls reasoning token behavior in chat completions.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReasoningConfig {
    /// `false` turns reasoning off for models that allow it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// When true, the model still reasons internally but reasoning
    /// tokens are excluded from the response. They still count against
    /// `max_tokens`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude: Option<bool>,
    /// Reasoning effort hint ("low", "medium", "high").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl ReasoningConfig {
    /// No reasoning: the whole token budget goes to the answer.
    pub fn disabled() -> Self {
        Self {
            enabled: Some(false),
            ..Self::default()
        }
    }

    /// Low-effort reasoning, hidden from the response. For models that
    /// refuse `disabled()`.
    pub fn hidden_low() -> Self {
        Self {
            exclude: Some(true),
            effort: Some("low".into()),
            ..Self::default()
        }
    }
}

/// Request body for `POST /api/v1/chat/completions`.
#[derive(Debug, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Controls reasoning: `ReasoningConfig::disabled()` turns it off,
    /// `exclude` hides it from the response while the model still reasons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
}

/// Response from `POST /api/v1/chat/completions`.
#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub choices: Vec<ChatChoice>,
    #[serde(default)]
    pub usage: Option<ChatUsage>,
}

/// A single choice in a chat completion response.
#[derive(Debug, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessageResponse,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// The message content in a chat completion choice.
#[derive(Debug, Deserialize)]
pub struct ChatMessageResponse {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    /// Reasoning models may return their chain-of-thought here.
    /// Used by DeepSeek R1, GLM-5, etc. (`reasoning-content` mechanism).
    #[serde(default)]
    pub reasoning_content: Option<String>,
    /// Alias for `reasoning_content` used by some providers
    /// (`reasoning` mechanism).
    #[serde(default)]
    pub reasoning: Option<String>,
}

/// Token usage for a chat completion request.
#[derive(Debug, Deserialize)]
pub struct ChatUsage {
    #[serde(default)]
    pub prompt_tokens: Option<u64>,
    #[serde(default)]
    pub completion_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_config_serializes_only_set_fields() {
        let off = serde_json::to_value(ReasoningConfig::disabled()).unwrap();
        assert_eq!(off, serde_json::json!({ "enabled": false }));
        let hidden = serde_json::to_value(ReasoningConfig::hidden_low()).unwrap();
        assert_eq!(
            hidden,
            serde_json::json!({ "exclude": true, "effort": "low" })
        );
    }
}
