use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::api::XApiClient;
use crate::api::types::{Includes, Tweet, User};
use crate::auth::AuthProvider;
use crate::auth::credentials::CredentialSet;
use crate::command::{self, Command};
use crate::config::AppConfig;
use crate::embeddings::cluster::ClusterResult;
use crate::event::{ApiResult, AppEvent, Event, EventHandler, ViewKind};
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

    // OpenRouter model selection
    pub openrouter_models: Vec<Model>,
    pub selected_embedding_model: Option<String>,
    pub models_loading: bool,

    // Clustering state
    pub cluster_result: Option<ClusterResult>,
    pub cluster_loading: bool,
    /// `None` = cluster list mode, `Some(c)` = viewing tweets in cluster c.
    pub selected_cluster: Option<usize>,

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
            openrouter_client: None,
            openrouter_models: Vec::new(),
            selected_embedding_model: None,
            models_loading: false,
            cluster_result: None,
            cluster_loading: false,
            selected_cluster: None,
            status_message: None,
            error_detail: None,
            loading: false,
        }
    }

    // -- Main event loop ----------------------------------------------------

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        // Try to initialize OpenRouter client from stored credentials.
        self.init_openrouter_client();

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

    // -- Key event routing --------------------------------------------------

    fn handle_key_event(&mut self, key: KeyEvent) {
        // Ctrl-C always quits.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            self.events.send(AppEvent::Quit);
            return;
        }

        // Dismiss error popup if open (swallow all other keys).
        if self.error_detail.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    self.error_detail = None;
                }
                _ => {}
            }
            return;
        }

        match self.mode {
            AppMode::Normal => self.handle_normal_key(key),
            AppMode::Command => self.handle_command_key(key),
            AppMode::Search => self.handle_search_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                // In cluster tweet list mode, go back to cluster list first
                if self.current_view() == Some(&ViewKind::Cluster)
                    && self.selected_cluster.is_some()
                {
                    let cluster_idx = self.selected_cluster.unwrap();
                    self.selected_cluster = None;
                    if let Some(vs) = self.view_stack.last_mut() {
                        vs.selected_index = cluster_idx;
                    }
                } else if self.view_stack.len() > 1 {
                    self.events.send(AppEvent::PopView);
                } else {
                    self.events.send(AppEvent::Quit);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection_up();
            }
            KeyCode::Enter => {
                self.open_selected();
            }
            KeyCode::Char('/') => {
                self.mode = AppMode::Search;
                self.search_input.clear();
            }
            KeyCode::Char(':') => {
                self.mode = AppMode::Command;
                self.command_input.clear();
            }
            KeyCode::Char('?') => {
                self.events.send(AppEvent::PushView(ViewKind::Help));
            }
            KeyCode::Char('1') => {
                self.events.send(AppEvent::SwitchView(ViewKind::Home));
            }
            KeyCode::Char('2') => {
                self.events.send(AppEvent::SwitchView(ViewKind::Mentions));
            }
            KeyCode::Char('3') => {
                self.events.send(AppEvent::SwitchView(ViewKind::Bookmarks));
            }
            KeyCode::Char('4') => {
                self.events.send(AppEvent::SwitchView(ViewKind::Search));
            }
            KeyCode::Char('@') => {
                self.mode = AppMode::Command;
                self.command_input = "user ".to_string();
            }
            KeyCode::Char('n') => {
                self.load_next_page();
            }
            _ => {}
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.command_input.clear();
            }
            KeyCode::Enter => {
                self.execute_command();
                self.mode = AppMode::Normal;
            }
            KeyCode::Backspace => {
                self.command_input.pop();
            }
            KeyCode::Char(c) => {
                self.command_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.search_input.clear();
            }
            KeyCode::Enter => {
                let query = self.search_input.clone();
                if !query.is_empty() {
                    self.search_query = query.clone();
                    self.events.send(AppEvent::FetchSearch {
                        query,
                        pagination_token: None,
                    });
                    self.events.send(AppEvent::SwitchView(ViewKind::Search));
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Backspace => {
                self.search_input.pop();
            }
            KeyCode::Char(c) => {
                self.search_input.push(c);
            }
            _ => {}
        }
    }

    // -- Command execution --------------------------------------------------

    fn execute_command(&mut self) {
        let input = self.command_input.clone();
        match command::parse_command(&input) {
            Some(Command::User(username)) => {
                self.events.send(AppEvent::FetchUser { username });
            }
            Some(Command::Search(query)) => {
                self.search_query = query.clone();
                self.events.send(AppEvent::FetchSearch {
                    query,
                    pagination_token: None,
                });
                self.events.send(AppEvent::SwitchView(ViewKind::Search));
            }
            Some(Command::Open(url_or_id)) => {
                if let Some(tweet_id) = command::parse_tweet_url(&url_or_id) {
                    self.events.send(AppEvent::FetchTweet { tweet_id });
                } else {
                    self.status_message = Some(format!("Invalid tweet URL or ID: {url_or_id}"));
                }
            }
            Some(Command::Home) => {
                self.events.send(AppEvent::SwitchView(ViewKind::Home));
            }
            Some(Command::Mentions) => {
                self.events.send(AppEvent::SwitchView(ViewKind::Mentions));
            }
            Some(Command::Bookmarks) => {
                self.events.send(AppEvent::SwitchView(ViewKind::Bookmarks));
            }
            Some(Command::Help) => {
                self.events.send(AppEvent::PushView(ViewKind::Help));
            }
            Some(Command::Auth) => {
                self.events.send(AppEvent::StartAuth);
            }
            Some(Command::OpenRouterAuth) => {
                self.events.send(AppEvent::StartOpenRouterAuth);
            }
            Some(Command::Models) => {
                self.events.send(AppEvent::FetchOpenRouterModels);
                self.events
                    .send(AppEvent::PushView(ViewKind::OpenRouterModels));
            }
            Some(Command::Cluster) => {
                self.events.send(AppEvent::ClusterTimeline);
            }
            Some(Command::Quit) => {
                self.events.send(AppEvent::Quit);
            }
            None => {
                self.status_message = Some(format!("Unknown command: {input}"));
            }
        }
        self.command_input.clear();
    }

    // -- Selection helpers --------------------------------------------------

    fn move_selection_down(&mut self) {
        let count = self.current_item_count();
        if let Some(vs) = self.view_stack.last_mut()
            && vs.selected_index + 1 < count
        {
            vs.selected_index += 1;
        }
    }

    fn move_selection_up(&mut self) {
        if let Some(vs) = self.view_stack.last_mut() {
            vs.selected_index = vs.selected_index.saturating_sub(1);
        }
    }

    fn current_item_count(&self) -> usize {
        match self.current_view() {
            Some(ViewKind::Home) => self.home_timeline.tweets.len(),
            Some(ViewKind::Mentions) => self.mentions.tweets.len(),
            Some(ViewKind::Bookmarks) => self.bookmarks.tweets.len(),
            Some(ViewKind::Search) => self.search_results.tweets.len(),
            Some(ViewKind::UserTimeline(_)) => self.viewed_user_timeline.tweets.len(),
            Some(ViewKind::Thread(_)) => self.thread_tweets.len(),
            Some(ViewKind::UserProfile(_)) => 0,
            Some(ViewKind::OpenRouterModels) => self.openrouter_models.len(),
            Some(ViewKind::Cluster) => {
                if let Some(ref result) = self.cluster_result {
                    if let Some(c) = self.selected_cluster {
                        result.tweet_indices_for_cluster(c).len()
                    } else {
                        result.num_clusters()
                    }
                } else {
                    0
                }
            }
            Some(ViewKind::Help) => 0,
            None => 0,
        }
    }

    pub fn selected_index(&self) -> usize {
        self.view_stack.last().map_or(0, |vs| vs.selected_index)
    }

    fn open_selected(&mut self) {
        let idx = self.selected_index();
        match self.current_view().cloned() {
            Some(ViewKind::Home) => {
                if let Some(tweet) = self.home_timeline.tweets.get(idx) {
                    let conv_id = tweet
                        .conversation_id
                        .clone()
                        .unwrap_or_else(|| tweet.id.clone());
                    self.events.send(AppEvent::FetchThread {
                        conversation_id: conv_id,
                        pagination_token: None,
                    });
                }
            }
            Some(ViewKind::Mentions) => {
                if let Some(tweet) = self.mentions.tweets.get(idx) {
                    let conv_id = tweet
                        .conversation_id
                        .clone()
                        .unwrap_or_else(|| tweet.id.clone());
                    self.events.send(AppEvent::FetchThread {
                        conversation_id: conv_id,
                        pagination_token: None,
                    });
                }
            }
            Some(ViewKind::Bookmarks) => {
                if let Some(tweet) = self.bookmarks.tweets.get(idx) {
                    let conv_id = tweet
                        .conversation_id
                        .clone()
                        .unwrap_or_else(|| tweet.id.clone());
                    self.events.send(AppEvent::FetchThread {
                        conversation_id: conv_id,
                        pagination_token: None,
                    });
                }
            }
            Some(ViewKind::Search) => {
                if let Some(tweet) = self.search_results.tweets.get(idx) {
                    let conv_id = tweet
                        .conversation_id
                        .clone()
                        .unwrap_or_else(|| tweet.id.clone());
                    self.events.send(AppEvent::FetchThread {
                        conversation_id: conv_id,
                        pagination_token: None,
                    });
                }
            }
            Some(ViewKind::UserTimeline(_)) => {
                if let Some(tweet) = self.viewed_user_timeline.tweets.get(idx) {
                    let conv_id = tweet
                        .conversation_id
                        .clone()
                        .unwrap_or_else(|| tweet.id.clone());
                    self.events.send(AppEvent::FetchThread {
                        conversation_id: conv_id,
                        pagination_token: None,
                    });
                }
            }
            Some(ViewKind::OpenRouterModels) => {
                if let Some(model) = self.openrouter_models.get(idx) {
                    let model_id = model.id.clone();
                    self.events
                        .send(AppEvent::SelectEmbeddingModel { model_id });
                }
            }
            Some(ViewKind::Cluster) => {
                if let Some(ref result) = self.cluster_result {
                    if let Some(c) = self.selected_cluster {
                        // In tweet list mode — open the selected tweet's thread
                        let indices = result.tweet_indices_for_cluster(c);
                        if let Some(&orig_idx) = indices.get(idx) {
                            let conv_id = result.conversation_ids[orig_idx]
                                .clone()
                                .unwrap_or_else(|| result.tweet_ids[orig_idx].clone());
                            self.events.send(AppEvent::FetchThread {
                                conversation_id: conv_id,
                                pagination_token: None,
                            });
                        }
                    } else {
                        // In cluster list mode — enter tweet list for this cluster
                        let num = result.num_clusters();
                        if idx < num {
                            self.selected_cluster = Some(idx);
                            if let Some(vs) = self.view_stack.last_mut() {
                                vs.selected_index = 0;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn load_next_page(&mut self) {
        match self.current_view().cloned() {
            Some(ViewKind::Home) => {
                if let Some(token) = self.home_timeline.next_token.clone() {
                    self.events.send(AppEvent::FetchHomeTimeline {
                        pagination_token: Some(token),
                    });
                }
            }
            Some(ViewKind::Mentions) => {
                if let Some(token) = self.mentions.next_token.clone() {
                    self.events.send(AppEvent::FetchMentions {
                        pagination_token: Some(token),
                    });
                }
            }
            Some(ViewKind::Bookmarks) => {
                if let Some(token) = self.bookmarks.next_token.clone() {
                    self.events.send(AppEvent::FetchBookmarks {
                        pagination_token: Some(token),
                    });
                }
            }
            Some(ViewKind::Search) => {
                if let Some(token) = self.search_results.next_token.clone() {
                    let query = self.search_query.clone();
                    self.events.send(AppEvent::FetchSearch {
                        query,
                        pagination_token: Some(token),
                    });
                }
            }
            Some(ViewKind::UserTimeline(ref user_id)) => {
                let user_id = user_id.clone();
                if let Some(token) = self.viewed_user_timeline.next_token.clone() {
                    self.events.send(AppEvent::FetchUserTimeline {
                        user_id,
                        pagination_token: Some(token),
                    });
                }
            }
            Some(ViewKind::Thread(ref conv_id)) => {
                let conv_id = conv_id.clone();
                // Threads don't currently track next_token, but could be added
                self.events.send(AppEvent::FetchThread {
                    conversation_id: conv_id,
                    pagination_token: None,
                });
            }
            _ => {}
        }
    }

    // -- Auth flow (suspends TUI) ------------------------------------------

    async fn run_auth_flow(&mut self, terminal: &mut DefaultTerminal) {
        let Some(ref oauth2_creds) = self.credentials.oauth2 else {
            self.status_message =
                Some("OAuth 2.0 not configured. Set X_CLIENT_ID in your .env file.".into());
            return;
        };
        let oauth2_creds = oauth2_creds.clone();

        // Suspend the TUI so the user can interact with their browser.
        ratatui::restore();

        let result = crate::auth::oauth2_pkce::start_pkce_flow(
            &oauth2_creds,
            self.config.oauth_callback_port,
        )
        .await;

        match &result {
            Ok(_) => {
                println!();
                println!("Authentication successful! Tokens saved.");
            }
            Err(e) => {
                println!();
                println!("Authentication failed: {e}");
            }
        }

        println!();
        println!("Press Enter to return to the TUI...");
        let _ = std::io::stdin().read_line(&mut String::new());

        // Re-initialize the terminal and event handler.
        *terminal = ratatui::init();
        self.events = EventHandler::new();

        // On success, rebuild the API client with the new tokens.
        if result.is_ok() {
            match AuthProvider::new(self.credentials.clone()) {
                Ok(auth) => {
                    self.api_client = Some(Arc::new(Mutex::new(XApiClient::new(
                        auth,
                        self.config.oauth_callback_port,
                    ))));
                    self.status_message = Some("Authenticated successfully!".into());
                }
                Err(e) => {
                    self.status_message = Some(format!("Auth provider error: {e}"));
                }
            }
        } else if let Err(e) = result {
            self.status_message = Some(format!("Auth failed: {e}"));
        }
    }

    // -- OpenRouter auth flow (suspends TUI) ---------------------------------

    async fn run_openrouter_auth_flow(&mut self, terminal: &mut DefaultTerminal) {
        ratatui::restore();

        let port = if self.config.openrouter_callback_port == 8478 {
            eprintln!(
                "Using OpenRouter callback port 3000 (legacy 8478 value detected in config)."
            );
            3000
        } else {
            self.config.openrouter_callback_port
        };

        let result = crate::openrouter::auth::start_openrouter_auth(port).await;

        match &result {
            Ok(_) => {
                println!();
                println!("OpenRouter authentication successful! API key saved.");
            }
            Err(e) => {
                println!();
                println!("OpenRouter authentication failed: {e}");
            }
        }

        println!();
        println!("Press Enter to return to the TUI...");
        let _ = std::io::stdin().read_line(&mut String::new());

        // Re-initialize the terminal and event handler.
        *terminal = ratatui::init();
        self.events = EventHandler::new();

        // On success, create the OpenRouter client.
        if result.is_ok() {
            match crate::cli::build_openrouter_client() {
                Ok(client) => {
                    self.openrouter_client = Some(Arc::new(client));
                    self.status_message = Some("OpenRouter authenticated successfully!".into());
                }
                Err(e) => {
                    self.status_message = Some(format!("OpenRouter client error: {e}"));
                }
            }
        } else if let Err(e) = result {
            self.status_message = Some(format!("OpenRouter auth failed: {e}"));
        }
    }

    /// Try to initialize the OpenRouter client from stored credentials.
    pub fn init_openrouter_client(&mut self) {
        if self.openrouter_client.is_some() {
            return;
        }
        if let Ok(client) = crate::cli::build_openrouter_client() {
            self.openrouter_client = Some(Arc::new(client));
        }
    }

    // -- OpenRouter dispatch methods ------------------------------------------

    fn dispatch_openrouter_models(&self) {
        let Some(ref client) = self.openrouter_client else {
            self.events
                .send(AppEvent::OpenRouterModelsLoaded(Err(Arc::new(
                    "OpenRouter not configured. Use :openrouter-auth first.".into(),
                ))));
            return;
        };
        let client = Arc::clone(client);
        let sender = self.events.sender();

        tokio::spawn(async move {
            let result: Result<crate::openrouter::types::ModelsResponse, _> =
                client.get("/embeddings/models").await;
            let mapped: ApiResult<Vec<Model>> =
                result.map(|r| r.data).map_err(|e| Arc::new(e.to_string()));
            let _ = sender.send(Event::App(Box::new(AppEvent::OpenRouterModelsLoaded(
                mapped,
            ))));
        });
    }

    fn dispatch_embed_and_rank(&self, query: String, tweets: Vec<Tweet>) {
        let Some(ref or_client) = self.openrouter_client else {
            return;
        };
        let Some(ref model_id) = self.selected_embedding_model else {
            return;
        };
        let client = Arc::clone(or_client);
        let model = model_id.clone();
        let sender = self.events.sender();
        let query_clone = query.clone();

        tokio::spawn(async move {
            let result = async {
                // Build texts: query + all tweet texts
                let mut texts: Vec<String> = vec![query_clone.clone()];
                texts.extend(tweets.iter().map(|t| t.text.clone()));

                let resp = client
                    .embed(&model, &texts)
                    .await
                    .map_err(|e| Arc::new(e.to_string()))?;

                if resp.data.len() != texts.len() {
                    return Err(Arc::new(format!(
                        "Expected {} embeddings, got {}",
                        texts.len(),
                        resp.data.len()
                    )));
                }

                // First embedding is the query, rest are tweets.
                let mut sorted_data: Vec<_> = resp.data;
                sorted_data.sort_by_key(|d| d.index);
                let query_emb = &sorted_data[0].embedding;
                let tweet_embs: Vec<(usize, Vec<f64>)> = sorted_data[1..]
                    .iter()
                    .enumerate()
                    .map(|(i, d)| (i, d.embedding.clone()))
                    .collect();

                let ranked =
                    crate::embeddings::similarity::rank_by_similarity(query_emb, &tweet_embs);

                let result: Vec<(Tweet, f64)> = ranked
                    .into_iter()
                    .filter_map(|(idx, score)| tweets.get(idx).map(|t| (t.clone(), score)))
                    .collect();

                Ok(result)
            }
            .await;

            let _ = sender.send(Event::App(Box::new(AppEvent::SearchRanked {
                query: query_clone,
                result,
            })));
        });
    }

    fn dispatch_cluster_timeline(&self) {
        let Some(ref or_client) = self.openrouter_client else {
            self.events.send(AppEvent::ClusteringComplete(Err(Arc::new(
                "OpenRouter not configured. Use :openrouter-auth first.".into(),
            ))));
            return;
        };
        let Some(ref model_id) = self.selected_embedding_model else {
            self.events.send(AppEvent::ClusteringComplete(Err(Arc::new(
                "No embedding model selected. Use :models first.".into(),
            ))));
            return;
        };
        let client = Arc::clone(or_client);
        let model = model_id.clone();
        let sender = self.events.sender();
        let tweets = self.home_timeline.tweets.clone();

        tokio::spawn(async move {
            let result = async {
                let texts: Vec<String> = tweets.iter().map(|t| t.text.clone()).collect();
                let ids: Vec<String> = tweets.iter().map(|t| t.id.clone()).collect();
                let conv_ids: Vec<Option<String>> =
                    tweets.iter().map(|t| t.conversation_id.clone()).collect();

                let resp = client
                    .embed(&model, &texts)
                    .await
                    .map_err(|e| Arc::new(e.to_string()))?;

                let mut sorted_data: Vec<_> = resp.data;
                sorted_data.sort_by_key(|d| d.index);
                let embeddings: Vec<Vec<f64>> =
                    sorted_data.into_iter().map(|d| d.embedding).collect();

                let k = 5.min(tweets.len());
                let cluster_result = crate::embeddings::cluster::build_cluster_result(
                    &embeddings,
                    texts,
                    ids,
                    conv_ids,
                    k,
                );

                Ok(cluster_result)
            }
            .await;

            let _ = sender.send(Event::App(Box::new(AppEvent::ClusteringComplete(result))));
        });
    }

    // -- App event handling -------------------------------------------------

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            // Navigation
            AppEvent::Quit => {
                self.running = false;
            }
            AppEvent::PushView(kind) => {
                self.push_view(kind);
            }
            AppEvent::PopView => {
                self.pop_view();
            }
            AppEvent::SwitchView(kind) => {
                // Replace the root view or push if stack is deeper.
                if self.view_stack.len() <= 1 {
                    self.view_stack.clear();
                    self.push_view(kind.clone());
                } else {
                    self.push_view(kind.clone());
                }
                // Trigger fetch if data is empty.
                self.fetch_for_view(&kind);
            }

            // API request triggers -> dispatch to async tasks.
            ref evt @ (AppEvent::FetchHomeTimeline { .. }
            | AppEvent::FetchUserTimeline { .. }
            | AppEvent::FetchTweet { .. }
            | AppEvent::FetchThread { .. }
            | AppEvent::FetchUser { .. }
            | AppEvent::FetchSearch { .. }
            | AppEvent::FetchMentions { .. }
            | AppEvent::FetchBookmarks { .. }
            | AppEvent::FetchFollowers { .. }
            | AppEvent::FetchFollowing { .. }) => {
                self.loading = true;
                self.dispatch_api_request(evt.clone());
            }

            // API response events
            AppEvent::HomeTimelineLoaded(result) => {
                self.loading = false;
                self.home_timeline.loading = false;
                match result {
                    Ok(resp) => {
                        self.cache_users_from_includes(&resp.includes);
                        self.home_timeline.next_token =
                            resp.meta.as_ref().and_then(|m| m.next_token.clone());
                        self.home_timeline.includes = resp.includes;
                        self.home_timeline
                            .tweets
                            .extend(resp.data.unwrap_or_default());
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading timeline: {e}"));
                    }
                }
            }
            AppEvent::UserTimelineLoaded { user_id: _, result } => {
                self.loading = false;
                self.viewed_user_timeline.loading = false;
                match result {
                    Ok(resp) => {
                        self.cache_users_from_includes(&resp.includes);
                        self.viewed_user_timeline.next_token =
                            resp.meta.as_ref().and_then(|m| m.next_token.clone());
                        self.viewed_user_timeline.includes = resp.includes;
                        self.viewed_user_timeline
                            .tweets
                            .extend(resp.data.unwrap_or_default());
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading user timeline: {e}"));
                    }
                }
            }
            AppEvent::TweetLoaded(result) => {
                self.loading = false;
                match *result {
                    Ok(resp) => {
                        self.cache_users_from_includes(&resp.includes);
                        if let Some(tweet) = resp.data {
                            let conv_id = tweet
                                .conversation_id
                                .clone()
                                .unwrap_or_else(|| tweet.id.clone());
                            self.thread_root = Some(tweet);
                            self.events.send(AppEvent::FetchThread {
                                conversation_id: conv_id.clone(),
                                pagination_token: None,
                            });
                            self.push_view(ViewKind::Thread(conv_id));
                        } else {
                            self.status_message = Some("Tweet not found".to_string());
                        }
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading tweet: {e}"));
                    }
                }
            }
            AppEvent::ThreadLoaded {
                conversation_id,
                result,
            } => {
                self.loading = false;
                match result {
                    Ok(resp) => {
                        self.cache_users_from_includes(&resp.includes);
                        self.thread_tweets = resp.data.unwrap_or_default();
                        // Push the thread view if not already on it.
                        if self.current_view() != Some(&ViewKind::Thread(conversation_id.clone())) {
                            self.push_view(ViewKind::Thread(conversation_id));
                        }
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading thread: {e}"));
                    }
                }
            }
            AppEvent::UserLoaded(result) => {
                self.loading = false;
                match result {
                    Ok(resp) => {
                        if let Some(user) = resp.data {
                            let username = user.username.clone();
                            self.viewed_user = Some(user);
                            self.viewed_user_timeline = TimelineState::default();
                            self.push_view(ViewKind::UserProfile(username));
                        } else {
                            self.status_message = Some("User not found".to_string());
                        }
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading user: {e}"));
                    }
                }
            }
            AppEvent::SearchLoaded { query, result } => {
                self.loading = false;
                self.search_results.loading = false;
                match result {
                    Ok(resp) => {
                        self.cache_users_from_includes(&resp.includes);
                        self.search_results.next_token =
                            resp.meta.as_ref().and_then(|m| m.next_token.clone());
                        self.search_results.includes = resp.includes;
                        let tweets = resp.data.unwrap_or_default();
                        self.search_results.tweets = tweets.clone();

                        // If OpenRouter + model configured, trigger semantic re-ranking.
                        if self.openrouter_client.is_some()
                            && self.selected_embedding_model.is_some()
                            && !tweets.is_empty()
                        {
                            self.events
                                .send(AppEvent::EmbedAndRankSearch { query, tweets });
                        }
                    }
                    Err(e) => {
                        self.set_error(format!("Error searching: {e}"));
                    }
                }
            }
            AppEvent::MentionsLoaded(result) => {
                self.loading = false;
                self.mentions.loading = false;
                match result {
                    Ok(resp) => {
                        self.cache_users_from_includes(&resp.includes);
                        self.mentions.next_token =
                            resp.meta.as_ref().and_then(|m| m.next_token.clone());
                        self.mentions.includes = resp.includes;
                        self.mentions.tweets.extend(resp.data.unwrap_or_default());
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading mentions: {e}"));
                    }
                }
            }
            AppEvent::BookmarksLoaded(result) => {
                self.loading = false;
                self.bookmarks.loading = false;
                match result {
                    Ok(resp) => {
                        self.cache_users_from_includes(&resp.includes);
                        self.bookmarks.next_token =
                            resp.meta.as_ref().and_then(|m| m.next_token.clone());
                        self.bookmarks.includes = resp.includes;
                        self.bookmarks.tweets.extend(resp.data.unwrap_or_default());
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading bookmarks: {e}"));
                    }
                }
            }
            AppEvent::FollowersLoaded { user_id: _, result } => {
                self.loading = false;
                match result {
                    Ok(resp) => {
                        self.followers = resp.data.unwrap_or_default();
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading followers: {e}"));
                    }
                }
            }
            AppEvent::FollowingLoaded { user_id: _, result } => {
                self.loading = false;
                match result {
                    Ok(resp) => {
                        self.following = resp.data.unwrap_or_default();
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading following: {e}"));
                    }
                }
            }

            // Auth (StartAuth is handled in run() before reaching here)
            AppEvent::StartAuth => unreachable!("StartAuth intercepted in run()"),
            AppEvent::AuthCompleted(result) => match result {
                Ok(user_id) => {
                    self.status_message = Some(format!("Authenticated as {user_id}"));
                }
                Err(e) => {
                    self.set_error(format!("Auth failed: {e}"));
                }
            },

            // OpenRouter auth (intercepted in run() before reaching here)
            AppEvent::StartOpenRouterAuth => {
                unreachable!("StartOpenRouterAuth intercepted in run()")
            }

            // OpenRouter models
            AppEvent::FetchOpenRouterModels => {
                self.models_loading = true;
                self.dispatch_openrouter_models();
            }
            AppEvent::OpenRouterModelsLoaded(result) => {
                self.models_loading = false;
                match result {
                    Ok(models) => {
                        self.openrouter_models = models;
                        self.status_message = Some(format!(
                            "Loaded {} embedding models",
                            self.openrouter_models.len()
                        ));
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading models: {e}"));
                    }
                }
            }
            AppEvent::SelectEmbeddingModel { model_id } => {
                self.selected_embedding_model = Some(model_id.clone());
                self.status_message = Some(format!("Selected model: {model_id}"));
                self.pop_view();
            }

            // Embeddings: semantic search re-ranking
            AppEvent::EmbedAndRankSearch { query, tweets } => {
                self.loading = true;
                self.dispatch_embed_and_rank(query, tweets);
            }
            AppEvent::SearchRanked { query: _, result } => {
                self.loading = false;
                match result {
                    Ok(ranked) => {
                        let tweets: Vec<Tweet> = ranked.into_iter().map(|(t, _)| t).collect();
                        self.search_results.tweets = tweets;
                        self.status_message =
                            Some("Search results re-ranked by semantic similarity".into());
                    }
                    Err(e) => {
                        self.set_error(format!("Ranking error: {e}"));
                    }
                }
            }

            // Clustering
            AppEvent::ClusterTimeline => {
                if self.home_timeline.tweets.is_empty() {
                    self.status_message =
                        Some("No tweets to cluster. Load home timeline first.".into());
                    return;
                }
                self.cluster_loading = true;
                self.selected_cluster = None;
                self.push_view(ViewKind::Cluster);
                self.dispatch_cluster_timeline();
            }
            AppEvent::ClusteringComplete(result) => {
                self.cluster_loading = false;
                match result {
                    Ok(cluster_result) => {
                        self.cluster_result = Some(cluster_result);
                        self.status_message = Some("Clustering complete!".into());
                    }
                    Err(e) => {
                        self.set_error(format!("Clustering error: {e}"));
                    }
                }
            }
        }
    }

    // -- API dispatch -------------------------------------------------------

    fn dispatch_api_request(&self, event: AppEvent) {
        let Some(ref client) = self.api_client else {
            // No API client configured -- nothing to dispatch.
            return;
        };
        let client = Arc::clone(client);
        let sender = self.events.sender();
        let max_results = self.config.default_max_results;

        tokio::spawn(async move {
            match event {
                AppEvent::FetchHomeTimeline { pagination_token } => {
                    let mut api = client.lock().await;
                    let result = api
                        .get_home_timeline(max_results, pagination_token.as_deref())
                        .await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::HomeTimelineLoaded(mapped))));
                }
                AppEvent::FetchUserTimeline {
                    user_id,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .get_timeline(&user_id, max_results, pagination_token.as_deref())
                        .await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::UserTimelineLoaded {
                        user_id,
                        result: mapped,
                    })));
                }
                AppEvent::FetchTweet { tweet_id } => {
                    let api = client.lock().await;
                    let result = api.get_tweet(&tweet_id).await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::TweetLoaded(Box::new(
                        mapped,
                    )))));
                }
                AppEvent::FetchThread {
                    conversation_id,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .get_conversation_thread(
                            &conversation_id,
                            max_results,
                            pagination_token.as_deref(),
                        )
                        .await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::ThreadLoaded {
                        conversation_id,
                        result: mapped,
                    })));
                }
                AppEvent::FetchUser { username } => {
                    let api = client.lock().await;
                    let result = api.get_user(&username).await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::UserLoaded(mapped))));
                }
                AppEvent::FetchSearch {
                    query,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .search_tweets(&query, max_results, pagination_token.as_deref())
                        .await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::SearchLoaded {
                        query,
                        result: mapped,
                    })));
                }
                AppEvent::FetchMentions { pagination_token } => {
                    let mut api = client.lock().await;
                    let result = api
                        .get_mentions(max_results, pagination_token.as_deref())
                        .await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::MentionsLoaded(mapped))));
                }
                AppEvent::FetchBookmarks { pagination_token } => {
                    let mut api = client.lock().await;
                    let result = api
                        .get_bookmarks(max_results, pagination_token.as_deref())
                        .await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::BookmarksLoaded(mapped))));
                }
                AppEvent::FetchFollowers {
                    user_id,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .get_followers(&user_id, max_results, pagination_token.as_deref())
                        .await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::FollowersLoaded {
                        user_id,
                        result: mapped,
                    })));
                }
                AppEvent::FetchFollowing {
                    user_id,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .get_following(&user_id, max_results, pagination_token.as_deref())
                        .await;
                    let mapped: ApiResult<_> = result.map_err(|e| Arc::new(e.to_string()));
                    let _ = sender.send(Event::App(Box::new(AppEvent::FollowingLoaded {
                        user_id,
                        result: mapped,
                    })));
                }
                _ => {
                    // Not an API request event -- ignore.
                }
            }
        });
    }

    // -- Helpers ------------------------------------------------------------

    fn fetch_for_view(&mut self, kind: &ViewKind) {
        match kind {
            ViewKind::Home if self.home_timeline.tweets.is_empty() => {
                self.events.send(AppEvent::FetchHomeTimeline {
                    pagination_token: None,
                });
            }
            ViewKind::Mentions if self.mentions.tweets.is_empty() => {
                self.events.send(AppEvent::FetchMentions {
                    pagination_token: None,
                });
            }
            ViewKind::Bookmarks if self.bookmarks.tweets.is_empty() => {
                self.events.send(AppEvent::FetchBookmarks {
                    pagination_token: None,
                });
            }
            _ => {}
        }
    }

    fn cache_users_from_includes(&mut self, includes: &Option<Includes>) {
        if let Some(inc) = includes
            && let Some(users) = &inc.users
        {
            for user in users {
                self.users_cache.insert(user.id.clone(), user.clone());
            }
        }
    }

    /// Look up a user by their ID from the includes cache.
    pub fn lookup_user(&self, user_id: &str) -> Option<&User> {
        self.users_cache.get(user_id)
    }
}
