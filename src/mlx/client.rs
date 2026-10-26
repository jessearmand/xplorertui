use std::time::Duration;

use reqwest::Response;
use serde::Serialize;

use super::MlxError;
use crate::openrouter::types::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, EmbeddingResponse,
};

/// Client for a local MLX embedding server.
///
/// The server exposes OpenAI-compatible endpoints so we reuse the same
/// request/response types as OpenRouter.
pub struct MlxClient {
    http: reqwest::Client,
    base_url: String,
    /// Limit for the `/health` probe, so a server that accepts the
    /// connection but never answers counts as unavailable.
    probe_timeout: Duration,
}

/// Default limit for the `/health` probe. The server answers it without
/// touching MLX, so a healthy one replies in milliseconds.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

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

/// One document for the document embedding endpoint: a text and its photo.
#[derive(Debug, Serialize)]
struct DocumentInput<'a> {
    text: &'a str,
    image: &'a str,
}

/// Request body for the document embedding endpoint.
#[derive(Debug, Serialize)]
struct DocumentEmbeddingRequest<'a> {
    model: &'a str,
    input: Vec<DocumentInput<'a>>,
    task: EmbedTask,
}

/// The server decodes at most this many images per request.
const MAX_IMAGES_PER_REQUEST: usize = 8;

impl MlxClient {
    pub fn new(base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .build()
            .expect("failed to build HTTP client");

        // Strip trailing slash for consistent URL joining.
        let base_url = base_url.trim_end_matches('/').to_string();

        Self {
            http,
            base_url,
            probe_timeout: PROBE_TIMEOUT,
        }
    }

    /// The same client with another probe limit.
    pub fn with_probe_timeout(mut self, timeout: Duration) -> Self {
        self.probe_timeout = timeout;
        self
    }

    /// Probe the `/health` endpoint and return the reported capabilities.
    ///
    /// Returns an empty vec on any error (server down, old version, no
    /// answer within the probe timeout, etc.).
    pub async fn capabilities(&self) -> Vec<String> {
        let url = format!("{}/health", self.base_url);
        let request = self.http.get(&url).timeout(self.probe_timeout);
        let Ok(resp) = request.send().await else {
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
    ///
    /// There is no reasoning setting to send: the server always asks the
    /// model's chat template to turn thinking off. `max_tokens` caps the
    /// answer; a model that thinks anyway gets extra room from the server.
    pub async fn chat_completion(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatCompletionResponse, MlxError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let request = ChatCompletionRequest {
            model: model.to_string(),
            messages,
            max_tokens,
            temperature,
            reasoning: None,
        };
        let resp = self.http.post(&url).json(&request).send().await?;
        self.handle_response(resp).await
    }

    /// One embedding per `(text, image)` document, in the same space as
    /// `embed` vectors of the same model and task. `image` is a `data:` URL.
    /// Fails with a 400 when the model cannot read images.
    pub async fn embed_documents(
        &self,
        model: &str,
        documents: &[(String, std::sync::Arc<str>)],
        task: EmbedTask,
    ) -> Result<EmbeddingResponse, MlxError> {
        let url = format!("{}/v1/embeddings/documents", self.base_url);
        let mut all = EmbeddingResponse {
            data: Vec::with_capacity(documents.len()),
            model: model.to_string(),
            usage: None,
        };
        for (batch_no, batch) in documents.chunks(MAX_IMAGES_PER_REQUEST).enumerate() {
            let request = DocumentEmbeddingRequest {
                model,
                input: batch
                    .iter()
                    .map(|(text, image)| DocumentInput { text, image })
                    .collect(),
                task,
            };
            let resp = self.http.post(&url).json(&request).send().await?;
            let part: EmbeddingResponse = self.handle_response(resp).await?;
            let offset = batch_no * MAX_IMAGES_PER_REQUEST;
            all.data.extend(part.data.into_iter().map(|mut d| {
                d.index += offset;
                d
            }));
        }
        Ok(all)
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

    #[tokio::test]
    async fn server_that_never_answers_has_no_capabilities() {
        // Accept connections and hold them open without replying.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        let client = MlxClient::new(url).with_probe_timeout(Duration::from_millis(100));
        let caps = tokio::time::timeout(Duration::from_secs(5), client.capabilities())
            .await
            .expect("the probe gives up at its timeout");
        assert!(caps.is_empty());
    }

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
