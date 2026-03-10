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
