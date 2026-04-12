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
    fn has_tag(&self, needle: &str) -> bool {
        self.tags.iter().any(|tag| tag.eq_ignore_ascii_case(needle))
    }

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

    /// Whether this model can be used for text generation / chat.
    pub fn is_chat_capable(&self) -> bool {
        matches!(
            self.pipeline_tag.as_deref(),
            Some("text-generation" | "any-to-any" | "image-text-to-text" | "video-text-to-text")
        )
    }

    /// Gemma 4 base checkpoints are poor fits for the app's chat-style
    /// cluster labeling flow; prefer instruction-tuned (`-it`) variants.
    pub fn is_gemma4_base_model(&self) -> bool {
        let short = self.short_name().to_ascii_lowercase();
        short.starts_with("gemma-4-") && !short.contains("-it")
    }

    /// Gemma 4 OptiQ checkpoints are still treated as unsupported in the app
    /// because upstream quantized Gemma 4 behavior remains unstable.
    pub fn is_gemma4_optiq_model(&self) -> bool {
        let short = self.short_name().to_ascii_lowercase();
        short.starts_with("gemma-4-") && self.has_tag("optiq")
    }

    /// Whether this model should be blocked in the HF picker for cluster topic
    /// labeling because it is likely to ignore the chat-style labeling prompt.
    pub fn is_discouraged_for_cluster_labels(&self) -> bool {
        self.is_gemma4_base_model()
    }

    /// Whether this model is currently unsupported for cluster labeling in the
    /// local MLX server integration.
    pub fn is_unsupported_for_cluster_labels(&self) -> bool {
        self.is_gemma4_optiq_model()
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
