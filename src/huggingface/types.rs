use serde::Deserialize;

/// A model from the HuggingFace Hub API.
#[derive(Debug, Clone, Deserialize)]
pub struct HfModel {
    pub id: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub likes: u32,
    #[serde(default)]
    pub library_name: Option<String>,
    #[serde(default)]
    pub pipeline_tag: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl HfModel {
    /// Extract the quantization format from tags (e.g. "4-bit", "8-bit", "mxfp8").
    pub fn quant_tag(&self) -> Option<&str> {
        self.tags.iter().find_map(|t| {
            let t = t.as_str();
            if t.ends_with("-bit") || t.starts_with("mxfp") || t.starts_with("nvfp") {
                Some(t)
            } else {
                None
            }
        })
    }

    /// Extract the provider/org from the model ID (e.g. "mlx-community").
    pub fn org(&self) -> &str {
        self.id.split('/').next().unwrap_or(&self.id)
    }

    /// Short display name without the org prefix.
    pub fn short_name(&self) -> &str {
        self.id.split('/').nth(1).unwrap_or(&self.id)
    }
}
