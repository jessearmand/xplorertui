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
