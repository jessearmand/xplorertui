mod auth;
mod commands;
mod dispatch;
mod event_handlers;
mod key_handlers;
mod navigation;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use ratatui::DefaultTerminal;

use crate::api::XApiClient;
use crate::api::types::{Includes, Tweet, User};
use crate::auth::credentials::CredentialSet;
use crate::config::AppConfig;
use crate::embeddings::cluster::ClusterResult;
use crate::event::{AppEvent, Event, EventHandler, ViewKind};
use crate::mlx::client::MlxClient;
use crate::openrouter::client::OpenRouterClient;
use crate::openrouter::types::Model;
use crate::ui;

// ---------------------------------------------------------------------------
// Timeline state
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct TimelineState {
    pub tweets: Vec<Tweet>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub next_token: Option<String>,
    pub loading: bool,
    pub includes: Option<Includes>,
}

// ---------------------------------------------------------------------------
// App mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Command,
    Search,
}

// ---------------------------------------------------------------------------
// View state
// ---------------------------------------------------------------------------

pub struct ViewState {
    pub kind: ViewKind,
    pub scroll_offset: usize,
    pub selected_index: usize,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct App {
    pub running: bool,
    pub events: EventHandler,
    pub config: AppConfig,

    // View system
    pub view_stack: Vec<ViewState>,
    pub mode: AppMode,

    // Data state
    pub home_timeline: TimelineState,
    pub mentions: TimelineState,
    pub bookmarks: TimelineState,
    pub search_results: TimelineState,
    pub search_query: String,
    pub current_user: Option<User>,
    pub viewed_user: Option<User>,
    pub viewed_user_timeline: TimelineState,
    pub thread_tweets: Vec<Tweet>,
    pub thread_root: Option<Tweet>,
    pub followers: Vec<User>,
    pub following: Vec<User>,

    // Input state
    pub command_input: String,
    pub search_input: String,

    // Credentials (needed for runtime auth flows)
    pub credentials: CredentialSet,

    // API client (wrapped for sharing with spawned tasks)
    pub api_client: Option<Arc<Mutex<XApiClient>>>,

    // Includes cache (users from API responses for author lookup)
    pub users_cache: HashMap<String, User>,

    // OpenRouter client
    pub openrouter_client: Option<Arc<OpenRouterClient>>,

    // MLX embedding server client
    pub mlx_client: Option<Arc<MlxClient>>,
    /// Whether the MLX server supports embeddings (probed at startup).
    pub mlx_embed_supported: bool,
    /// Whether the MLX server supports chat completions (probed at startup).
    pub mlx_chat_supported: bool,

    // OpenRouter model selection
    pub openrouter_models: Vec<Model>,
    pub selected_embedding_model: Option<String>,
    pub models_loading: bool,

    // Text model selection (for chat/topic generation)
    pub text_models: Vec<Model>,
    pub selected_chat_model: Option<String>,
    pub text_models_loading: bool,

    /// User-preferred chat provider. `None` = auto (MLX if available, else OpenRouter).
    pub preferred_chat_provider: Option<dispatch::ChatProviderKind>,

    // Model filter state (shared by both model views)
    pub model_filter: Option<String>,
    pub model_filter_open: bool,
    pub model_filter_index: usize,

    // Model search state
    pub model_filter_search: String,
    pub model_filter_search_active: bool,
    pub model_search: String,
    pub model_search_active: bool,

    // HuggingFace Hub models
    pub hf_models: Vec<crate::huggingface::types::HfModel>,
    pub hf_models_loading: bool,
    pub hf_search: String,
    pub hf_search_active: bool,
    /// Org filter for HF models view (e.g. "mlx-community").
    pub hf_org_filter: Option<String>,
    pub hf_org_filter_open: bool,
    pub hf_org_filter_index: usize,

    // Clustering state
    pub cluster_result: Option<ClusterResult>,
    pub cluster_loading: bool,
    pub cluster_topics_loading: bool,
    /// Monotonic counter incremented on each cluster/topic-generation request.
    /// Used to discard stale async responses.
    pub cluster_generation: u64,
    /// `None` = cluster list mode, `Some(c)` = viewing tweets in cluster c.
    pub selected_cluster: Option<usize>,
    /// When true, a cluster operation will be triggered after the home timeline refresh completes.
    pub refresh_then_cluster: bool,

    // Status
    pub status_message: Option<String>,
    pub error_detail: Option<String>,
    pub loading: bool,
}

impl App {
    pub fn new(
        config: AppConfig,
        api_client: Option<XApiClient>,
        credentials: CredentialSet,
    ) -> Self {
        let default_view = match config.default_view {
            crate::config::DefaultView::Home => ViewKind::Home,
            crate::config::DefaultView::Mentions => ViewKind::Mentions,
            crate::config::DefaultView::Bookmarks => ViewKind::Bookmarks,
            crate::config::DefaultView::Search => ViewKind::Search,
        };

        let initial_view = ViewState {
            kind: default_view,
            scroll_offset: 0,
            selected_index: 0,
        };

        let mlx_client = config
            .mlx_server_url
            .as_ref()
            .map(|url| Arc::new(MlxClient::new(url.clone())));

        Self {
            running: true,
            events: EventHandler::new(),
            config,
            view_stack: vec![initial_view],
            mode: AppMode::Normal,
            home_timeline: TimelineState::default(),
            mentions: TimelineState::default(),
            bookmarks: TimelineState::default(),
            search_results: TimelineState::default(),
            search_query: String::new(),
            current_user: None,
            viewed_user: None,
            viewed_user_timeline: TimelineState::default(),
            thread_tweets: Vec::new(),
            thread_root: None,
            followers: Vec::new(),
            following: Vec::new(),
            command_input: String::new(),
            search_input: String::new(),
            credentials,
            api_client: api_client.map(|c| Arc::new(Mutex::new(c))),
            users_cache: HashMap::new(),
            mlx_client,
            mlx_embed_supported: false,
            mlx_chat_supported: false,
            openrouter_client: None,
            openrouter_models: Vec::new(),
            selected_embedding_model: None,
            models_loading: false,
            text_models: Vec::new(),
            selected_chat_model: None,
            text_models_loading: false,
            preferred_chat_provider: None,
            model_filter: None,
            model_filter_open: false,
            model_filter_index: 0,
            model_filter_search: String::new(),
            model_filter_search_active: false,
            model_search: String::new(),
            model_search_active: false,
            hf_models: Vec::new(),
            hf_models_loading: false,
            hf_search: String::new(),
            hf_search_active: false,
            hf_org_filter: None,
            hf_org_filter_open: false,
            hf_org_filter_index: 0,

            cluster_result: None,
            cluster_loading: false,
            cluster_topics_loading: false,
            cluster_generation: 0,
            selected_cluster: None,
            refresh_then_cluster: false,
            status_message: None,
            error_detail: None,
            loading: false,
        }
    }

    // -- Main event loop ----------------------------------------------------

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        // Try to initialize OpenRouter client from stored credentials.
        self.init_openrouter_client();

        // Probe MLX server capabilities (non-blocking, fast health check).
        if let Some(ref mlx) = self.mlx_client {
            let caps = mlx.capabilities().await;
            self.mlx_embed_supported = caps.iter().any(|c| c == "embeddings");
            self.mlx_chat_supported = caps.iter().any(|c| c == "chat");
        }

        // Trigger initial data fetch based on default view.
        match self.current_view() {
            Some(ViewKind::Home) => {
                self.events.send(AppEvent::FetchHomeTimeline {
                    pagination_token: None,
                });
            }
            Some(ViewKind::Mentions) => {
                self.events.send(AppEvent::FetchMentions {
                    pagination_token: None,
                });
            }
            Some(ViewKind::Bookmarks) => {
                self.events.send(AppEvent::FetchBookmarks {
                    pagination_token: None,
                });
            }
            _ => {}
        }

        while self.running {
            terminal.draw(|frame| self.draw(frame))?;
            match self.events.next().await? {
                Event::Tick => self.tick(),
                Event::Crossterm(event) => {
                    if let crossterm::event::Event::Key(key) = event
                        && key.kind == crossterm::event::KeyEventKind::Press
                    {
                        self.handle_key_event(key);
                    }
                }
                Event::App(app_event) => {
                    if matches!(*app_event, AppEvent::StartAuth) {
                        self.run_auth_flow(&mut terminal).await;
                    } else if matches!(*app_event, AppEvent::StartOpenRouterAuth) {
                        self.run_openrouter_auth_flow(&mut terminal).await;
                    } else {
                        self.handle_app_event(*app_event);
                    }
                }
            }
        }
        Ok(())
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        ui::draw(frame, self);
    }

    fn tick(&self) {}

    // -- View stack ---------------------------------------------------------

    pub fn current_view(&self) -> Option<&ViewKind> {
        self.view_stack.last().map(|vs| &vs.kind)
    }

    pub fn push_view(&mut self, kind: ViewKind) {
        self.view_stack.push(ViewState {
            kind,
            scroll_offset: 0,
            selected_index: 0,
        });
    }

    pub fn pop_view(&mut self) {
        if self.view_stack.len() > 1 {
            self.view_stack.pop();
        }
    }

    fn set_error(&mut self, msg: String) {
        self.status_message = Some(msg.clone());
        self.error_detail = Some(msg);
    }
}

/// Build a tweet URL. Uses `x.com/i/status/{id}` as fallback when username is unknown.
fn tweet_url(tweet_id: &str, username: Option<&str>) -> String {
    match username {
        Some(name) => format!("https://x.com/{name}/status/{tweet_id}"),
        None => format!("https://x.com/i/status/{tweet_id}"),
    }
}

#[cfg(test)]
mod tests {
    use crate::openrouter;

    #[test]
    fn strip_think_tags_removes_reasoning() {
        let input = "<think>\nLet me analyze...\nCluster 0 is about tech\n</think>\nTech Innovation\nCrypto Trading";
        assert_eq!(
            openrouter::strip_think_tags(input),
            "\nTech Innovation\nCrypto Trading"
        );
    }

    #[test]
    fn strip_think_tags_no_tags() {
        assert_eq!(openrouter::strip_think_tags("hello world"), "hello world");
    }

    #[test]
    fn strip_think_tags_unclosed() {
        let input = "before<think>reasoning without end";
        assert_eq!(openrouter::strip_think_tags(input), "before");
    }

    #[test]
    fn strip_think_tags_multiple() {
        let input = "<think>first</think>middle<think>second</think>end";
        assert_eq!(openrouter::strip_think_tags(input), "middleend");
    }

    #[test]
    fn extract_provider_splits_correctly() {
        assert_eq!(openrouter::extract_provider("openai/gpt-4o"), "openai");
        assert_eq!(
            openrouter::extract_provider("anthropic/claude-3.5-haiku"),
            "anthropic"
        );
        assert_eq!(
            openrouter::extract_provider("bare-model-id"),
            "bare-model-id"
        );
    }
}
