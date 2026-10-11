use reqwest::Response;
use serde::Serialize;

use super::MlxError;
use crate::openrouter::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, EmbeddingResponse, ReasoningConfig,
};

/// Client for a local MLX embedding server.
///
/// The server exposes OpenAI-compatible endpoints so we reuse the same
/// request/response types as OpenRouter.
pub struct MlxClient {
    http: reqwest::Client,
    base_url: String,
}

/// What a batch of embeddings is for. The MLX server maps it to the task
/// prompt the loaded model expects (e.g. EmbeddingGemma 2's
/// `task: clustering | query: `); models without task prompts ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbedTask {
    /// Texts that will be grouped by similarity.
    Clustering,
    /// A search query, scored against `Document` embeddings.
    SearchQuery,
    /// A text that search queries are scored against.
    Document,
}

/// Request body for the text embedding endpoint.
#[derive(Debug, Serialize)]
struct TextEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
    task: EmbedTask,
}

/// Request body for the multimodal embedding endpoint.
#[derive(Debug, Serialize)]
pub struct MultimodalEmbeddingRequest {
    pub model: String,
    pub texts: Vec<String>,
    pub images: Vec<String>,
}

impl MlxClient {
    pub fn new(base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .build()
            .expect("failed to build HTTP client");

        // Strip trailing slash for consistent URL joining.
        let base_url = base_url.trim_end_matches('/').to_string();

        Self { http, base_url }
    }

    /// Probe the `/health` endpoint and return the reported capabilities.
    ///
    /// Returns an empty vec on any error (server down, old version, etc.).
    pub async fn capabilities(&self) -> Vec<String> {
        let url = format!("{}/health", self.base_url);
        let Ok(resp) = self.http.get(&url).send().await else {
            return Vec::new();
        };
        let Ok(json) = resp.json::<serde_json::Value>().await else {
            return Vec::new();
        };
        json.get("capabilities")
            .and_then(|v| v.as_array())
            .map(|caps| {
                caps.iter()
                    .filter_map(|c| c.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Generate text embeddings via the local MLX server.
    pub async fn embed(
        &self,
        model: &str,
        texts: &[String],
        task: EmbedTask,
    ) -> Result<EmbeddingResponse, MlxError> {
        let url = format!("{}/v1/embeddings", self.base_url);
        let request = TextEmbeddingRequest {
            model,
            input: texts,
            task,
        };
        let resp = self.http.post(&url).json(&request).send().await?;
        self.handle_response(resp).await
    }

    /// Generate a chat completion via the local MLX server.
    pub async fn chat_completion(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        _reasoning: Option<ReasoningConfig>,
    ) -> Result<ChatCompletionResponse, MlxError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let request = ChatCompletionRequest {
            model: model.to_string(),
            messages,
            max_tokens,
            temperature,
            reasoning: None, // MLX server does not support reasoning config
        };
        let resp = self.http.post(&url).json(&request).send().await?;
        self.handle_response(resp).await
    }

    /// Generate multimodal (text + image) embeddings via the local MLX server.
    pub async fn embed_multimodal(
        &self,
        model: &str,
        texts: &[String],
        image_urls: &[String],
    ) -> Result<EmbeddingResponse, MlxError> {
        let url = format!("{}/v1/embeddings/multimodal", self.base_url);
        let request = MultimodalEmbeddingRequest {
            model: model.to_string(),
            texts: texts.to_vec(),
            images: image_urls.to_vec(),
        };
        let resp = self.http.post(&url).json(&request).send().await?;
        self.handle_response(resp).await
    }

    async fn handle_response<T: serde::de::DeserializeOwned>(
        &self,
        resp: Response,
    ) -> Result<T, MlxError> {
        let status = resp.status();
        if !status.is_success() {
            let detail = resp.text().await.unwrap_or_default();
            return Err(MlxError::ServerError {
                status: status.as_u16(),
                detail,
            });
        }
        Ok(resp.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_request_names_the_task_in_snake_case() {
        let input = vec!["hello".to_string()];
        for (task, name) in [
            (EmbedTask::Clustering, "clustering"),
            (EmbedTask::SearchQuery, "search_query"),
            (EmbedTask::Document, "document"),
        ] {
            let request = TextEmbeddingRequest {
                model: "some/model",
                input: &input,
                task,
            };
            assert_eq!(
                serde_json::to_value(&request).unwrap(),
                serde_json::json!({"model": "some/model", "input": ["hello"], "task": name}),
            );
        }
    }
}
