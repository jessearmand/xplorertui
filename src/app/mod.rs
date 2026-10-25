mod auth;
mod clustering;
mod commands;
mod dispatch;
mod event_handlers;
mod key_handlers;
mod llama_cpp;
mod navigation;
pub mod photos;
mod popup;

pub use clustering::ClusterFill;
pub use llama_cpp::{LlamaCppProbe, LlamaCppState};
pub use photos::PhotoView;
pub use popup::{Popup, PopupTone, PromptAction};

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use ratatui::DefaultTerminal;

use crate::api::XApiClient;
use crate::api::types::{Includes, Media, Tweet, User};
use crate::auth::credentials::CredentialSet;
use crate::config::AppConfig;
use crate::embeddings::cluster::ClusterResult;
use crate::event::{AppEvent, Event, EventHandler, ViewKind};
use crate::media::PhotoCache;
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
// Cluster source
// ---------------------------------------------------------------------------

/// Which timeline the current cluster result was computed from.
///
/// `Home` maps to `reverse_chronological` at the API level — the Following
/// feed. Its `Display` impl renders `"following"` so the UI doesn't overload
/// the X app's "Home" tab (which defaults to the algorithmic "For You" feed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClusterSource {
    Home,
    Mentions,
    Search,
    Bookmarks,
}

impl ClusterSource {
    pub fn from_view(view: &ViewKind) -> Option<Self> {
        match view {
            ViewKind::Home => Some(Self::Home),
            ViewKind::Mentions => Some(Self::Mentions),
            ViewKind::Search => Some(Self::Search),
            ViewKind::Bookmarks => Some(Self::Bookmarks),
            _ => None,
        }
    }
}

impl std::fmt::Display for ClusterSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Home => "following",
            Self::Mentions => "mentions",
            Self::Search => "search",
            Self::Bookmarks => "bookmarks",
        };
        f.write_str(label)
    }
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
    /// Photos and videos from API responses, by media key.
    pub media_cache: HashMap<String, Media>,

    // Photos
    /// Terminal graphics protocol and font size, found once at start.
    pub picker: ratatui_image::picker::Picker,
    /// Downloaded and prepared photos, shared with background tasks.
    pub photo_cache: Arc<PhotoCache>,
    /// Encoded photos for the thread view, by photo URL.
    pub photo_views: HashMap<String, PhotoView>,
    /// Box height each photo was last asked to be encoded for, by photo URL.
    pub photo_requested_rows: HashMap<String, u16>,
    /// Rows the thread view can give the photo, from its last layout. The
    /// view sets it while drawing; the next tick encodes for it.
    pub photo_box_rows: std::cell::Cell<u16>,
    /// Which photo of the focused post the thread view shows.
    pub photo_index: usize,

    // OpenRouter client
    pub openrouter_client: Option<Arc<OpenRouterClient>>,

    // MLX embedding server client
    pub mlx_client: Option<Arc<MlxClient>>,
    /// Whether the MLX server supports embeddings (probed at startup).
    pub mlx_embed_supported: bool,
    /// Whether the MLX server supports chat completions (probed at startup).
    pub mlx_chat_supported: bool,

    /// llama.cpp servers from `[llama_cpp]` and what their probes found.
    pub llama_cpp: LlamaCppState,

    // OpenRouter model selection
    pub openrouter_models: Vec<Model>,
    pub selected_embedding_model: Option<String>,
    pub models_loading: bool,

    // Text model selection (for chat/topic generation)
    pub text_models: Vec<Model>,
    pub selected_chat_model: Option<String>,
    pub text_models_loading: bool,

    /// User-preferred chat provider. `None` = auto (MLX, then llama.cpp, then
    /// OpenRouter).
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
    /// The labeller the running `:topics` request is using right now, once
    /// it has picked one. Shown in the loading hint.
    pub cluster_topics_labeller: Option<String>,
    /// Monotonic counter incremented on each cluster/topic-generation request.
    /// Used to discard stale async responses.
    pub cluster_generation: u64,
    /// `None` = cluster list mode, `Some(c)` = viewing tweets in cluster c.
    pub selected_cluster: Option<usize>,
    /// Which timeline the current cluster result was derived from. Set at
    /// `:cluster` time, consumed by the dispatcher, the UI labels, and the
    /// refresh flow in the Cluster view.
    pub cluster_source: Option<ClusterSource>,
    /// When true, a cluster operation will be triggered after the source
    /// timeline (determined by `cluster_source`) finishes refreshing.
    pub refresh_then_cluster: bool,
    /// Set while `:cluster` loads more posts: the post count before the
    /// page in flight. A page that adds posts leads to the next one; a
    /// failed or empty page ends the loading.
    pub cluster_fill: Option<ClusterFill>,

    // Status
    pub status_message: Option<String>,
    /// Modal overlay (error details, full status message, or a prompt).
    pub popup: Option<Popup>,
    /// First visible text row of the open popup (`j`/`k` scroll). The
    /// renderer clamps it, so it may exceed the text length.
    pub popup_scroll: u16,
    /// Cluster source to re-run once an embedding model is selected (set
    /// when the user accepts the "Embedding Model Needed" prompt).
    pub resume_cluster_after_model: Option<ClusterSource>,
    pub loading: bool,

    // Skeleton loading animation
    /// App-creation timestamp used to compute `elapsed_ms` for skeleton widgets.
    pub epoch: Instant,
    /// When any loading operation started. Skeleton animations only render after
    /// a debounce threshold (200ms) to avoid flicker on fast responses.
    pub loading_started_at: Option<Instant>,
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
        let llama_cpp = LlamaCppState::from_config(config.llama_cpp.as_ref());

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
            media_cache: HashMap::new(),
            picker: ratatui_image::picker::Picker::halfblocks(),
            photo_cache: Arc::new(PhotoCache::new()),
            photo_views: HashMap::new(),
            photo_requested_rows: HashMap::new(),
            photo_box_rows: std::cell::Cell::new(photos::PHOTO_ROWS),
            photo_index: 0,
            mlx_client,
            mlx_embed_supported: false,
            mlx_chat_supported: false,
            llama_cpp,
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
            cluster_topics_labeller: None,
            cluster_generation: 0,
            selected_cluster: None,
            cluster_source: None,
            refresh_then_cluster: false,
            cluster_fill: None,
            status_message: None,
            popup: None,
            popup_scroll: 0,
            resume_cluster_after_model: None,
            loading: false,
            epoch: Instant::now(),
            loading_started_at: None,
        }
    }

    /// Use the terminal's graphics protocol for photos. `new()` starts with
    /// half-block characters, which work in every terminal.
    pub fn with_picker(mut self, picker: ratatui_image::picker::Picker) -> Self {
        self.picker = picker;
        self
    }

    /// Returns `Some(elapsed_ms)` for skeleton animation if any loading has been
    /// active for longer than the debounce threshold (200ms). Returns `None` if
    /// nothing is loading or loading just started (no skeleton should render yet).
    ///
    /// Use this for network fetches where fast responses shouldn't flicker.
    pub fn skeleton_elapsed_ms(&self) -> Option<u64> {
        const DEBOUNCE_MS: u128 = 200;
        let started = self.loading_started_at?;
        if started.elapsed().as_millis() < DEBOUNCE_MS {
            return None;
        }
        Some(self.epoch.elapsed().as_millis() as u64)
    }

    /// Returns `elapsed_ms` for skeleton animation without debounce.
    ///
    /// Use this for operations known to be slow (clustering, model loading)
    /// where the skeleton should appear immediately.
    pub fn skeleton_elapsed_ms_immediate(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    /// Mark that a loading operation has started. Only records the timestamp
    /// on the first call; subsequent calls while already loading are no-ops.
    pub fn mark_loading_started(&mut self) {
        if self.loading_started_at.is_none() {
            self.loading_started_at = Some(Instant::now());
        }
    }

    /// Check whether any loading flag is still active and clear
    /// `loading_started_at` if everything has finished.
    pub fn check_loading_finished(&mut self) {
        let any_loading = self.loading
            || self.home_timeline.loading
            || self.mentions.loading
            || self.bookmarks.loading
            || self.search_results.loading
            || self.viewed_user_timeline.loading
            || self.cluster_loading
            || self.cluster_topics_loading
            || self.models_loading
            || self.text_models_loading
            || self.hf_models_loading;
        if !any_loading {
            self.loading_started_at = None;
        }
    }

    // -- Main event loop ----------------------------------------------------

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        // Try to initialize OpenRouter client from stored credentials.
        self.init_openrouter_client();

        // Probe the local servers in the background: the results arrive as
        // MLXCapabilitiesProbed and LlamaCppProbed, so a slow or stalled
        // server never holds up the first draw.
        self.dispatch_probe_mlx();
        self.dispatch_probe_llama_cpp();

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

    fn tick(&mut self) {
        self.ensure_focused_photo();
    }

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
        self.open_popup(Popup::error(msg));
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
    fn strip_think_tags_handles_gemma_channel_variants() {
        let input = "<|channel>reasoning<channel|>\nAnswer";
        assert_eq!(openrouter::strip_think_tags(input), "\nAnswer");
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
