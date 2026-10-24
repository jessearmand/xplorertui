use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_tick_rate")]
    pub tick_rate_fps: f64,
    #[serde(default = "default_max_results")]
    pub default_max_results: u32,
    #[serde(default)]
    pub default_view: DefaultView,
    #[serde(default = "default_oauth_callback_port")]
    pub oauth_callback_port: u16,
    #[serde(default = "default_openrouter_callback_port")]
    pub openrouter_callback_port: u16,
    /// Base URL for a local MLX embedding server (e.g. "http://localhost:8678").
    /// When set, embedding requests can be routed to this server instead of
    /// OpenRouter.
    #[serde(default)]
    pub mlx_server_url: Option<String>,
    /// Model ID to use with the MLX embedding server.
    /// Falls back to `DEFAULT_MLX_EMBEDDING_MODEL` when not set.
    #[serde(default)]
    pub mlx_embedding_model: Option<String>,
    /// Model ID to use with the MLX server for chat completions.
    /// Falls back to `DEFAULT_MLX_CHAT_MODEL` when not set.
    #[serde(default)]
    pub mlx_chat_model: Option<String>,
    /// `[llama_cpp]` section: `llama-server` processes for chat and
    /// embeddings, used alongside (or instead of) the MLX server.
    #[serde(default)]
    pub llama_cpp: Option<LlamaCppConfig>,
    /// Jev (TypeSafe) decision model settings. When `[jev]` is present,
    /// `:topics` labels clusters with Jev instead of a chat LLM.
    #[serde(default)]
    pub jev: Option<JevConfig>,
    /// Posts `:cluster` collects before it clusters. It loads more pages
    /// when fewer are loaded. One X API read returns 100 at most.
    #[serde(default = "default_cluster_posts")]
    pub cluster_posts: usize,
    /// Number of clusters. Unset: √(posts / 2), from 4 to 10.
    #[serde(default)]
    pub cluster_k: Option<usize>,
}

/// `[llama_cpp]` section of config.toml.
///
/// A `llama-server` started with `--embeddings` answers only embedding
/// requests, so chat and embeddings usually run as two servers. Set both
/// URLs to the same server when it runs in router mode.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlamaCppConfig {
    /// Base URL of the chat server (e.g. "http://localhost:8092").
    #[serde(default)]
    pub chat_url: Option<String>,
    /// Base URL of the embedding server, started with `--embeddings`.
    #[serde(default)]
    pub embedding_url: Option<String>,
    /// Model name sent to the chat server. Unset: the first model the
    /// server lists (its `--alias`, or the file name).
    #[serde(default)]
    pub chat_model: Option<String>,
    /// Model name sent to the embedding server. Unset: as for `chat_model`.
    #[serde(default)]
    pub embedding_model: Option<String>,
    /// Text put in front of each input, per embedding task. `llama-server`
    /// does not read a model's prompt catalog, so the prompts the model was
    /// trained with go here (e.g. EmbeddingGemma's `task: clustering |
    /// query: `). A task without a prompt embeds the text as given.
    #[serde(default)]
    pub task_prompts: TaskPrompts,
}

/// `[llama_cpp.task_prompts]`: one optional prefix per embedding task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskPrompts {
    #[serde(default)]
    pub clustering: Option<String>,
    #[serde(default)]
    pub search_query: Option<String>,
    #[serde(default)]
    pub document: Option<String>,
}

impl TaskPrompts {
    /// The prefix for `task`, if one is set and not empty.
    pub fn for_task(&self, task: crate::mlx::client::EmbedTask) -> Option<&str> {
        use crate::mlx::client::EmbedTask;
        let prompt = match task {
            EmbedTask::Clustering => &self.clustering,
            EmbedTask::SearchQuery => &self.search_query,
            EmbedTask::Document => &self.document,
        };
        prompt.as_deref().filter(|p| !p.is_empty())
    }

    /// `texts` with the prompt for `task` in front of each one.
    pub fn apply(&self, task: crate::mlx::client::EmbedTask, texts: &[String]) -> Vec<String> {
        match self.for_task(task) {
            Some(prefix) => texts.iter().map(|t| format!("{prefix}{t}")).collect(),
            None => texts.to_vec(),
        }
    }

    /// Whether queries and documents get different prompts, so a search
    /// must embed them in separate requests.
    pub fn splits_search(&self) -> bool {
        use crate::mlx::client::EmbedTask;
        self.for_task(EmbedTask::SearchQuery) != self.for_task(EmbedTask::Document)
    }
}

/// `[jev]` section of config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JevConfig {
    /// Decision model ID. Pin a version: thresholds shift between releases.
    #[serde(default = "default_jev_model")]
    pub model: String,
    /// Maximum concurrent Decisions requests.
    #[serde(default = "default_jev_concurrency")]
    pub concurrency: usize,
    /// Topic taxonomy (key → description). Uses the built-in taxonomy when
    /// unset; an `other` option is always added.
    #[serde(default)]
    pub topics: Option<std::collections::BTreeMap<String, String>>,
    /// When a chat model is available, use it for `:topics`: on a mostly
    /// tech timeline it proposes labels and Jev picks a distinct one per
    /// cluster; otherwise it writes the labels itself. When off, Jev picks
    /// from `topics`.
    #[serde(default = "default_true")]
    pub propose_labels: bool,
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            model: default_jev_model(),
            concurrency: default_jev_concurrency(),
            topics: None,
            propose_labels: true,
        }
    }
}

impl JevConfig {
    /// The configured taxonomy (or the default), always including `other`.
    pub fn topics(&self) -> std::collections::BTreeMap<String, String> {
        use crate::openrouter::decisions;
        match &self.topics {
            Some(t) if !t.is_empty() => decisions::with_other(t.clone()),
            _ => decisions::default_topics(),
        }
    }
}

fn default_jev_model() -> String {
    crate::openrouter::decisions::DEFAULT_JEV_MODEL.to_string()
}

fn default_true() -> bool {
    true
}

fn default_jev_concurrency() -> usize {
    crate::openrouter::decisions::DEFAULT_CONCURRENCY
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultView {
    #[default]
    Home,
    Mentions,
    Bookmarks,
    Search,
}

fn default_cluster_posts() -> usize {
    100
}

fn default_tick_rate() -> f64 {
    30.0
}

fn default_max_results() -> u32 {
    20
}

fn default_oauth_callback_port() -> u16 {
    8477
}

fn default_openrouter_callback_port() -> u16 {
    3000
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tick_rate_fps: default_tick_rate(),
            default_max_results: default_max_results(),
            default_view: DefaultView::default(),
            oauth_callback_port: default_oauth_callback_port(),
            openrouter_callback_port: default_openrouter_callback_port(),
            mlx_server_url: None,
            mlx_embedding_model: None,
            mlx_chat_model: None,
            llama_cpp: None,
            jev: None,
            cluster_posts: default_cluster_posts(),
            cluster_k: None,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config/xplorertui/config.toml"))
}

pub fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };

    let Ok(contents) = fs::read_to_string(&path) else {
        return AppConfig::default();
    };

    toml::from_str(&contents).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_settings_default_to_100_posts_and_the_k_rule() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(config.cluster_posts, 100);
        assert_eq!(config.cluster_k, None);
        let config: AppConfig = toml::from_str("cluster_posts = 50\ncluster_k = 6\n").unwrap();
        assert_eq!((config.cluster_posts, config.cluster_k), (50, Some(6)));
    }

    #[test]
    fn jev_proposes_labels_by_default() {
        let config: AppConfig = toml::from_str("[jev]\nmodel = \"typesafe/jev-1.13\"\n").unwrap();
        assert!(config.jev.unwrap().propose_labels);
        let config: AppConfig = toml::from_str("[jev]\npropose_labels = false\n").unwrap();
        assert!(!config.jev.unwrap().propose_labels);
    }

    #[test]
    fn llama_cpp_section_reads_urls_models_and_task_prompts() {
        use crate::mlx::client::EmbedTask;
        let config: AppConfig = toml::from_str(
            "[llama_cpp]\n\
             chat_url = \"http://localhost:8092\"\n\
             chat_model = \"gemma-4-12b\"\n\
             [llama_cpp.task_prompts]\n\
             clustering = \"task: clustering | query: \"\n\
             document = \"\"\n",
        )
        .unwrap();
        let llama = config.llama_cpp.unwrap();
        assert_eq!(llama.chat_url.as_deref(), Some("http://localhost:8092"));
        assert_eq!(llama.embedding_url, None);
        assert_eq!(llama.chat_model.as_deref(), Some("gemma-4-12b"));
        let prompts = &llama.task_prompts;
        assert_eq!(
            prompts.apply(EmbedTask::Clustering, &["Rust".into()]),
            vec!["task: clustering | query: Rust".to_string()]
        );
        // An empty prompt counts as none.
        assert_eq!(prompts.for_task(EmbedTask::Document), None);
        assert!(!prompts.splits_search());
    }

    #[test]
    fn llama_cpp_is_off_without_its_section() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert!(config.llama_cpp.is_none());
    }
}
