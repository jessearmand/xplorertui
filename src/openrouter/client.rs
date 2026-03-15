use reqwest::Response;
use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::OpenRouterError;

const BASE_URL: &str = "https://openrouter.ai/api/v1";
const APP_URL: &str = "https://github.com/jessearmand/xplorertui";
const APP_TITLE: &str = "xplorertui";

pub struct OpenRouterClient {
    http: reqwest::Client,
}

impl OpenRouterClient {
    pub fn new(api_key: String) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {api_key}")).expect("valid header"),
        );
        // App attribution headers for OpenRouter rankings/discovery.
        // Keep both legacy and current names for compatibility.
        headers.insert(
            HeaderName::from_static("http-referer"),
            HeaderValue::from_static(APP_URL),
        );
        headers.insert(header::REFERER, HeaderValue::from_static(APP_URL));
        headers.insert(
            HeaderName::from_static("x-openrouter-title"),
            HeaderValue::from_static(APP_TITLE),
        );
        headers.insert("X-Title", HeaderValue::from_static(APP_TITLE));

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("failed to build HTTP client");

        Self { http }
    }

    /// Issue an authenticated GET request to an OpenRouter API path.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, OpenRouterError> {
        let url = format!("{BASE_URL}{path}");
        let resp = self.http.get(&url).send().await?;
        self.handle_response(resp).await
    }

    /// Issue an authenticated POST request to an OpenRouter API path.
    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, OpenRouterError> {
        let url = format!("{BASE_URL}{path}");
        let resp = self.http.post(&url).json(body).send().await?;
        self.handle_response(resp).await
    }

    /// Generate a chat completion.
    pub async fn chat_completion(
        &self,
        model: &str,
        messages: Vec<super::types::ChatMessage>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        reasoning: Option<super::types::ReasoningConfig>,
    ) -> Result<super::types::ChatCompletionResponse, OpenRouterError> {
        let request = super::types::ChatCompletionRequest {
            model: model.to_string(),
            messages,
            max_tokens,
            temperature,
            reasoning,
        };
        self.post("/chat/completions", &request).await
    }

    /// Generate embeddings for a batch of texts.
    pub async fn embed(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<super::types::EmbeddingResponse, OpenRouterError> {
        let request = super::types::EmbeddingRequest {
            model: model.to_string(),
            input: texts.to_vec(),
        };
        self.post("/embeddings", &request).await
    }

    /// Check status and deserialize the response body.
    async fn handle_response<T: DeserializeOwned>(
        &self,
        resp: Response,
    ) -> Result<T, OpenRouterError> {
        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(OpenRouterError::ApiError {
                status: status.as_u16(),
                detail: body,
            });
        }

        let body = resp.text().await?;
        tracing::debug!("openrouter response: {body}");
        Ok(serde_json::from_str::<T>(&body)?)
    }
}
