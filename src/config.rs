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
    /// Jev (TypeSafe) decision model settings. When `[jev]` is present,
    /// `:topics` labels clusters with Jev instead of a chat LLM.
    #[serde(default)]
    pub jev: Option<JevConfig>,
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
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            model: default_jev_model(),
            concurrency: default_jev_concurrency(),
            topics: None,
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
            jev: None,
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
