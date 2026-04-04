use super::types::HfModel;

const HUB_API_BASE: &str = "https://huggingface.co/api";

/// Client for the HuggingFace Hub REST API (model search/browse).
pub struct HfHubClient {
    http: reqwest::Client,
}

impl Default for HfHubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HfHubClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("xplorertui/0.1")
            .build()
            .expect("failed to build HTTP client");
        Self { http }
    }

    /// Search for MLX models on the Hub.
    ///
    /// The `filter=mlx` parameter matches models tagged with "mlx" regardless
    /// of their `library_name` field.  When `query` is provided, results are
    /// sorted by last-modified (so the latest variants appear first); otherwise
    /// sorted by downloads (popular models first).
    pub async fn search_mlx_models(
        &self,
        query: Option<&str>,
        limit: u32,
    ) -> Result<Vec<HfModel>, reqwest::Error> {
        let sort = if query.is_some() {
            "lastModified"
        } else {
            "downloads"
        };
        let mut url =
            format!("{HUB_API_BASE}/models?filter=mlx&sort={sort}&direction=-1&limit={limit}");
        if let Some(q) = query {
            url.push_str(&format!("&search={}", urlencoding::encode(q)));
        }
        let models: Vec<HfModel> = self.http.get(&url).send().await?.json().await?;
        Ok(models)
    }
}
