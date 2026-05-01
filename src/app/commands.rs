use super::App;
use super::dispatch::ChatProviderKind;
use crate::command::{self, Command};
use crate::event::{AppEvent, ViewKind};

impl App {
    // -- Command execution --------------------------------------------------

    pub(super) fn execute_command(&mut self) {
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
            Some(Command::Embeddings) => {
                self.model_filter = None;
                self.model_filter_open = false;
                self.model_search.clear();
                self.model_search_active = false;
                self.model_filter_search.clear();
                self.model_filter_search_active = false;
                self.events.send(AppEvent::FetchOpenRouterModels);
                self.events
                    .send(AppEvent::PushView(ViewKind::OpenRouterModels));
            }
            Some(Command::OpenRouter) => {
                self.model_filter = None;
                self.model_filter_open = false;
                self.model_search.clear();
                self.model_search_active = false;
                self.model_filter_search.clear();
                self.model_filter_search_active = false;
                self.events.send(AppEvent::FetchTextModels);
                self.events.send(AppEvent::PushView(ViewKind::TextModels));
            }
            Some(Command::HuggingFaceModels) => {
                self.events.send(AppEvent::FetchHuggingFaceModels);
                self.events
                    .send(AppEvent::PushView(ViewKind::HuggingFaceModels));
            }
            Some(Command::Cluster) => {
                self.events.send(AppEvent::ClusterTimeline);
            }
            Some(Command::Topics) => {
                self.events.send(AppEvent::GenerateClusterTopics);
            }
            Some(Command::Provider(arg)) => match arg.as_deref() {
                Some("mlx") => {
                    self.preferred_chat_provider = Some(ChatProviderKind::Mlx);
                    // Always re-probe — the result arrives asynchronously via
                    // MLXCapabilitiesProbed, which updates flags and shows status.
                    self.events.send(AppEvent::ProbeMLXCapabilities);
                    self.status_message = Some("Preferred provider: MLX. Probing server...".into());
                }
                Some("openrouter" | "or") => {
                    self.preferred_chat_provider = Some(ChatProviderKind::OpenRouter);
                    if self.has_chat_provider() {
                        let model = self
                            .resolved_chat_model()
                            .unwrap_or_else(|| "(none selected)".into());
                        self.status_message =
                            Some(format!("Chat provider set to OpenRouter: {model}"));
                    } else {
                        self.status_message = Some(
                            "OpenRouter chat not available. Use :openrouter-auth \
                                 and :openrouter-models first."
                                .into(),
                        );
                    }
                }
                Some("auto") => {
                    self.preferred_chat_provider = None;
                    // Re-probe in case MLX server started after the TUI.
                    self.events.send(AppEvent::ProbeMLXCapabilities);
                    self.status_message =
                        Some("Preferred provider: auto. Probing MLX server...".into());
                }
                _ => {
                    let current = self.resolved_chat_provider_name().unwrap_or("none");
                    self.status_message = Some(format!(
                        "Active: {current}. Usage: :provider <mlx|openrouter|auto>"
                    ));
                }
            },
            Some(Command::Refresh) => {
                self.events.send(AppEvent::RefreshView);
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
}
