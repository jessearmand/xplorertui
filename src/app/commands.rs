use super::App;
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
            Some(Command::Models) => {
                self.model_filter = None;
                self.model_filter_open = false;
                self.events.send(AppEvent::FetchOpenRouterModels);
                self.events
                    .send(AppEvent::PushView(ViewKind::OpenRouterModels));
            }
            Some(Command::TextModels) => {
                self.model_filter = None;
                self.model_filter_open = false;
                self.events.send(AppEvent::FetchTextModels);
                self.events.send(AppEvent::PushView(ViewKind::TextModels));
            }
            Some(Command::Cluster) => {
                self.events.send(AppEvent::ClusterTimeline);
            }
            Some(Command::Topics) => {
                self.events.send(AppEvent::GenerateClusterTopics);
            }
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
