use super::{App, ClusterSource, TimelineState, tweet_url};
use crate::api::types::Tweet;
use crate::event::{AppEvent, ViewKind};
use crate::openrouter;
use crate::openrouter::types::Model;

impl App {
    // -- Selection helpers --------------------------------------------------

    pub(super) fn move_selection_down(&mut self) {
        let count = self.current_item_count();
        if let Some(vs) = self.view_stack.last_mut()
            && vs.selected_index + 1 < count
        {
            vs.selected_index += 1;
        }
    }

    pub(super) fn move_selection_up(&mut self) {
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
            Some(ViewKind::OpenRouterModels) | Some(ViewKind::TextModels) => {
                self.filtered_model_list().len()
            }
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
            Some(ViewKind::HuggingFaceModels) => self.filtered_hf_models().len(),
            Some(ViewKind::Help) => 0,
            None => 0,
        }
    }

    pub fn selected_index(&self) -> usize {
        self.view_stack.last().map_or(0, |vs| vs.selected_index)
    }

    /// Returns the model list for the current model view.
    pub fn current_model_list(&self) -> &[Model] {
        if self.current_view() == Some(&ViewKind::TextModels) {
            &self.text_models
        } else {
            &self.openrouter_models
        }
    }

    /// Returns models for the current model view, filtered by provider and search text, sorted.
    pub fn filtered_model_list(&self) -> Vec<&Model> {
        let search = self.model_search.to_lowercase();
        let mut filtered: Vec<&Model> = self
            .current_model_list()
            .iter()
            .filter(|m| match &self.model_filter {
                Some(provider) => openrouter::extract_provider(&m.id) == provider.as_str(),
                None => true,
            })
            .filter(|m| {
                search.is_empty()
                    || m.id.to_lowercase().contains(&search)
                    || m.name
                        .as_deref()
                        .is_some_and(|n| n.to_lowercase().contains(&search))
            })
            .collect();

        filtered.sort_by(|a, b| {
            let pa = openrouter::extract_provider(&a.id);
            let pb = openrouter::extract_provider(&b.id);
            pa.cmp(pb).then_with(|| a.id.cmp(&b.id))
        });

        filtered
    }

    /// Returns unique provider names from the current model list, sorted.
    pub fn model_providers(&self) -> Vec<String> {
        let mut providers: Vec<String> = self
            .current_model_list()
            .iter()
            .map(|m| openrouter::extract_provider(&m.id).to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        providers.sort();
        providers
    }

    /// Returns providers filtered by `model_filter_search` (case-insensitive substring).
    pub fn filtered_model_providers(&self) -> Vec<String> {
        let query = self.model_filter_search.to_lowercase();
        self.model_providers()
            .into_iter()
            .filter(|p| query.is_empty() || p.to_lowercase().contains(&query))
            .collect()
    }

    // -- HuggingFace model helpers ------------------------------------------

    /// Returns HF models filtered by the active org filter, sorted by org then ID.
    pub fn filtered_hf_models(&self) -> Vec<&crate::huggingface::types::HfModel> {
        let mut filtered: Vec<_> = self
            .hf_models
            .iter()
            .filter(|m| match &self.hf_org_filter {
                Some(org) => m.org() == org.as_str(),
                None => true,
            })
            .collect();
        filtered.sort_by(|a, b| a.org().cmp(b.org()).then_with(|| a.id.cmp(&b.id)));
        filtered
    }

    /// Returns unique orgs from the current HF models list.
    pub fn hf_orgs(&self) -> Vec<String> {
        let mut orgs: Vec<String> = self.hf_models.iter().map(|m| m.org().to_string()).collect();
        orgs.sort();
        orgs.dedup();
        orgs
    }

    /// Open the thread of `tweet`, with `tweet` at the top. X's conversation
    /// search does not return a conversation's first post, and lists replies
    /// newest first, so the opened post is kept as the root instead of being
    /// looked for among the replies.
    ///
    /// A retweet has no thread of its own and carries the original's text
    /// cut short, so it opens the original post instead.
    pub(super) fn open_thread(&mut self, tweet: Tweet) {
        if let Some(original) = retweeted_id(&tweet) {
            self.events.send(AppEvent::FetchTweet {
                tweet_id: original.to_string(),
            });
            return;
        }
        let conversation_id = tweet
            .conversation_id
            .clone()
            .unwrap_or_else(|| tweet.id.clone());
        self.thread_root = Some(tweet);
        self.thread_tweets.clear();
        self.photo_index = 0;
        self.events.send(AppEvent::FetchThread {
            conversation_id,
            pagination_token: None,
        });
    }
}

/// ID of the post a retweet repeats, if `tweet` is a retweet.
fn retweeted_id(tweet: &Tweet) -> Option<&str> {
    tweet
        .referenced_tweets
        .iter()
        .flatten()
        .find(|r| r.type_ == "retweeted")
        .map(|r| r.id.as_str())
}

impl App {
    /// A tweet from any loaded timeline, by ID.
    fn find_loaded_tweet(&self, tweet_id: &str) -> Option<&Tweet> {
        [
            &self.home_timeline,
            &self.mentions,
            &self.bookmarks,
            &self.search_results,
            &self.viewed_user_timeline,
        ]
        .into_iter()
        .flat_map(|timeline| timeline.tweets.iter())
        .find(|t| t.id == tweet_id)
    }

    pub(super) fn open_selected(&mut self) {
        let idx = self.selected_index();
        match self.current_view().cloned() {
            Some(ViewKind::Home) => {
                if let Some(tweet) = self.home_timeline.tweets.get(idx).cloned() {
                    self.open_thread(tweet);
                }
            }
            Some(ViewKind::Mentions) => {
                if let Some(tweet) = self.mentions.tweets.get(idx).cloned() {
                    self.open_thread(tweet);
                }
            }
            Some(ViewKind::Bookmarks) => {
                if let Some(tweet) = self.bookmarks.tweets.get(idx).cloned() {
                    self.open_thread(tweet);
                }
            }
            Some(ViewKind::Search) => {
                if let Some(tweet) = self.search_results.tweets.get(idx).cloned() {
                    self.open_thread(tweet);
                }
            }
            Some(ViewKind::UserTimeline(_)) => {
                if let Some(tweet) = self.viewed_user_timeline.tweets.get(idx).cloned() {
                    self.open_thread(tweet);
                }
            }
            Some(ViewKind::OpenRouterModels) => {
                let filtered = self.filtered_model_list();
                if let Some(model) = filtered.get(idx) {
                    let model_id = model.id.clone();
                    self.events
                        .send(AppEvent::SelectEmbeddingModel { model_id });
                }
            }
            Some(ViewKind::TextModels) => {
                let filtered = self.filtered_model_list();
                if let Some(model) = filtered.get(idx) {
                    let model_id = model.id.clone();
                    self.events.send(AppEvent::SelectChatModel { model_id });
                }
            }
            Some(ViewKind::HuggingFaceModels) => {
                let filtered = self.filtered_hf_models();
                if let Some(model) = filtered.get(idx) {
                    if !model.is_chat_capable() {
                        self.status_message = Some(format!(
                            "Model {} is not chat-capable (pipeline: {})",
                            model.id,
                            model.pipeline_tag.as_deref().unwrap_or("unknown"),
                        ));
                        return;
                    }
                    if model.is_discouraged_for_cluster_labels() {
                        self.set_error(format!(
                            "Model {} is a Gemma 4 base checkpoint. For cluster topic labeling, \
                             select an instruction-tuned Gemma 4 `-it` variant instead.",
                            model.id
                        ));
                        return;
                    }
                    let model_id = model.id.clone();
                    self.config.mlx_chat_model = Some(model_id.clone());
                    self.status_message = Some(format!("MLX chat model set to: {model_id}"));
                    self.pop_view();
                }
            }
            Some(ViewKind::Cluster) => {
                if let Some(ref result) = self.cluster_result {
                    if let Some(c) = self.selected_cluster {
                        // In tweet list mode -- open the selected tweet's thread
                        let indices = result.tweet_indices_for_cluster(c);
                        if let Some(&orig_idx) = indices.get(idx) {
                            let tweet_id = result.tweet_ids[orig_idx].clone();
                            match self.find_loaded_tweet(&tweet_id).cloned() {
                                Some(tweet) => self.open_thread(tweet),
                                // Not loaded any more: fetch it, which opens its thread.
                                None => self.events.send(AppEvent::FetchTweet { tweet_id }),
                            }
                        }
                    } else {
                        // In cluster list mode -- enter tweet list for this cluster
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

    /// Returns a reference to the currently selected tweet, if any.
    fn selected_tweet(&self) -> Option<&Tweet> {
        let idx = self.selected_index();
        match self.current_view() {
            Some(ViewKind::Home) => self.home_timeline.tweets.get(idx),
            Some(ViewKind::Mentions) => self.mentions.tweets.get(idx),
            Some(ViewKind::Bookmarks) => self.bookmarks.tweets.get(idx),
            Some(ViewKind::Search) => self.search_results.tweets.get(idx),
            Some(ViewKind::UserTimeline(_)) => self.viewed_user_timeline.tweets.get(idx),
            Some(ViewKind::Thread(_)) => self.thread_tweets.get(idx),
            _ => None,
        }
    }

    /// Builds the tweet URL for the current selection, handling both regular
    /// views (via `selected_tweet()`) and the cluster tweet-list view.
    fn selected_tweet_url(&self) -> Option<String> {
        // Cluster tweet-list view: tweets stored as IDs, not Tweet objects.
        if self.current_view() == Some(&ViewKind::Cluster) {
            if let Some(c) = self.selected_cluster
                && let Some(ref result) = self.cluster_result
            {
                let indices = result.tweet_indices_for_cluster(c);
                let orig_idx = *indices.get(self.selected_index())?;
                let tweet_id = &result.tweet_ids[orig_idx];
                let username = result.author_ids[orig_idx]
                    .as_deref()
                    .and_then(|aid| self.lookup_user(aid))
                    .map(|u| u.username.as_str());
                return Some(tweet_url(tweet_id, username));
            }
            return None;
        }

        let tweet = self.selected_tweet()?;
        let username = tweet
            .author_id
            .as_deref()
            .and_then(|aid| self.lookup_user(aid))
            .map(|u| u.username.as_str());
        Some(tweet_url(&tweet.id, username))
    }

    pub(super) fn copy_tweet_url(&mut self) {
        match self.selected_tweet_url() {
            Some(url) => match crate::clipboard::copy_to_clipboard(&url) {
                Ok(()) => {
                    self.status_message = Some(format!("Copied: {url}"));
                }
                Err(e) => {
                    self.status_message = Some(format!("Clipboard error: {e}"));
                }
            },
            None => {
                self.status_message = Some("No tweet selected".into());
            }
        }
    }

    pub(super) fn open_tweet_url(&mut self) {
        match self.selected_tweet_url() {
            Some(url) => match open::that(&url) {
                Ok(()) => {
                    self.status_message = Some(format!("Opened: {url}"));
                }
                Err(e) => {
                    self.status_message = Some(format!("Failed to open browser: {e}"));
                }
            },
            None => {
                self.status_message = Some("No tweet selected".into());
            }
        }
    }

    pub(super) fn load_next_page(&mut self) {
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

    pub(super) fn refresh_current_view(&mut self) {
        match self.current_view().cloned() {
            Some(ViewKind::Home) => {
                self.reset_timeline(&mut Self::home_timeline_ref);
                self.events.send(AppEvent::FetchHomeTimeline {
                    pagination_token: None,
                });
            }
            Some(ViewKind::Mentions) => {
                self.reset_timeline(&mut Self::mentions_ref);
                self.events.send(AppEvent::FetchMentions {
                    pagination_token: None,
                });
            }
            Some(ViewKind::Bookmarks) => {
                self.reset_timeline(&mut Self::bookmarks_ref);
                self.events.send(AppEvent::FetchBookmarks {
                    pagination_token: None,
                });
            }
            Some(ViewKind::Cluster) => {
                let Some(source) = self.cluster_source else {
                    self.status_message =
                        Some("No cluster source to refresh. Run :cluster again.".into());
                    return;
                };
                self.cluster_result = None;
                self.selected_cluster = None;
                self.refresh_then_cluster = true;
                match source {
                    ClusterSource::Home => {
                        self.reset_timeline(&mut Self::home_timeline_ref);
                        self.events.send(AppEvent::FetchHomeTimeline {
                            pagination_token: None,
                        });
                    }
                    ClusterSource::Mentions => {
                        self.reset_timeline(&mut Self::mentions_ref);
                        self.events.send(AppEvent::FetchMentions {
                            pagination_token: None,
                        });
                    }
                    ClusterSource::Bookmarks => {
                        self.reset_timeline(&mut Self::bookmarks_ref);
                        self.events.send(AppEvent::FetchBookmarks {
                            pagination_token: None,
                        });
                    }
                    ClusterSource::Search => {
                        let query = self.search_query.clone();
                        if query.is_empty() {
                            self.status_message = Some(
                                "No search query to re-run. Go to Search view and try again."
                                    .into(),
                            );
                            self.refresh_then_cluster = false;
                            return;
                        }
                        self.reset_timeline(&mut Self::search_results_ref);
                        self.events.send(AppEvent::FetchSearch {
                            query,
                            pagination_token: None,
                        });
                    }
                }
            }
            _ => {
                self.status_message = Some("Refresh not supported for this view".into());
            }
        }
    }

    /// Clear a timeline and reset the view stack's selection/scroll to the top.
    fn reset_timeline(&mut self, timeline_fn: &mut dyn FnMut(&mut Self) -> &mut TimelineState) {
        let tl = timeline_fn(self);
        tl.tweets.clear();
        tl.next_token = None;
        tl.includes = None;
        tl.selected_index = 0;
        tl.scroll_offset = 0;
        if let Some(vs) = self.view_stack.last_mut() {
            vs.selected_index = 0;
            vs.scroll_offset = 0;
        }
        self.status_message = Some("Refreshing...".into());
    }

    fn home_timeline_ref(&mut self) -> &mut TimelineState {
        &mut self.home_timeline
    }

    fn mentions_ref(&mut self) -> &mut TimelineState {
        &mut self.mentions
    }

    fn bookmarks_ref(&mut self) -> &mut TimelineState {
        &mut self.bookmarks
    }

    fn search_results_ref(&mut self) -> &mut TimelineState {
        &mut self.search_results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::ListResponse;
    use crate::auth::credentials::CredentialSet;
    use crate::config::AppConfig;

    fn tweet(id: &str, conversation_id: &str) -> Tweet {
        serde_json::from_value(serde_json::json!({
            "id": id, "text": format!("post {id}"), "conversation_id": conversation_id,
        }))
        .unwrap()
    }

    fn thread_page(tweets: Vec<Tweet>) -> ListResponse<Tweet> {
        serde_json::from_value(serde_json::json!({ "data": tweets })).unwrap()
    }

    #[tokio::test]
    async fn opening_a_search_result_makes_it_the_thread_root() {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.thread_root = Some(tweet("stale", "stale"));
        app.thread_tweets = vec![tweet("old-reply", "stale")];
        app.search_results.tweets = vec![tweet("1", "1"), tweet("2", "1")];
        app.push_view(ViewKind::Search);
        app.view_stack.last_mut().unwrap().selected_index = 1;

        app.open_selected();

        assert_eq!(app.thread_root.as_ref().map(|t| t.id.as_str()), Some("2"));
        assert!(
            app.thread_tweets.is_empty(),
            "replies of the previous thread are gone"
        );
    }

    #[tokio::test]
    async fn opening_a_retweet_fetches_the_original_post() {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        let mut rt = tweet("rt", "rt");
        rt.referenced_tweets = Some(vec![crate::api::types::ReferencedTweet {
            type_: "retweeted".into(),
            id: "orig".into(),
        }]);
        app.thread_root = Some(tweet("stale", "stale"));
        app.open_thread(rt);
        // The original arrives through `FetchTweet`, which then opens its thread.
        assert_eq!(
            app.thread_root.as_ref().map(|t| t.id.as_str()),
            Some("stale")
        );
        // Skip ticks: the handler interleaves them with app events.
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if let crate::event::Event::App(e) = app.events.next().await.expect("open") {
                    return *e;
                }
            }
        })
        .await
        .expect("an app event");
        match event {
            AppEvent::FetchTweet { tweet_id } => assert_eq!(tweet_id, "orig"),
            other => panic!("expected FetchTweet, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_root_is_not_listed_again_among_the_replies() {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.open_thread(tweet("1", "1"));
        app.handle_app_event(AppEvent::ThreadLoaded {
            conversation_id: "1".into(),
            result: Ok(thread_page(vec![
                tweet("3", "1"),
                tweet("2", "1"),
                tweet("1", "1"),
            ])),
        });
        let ids: Vec<&str> = app.thread_tweets.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["3", "2"]);
        assert_eq!(app.thread_root.as_ref().map(|t| t.id.as_str()), Some("1"));
    }
}
