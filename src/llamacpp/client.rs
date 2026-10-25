use std::time::Duration;

use reqwest::{RequestBuilder, Response};
use serde::{Deserialize, Serialize};

use super::LlamaCppError;
use crate::openrouter::types::{ChatCompletionResponse, ChatMessage, EmbeddingResponse};

/// Client for one `llama-server` process from llama.cpp.
///
/// `llama-server` exposes OpenAI-compatible endpoints, so the response types
/// are the same as OpenRouter's. It loads one model per process (unless it
/// runs in router mode), and a server started with `--embeddings` answers
/// only embedding requests. Chat and embeddings therefore usually come from
/// two servers, each with its own client.
pub struct LlamaCppClient {
    http: reqwest::Client,
    base_url: String,
    /// Limit for each probe request, so a server that accepts the
    /// connection but never answers counts as unavailable.
    probe_timeout: Duration,
}

/// Request body for `/v1/embeddings`. `llama-server` has no task prompts;
/// the caller puts any prompt in front of each text.
#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

/// Request body for `/v1/chat/completions`.
#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    /// Variables for the model's Jinja chat template.
    chat_template_kwargs: ChatTemplateKwargs,
}

#[derive(Debug, Serialize)]
struct ChatTemplateKwargs {
    /// Read by the Qwen 3 and Gemma 4 templates, among others. A template
    /// that does not read it thinks anyway; `llama-server` then returns the
    /// thinking in `reasoning_content`, apart from `content`.
    enable_thinking: bool,
}

/// `/v1/models` response, reduced to the model ids.
#[derive(Debug, Deserialize)]
struct ModelList {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

/// Sent as `model` when the server lists no model and none is configured.
/// A single-model `llama-server` accepts any name.
const FALLBACK_MODEL_NAME: &str = "default";

/// Default limit for each probe request. A healthy local server answers
/// `/health` in milliseconds; the one-text embedding probe is the slowest.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

impl LlamaCppClient {
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

    /// The same client with another per-request probe limit.
    pub fn with_probe_timeout(mut self, timeout: Duration) -> Self {
        self.probe_timeout = timeout;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `true` when `/health` answers 200. It answers 503 while the model is
    /// still loading.
    async fn healthy(&self) -> bool {
        let url = format!("{}/health", self.base_url);
        self.http
            .get(&url)
            .timeout(self.probe_timeout)
            .send()
            .await
            .is_ok_and(|resp| resp.status().is_success())
    }

    /// The id of the first model the server lists, if any.
    async fn first_model(&self) -> Option<String> {
        let url = format!("{}/v1/models", self.base_url);
        let resp = self
            .http
            .get(&url)
            .timeout(self.probe_timeout)
            .send()
            .await
            .ok()?;
        let list: ModelList = resp.json().await.ok()?;
        list.data.into_iter().next().map(|m| m.id)
    }

    /// The model name to send: the configured one, else the one the server
    /// lists (its `--alias`, or the file name).
    async fn model_name(&self, configured: Option<&str>) -> String {
        match configured {
            Some(name) => name.to_string(),
            None => self
                .first_model()
                .await
                .unwrap_or_else(|| FALLBACK_MODEL_NAME.to_string()),
        }
    }

    /// Check that the server is up for chat and return the model name to
    /// use. `None` when the server is not reachable or still loading.
    ///
    /// No chat request is made: a generation costs real time on a large
    /// model. A server started with `--embeddings` passes this check and
    /// fails on the first chat request instead.
    pub async fn probe_chat(&self, configured_model: Option<&str>) -> Option<String> {
        if !self.healthy().await {
            return None;
        }
        Some(self.model_name(configured_model).await)
    }

    /// Check that the server embeds text and return the model name to use.
    /// `None` when it is not reachable, or was not started with
    /// `--embeddings` (it then answers 501).
    pub async fn probe_embeddings(&self, configured_model: Option<&str>) -> Option<String> {
        if !self.healthy().await {
            return None;
        }
        let model = self.model_name(configured_model).await;
        let texts = ["ping".to_string()];
        let request = self
            .embedding_request(&model, &texts)
            .timeout(self.probe_timeout);
        let _: EmbeddingResponse = self
            .handle_response(request.send().await.ok()?)
            .await
            .ok()?;
        Some(model)
    }

    /// Embed `texts` as given, one vector each.
    pub async fn embed(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<EmbeddingResponse, LlamaCppError> {
        let resp = self.embedding_request(model, texts).send().await?;
        self.handle_response(resp).await
    }

    fn embedding_request(&self, model: &str, texts: &[String]) -> RequestBuilder {
        let url = format!("{}/v1/embeddings", self.base_url);
        let request = EmbeddingRequest {
            model,
            input: texts,
        };
        self.http.post(url).json(&request)
    }

    /// Generate a chat completion with thinking turned off in the chat
    /// template. `max_tokens` counts thinking tokens too, for a model whose
    /// template ignores the setting.
    pub async fn chat_completion(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<ChatCompletionResponse, LlamaCppError> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let request = ChatRequest {
            model,
            messages,
            max_tokens,
            temperature,
            chat_template_kwargs: ChatTemplateKwargs {
                enable_thinking: false,
            },
        };
        let resp = self.http.post(&url).json(&request).send().await?;
        self.handle_response(resp).await
    }

    async fn handle_response<T: serde::de::DeserializeOwned>(
        &self,
        resp: Response,
    ) -> Result<T, LlamaCppError> {
        let status = resp.status();
        if !status.is_success() {
            let detail = resp.text().await.unwrap_or_default();
            return Err(LlamaCppError::ServerError {
                status: status.as_u16(),
                detail,
            });
        }
        Ok(resp.json().await?)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A recorded request: method, path and JSON body (`Null` when empty).
    pub(crate) type Seen = Arc<Mutex<Vec<(String, String, serde_json::Value)>>>;

    /// Maps a request's method, path and JSON body to a status and reply.
    pub(crate) type Route = fn(&str, &str, &serde_json::Value) -> (u16, serde_json::Value);

    /// Local stand-in for `llama-server`, answering through `route`. Every
    /// request is recorded.
    pub(crate) async fn fake_llama_server(route: Route) -> (String, Seen) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let mut raw = Vec::new();
                let (head, body) = loop {
                    let mut chunk = [0u8; 4096];
                    let n = stream.read(&mut chunk).await.unwrap();
                    assert!(n > 0, "request ended early");
                    raw.extend_from_slice(&chunk[..n]);
                    let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&raw[..split]).to_string();
                    let length: usize = head
                        .to_ascii_lowercase()
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map(|v| v.trim().parse().unwrap())
                        .unwrap_or(0);
                    if raw.len() >= split + 4 + length {
                        break (head, raw[split + 4..split + 4 + length].to_vec());
                    }
                };
                let mut request_line = head.lines().next().unwrap().split(' ');
                let method = request_line.next().unwrap().to_string();
                let path = request_line.next().unwrap().to_string();
                let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
                let (status, reply) = route(&method, &path, &json);
                log.lock().unwrap().push((method, path, json));
                let reply = reply.to_string();
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (url, seen)
    }

    pub(crate) fn embedding_reply(count: usize) -> serde_json::Value {
        let data: Vec<_> = (0..count)
            .map(|i| serde_json::json!({"embedding": [i as f64, 1.0], "index": i}))
            .collect();
        serde_json::json!({"data": data, "model": "m"})
    }

    /// An embedding server named `bge-small`: one vector per input.
    pub(crate) fn embedding_server(
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> (u16, serde_json::Value) {
        match (method, path) {
            ("GET", "/health") => (200, serde_json::json!({"status": "ok"})),
            ("GET", "/v1/models") => (200, serde_json::json!({"data": [{"id": "bge-small"}]})),
            ("POST", "/v1/embeddings") => {
                let count = body["input"].as_array().map_or(0, Vec::len);
                (200, embedding_reply(count))
            }
            _ => (404, serde_json::json!({})),
        }
    }

    /// A chat server: up, but started without `--embeddings`.
    pub(crate) fn chat_server(
        method: &str,
        path: &str,
        _body: &serde_json::Value,
    ) -> (u16, serde_json::Value) {
        match (method, path) {
            ("GET", "/health") => (200, serde_json::json!({"status": "ok"})),
            ("GET", "/v1/models") => (200, serde_json::json!({"data": [{"id": "gemma-4-12b"}]})),
            ("POST", "/v1/embeddings") => (
                501,
                serde_json::json!({"error": {"code": 501, "type": "not_supported_error"}}),
            ),
            ("POST", "/v1/chat/completions") => (
                200,
                serde_json::json!({"choices": [{
                    "message": {"role": "assistant", "content": "reqwest"},
                    "finish_reason": "stop",
                }]}),
            ),
            _ => (404, serde_json::json!({})),
        }
    }

    #[tokio::test]
    async fn embedding_probe_takes_the_listed_model_name() {
        let (url, seen) = fake_llama_server(embedding_server).await;
        let client = LlamaCppClient::new(format!("{url}/"));
        assert_eq!(
            client.probe_embeddings(None).await.as_deref(),
            Some("bge-small")
        );
        let seen = seen.lock().unwrap();
        let (_, path, body) = seen.last().unwrap();
        assert_eq!(path, "/v1/embeddings");
        assert_eq!(
            body,
            &serde_json::json!({"model": "bge-small", "input": ["ping"]})
        );
    }

    #[tokio::test]
    async fn configured_model_name_wins_over_the_listed_one() {
        let (url, _) = fake_llama_server(embedding_server).await;
        let client = LlamaCppClient::new(url);
        assert_eq!(
            client
                .probe_embeddings(Some("my-embedder"))
                .await
                .as_deref(),
            Some("my-embedder")
        );
    }

    #[tokio::test]
    async fn chat_server_fails_the_embedding_probe_and_passes_the_chat_probe() {
        let (url, _) = fake_llama_server(chat_server).await;
        let client = LlamaCppClient::new(url);
        assert_eq!(client.probe_embeddings(None).await, None);
        assert_eq!(
            client.probe_chat(None).await.as_deref(),
            Some("gemma-4-12b")
        );
    }

    #[tokio::test]
    async fn unreachable_server_fails_both_probes() {
        // Bind and drop to get a port that nothing listens on.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let client = LlamaCppClient::new(url);
        assert_eq!(client.probe_chat(None).await, None);
        assert_eq!(client.probe_embeddings(None).await, None);
    }

    #[tokio::test]
    async fn server_that_never_answers_fails_the_probes_in_time() {
        // Accept connections and hold them open without replying.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
            }
        });
        let client = LlamaCppClient::new(url).with_probe_timeout(Duration::from_millis(100));
        let probes = async { tokio::join!(client.probe_chat(None), client.probe_embeddings(None)) };
        let found = tokio::time::timeout(Duration::from_secs(5), probes)
            .await
            .expect("probes give up at their timeout");
        assert_eq!(found, (None, None));
    }

    #[tokio::test]
    async fn chat_turns_thinking_off_in_the_template() {
        let (url, seen) = fake_llama_server(chat_server).await;
        let client = LlamaCppClient::new(url);
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "One HTTP crate?".into(),
        }];
        let resp = client
            .chat_completion("gemma-4-12b", messages, Some(16), Some(0.3))
            .await
            .unwrap();
        assert_eq!(resp.choices[0].message.content.as_deref(), Some("reqwest"));
        let seen = seen.lock().unwrap();
        let (_, _, body) = seen.last().unwrap();
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["max_tokens"], 16);
        assert!(body.get("reasoning").is_none());
    }
}
