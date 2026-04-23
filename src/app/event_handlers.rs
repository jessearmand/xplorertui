use super::{App, ClusterSource, TimelineState};
use crate::api::types::Tweet;
use crate::event::{AppEvent, ViewKind};

impl App {
    // -- App event handling -------------------------------------------------

    pub(super) fn handle_app_event(&mut self, event: AppEvent) {
        self.handle_app_event_inner(event);
        self.check_loading_finished();
    }

    fn handle_app_event_inner(&mut self, event: AppEvent) {
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
            AppEvent::RefreshView => {
                self.refresh_current_view();
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
                self.mark_loading_started();
                // Set per-timeline loading flags so UI widgets know to show
                // skeleton / loading indicators.
                match evt {
                    AppEvent::FetchHomeTimeline { .. } => self.home_timeline.loading = true,
                    AppEvent::FetchMentions { .. } => self.mentions.loading = true,
                    AppEvent::FetchBookmarks { .. } => self.bookmarks.loading = true,
                    AppEvent::FetchSearch { .. } => self.search_results.loading = true,
                    AppEvent::FetchUserTimeline { .. } => {
                        self.viewed_user_timeline.loading = true;
                    }
                    _ => {}
                }
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
                if self.refresh_then_cluster && self.cluster_source == Some(ClusterSource::Home) {
                    self.refresh_then_cluster = false;
                    self.start_cluster(ClusterSource::Home);
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

                        // If any embedding provider is available, trigger semantic re-ranking.
                        if self.has_embed_provider() && !tweets.is_empty() {
                            self.events
                                .send(AppEvent::EmbedAndRankSearch { query, tweets });
                        }
                    }
                    Err(e) => {
                        self.set_error(format!("Error searching: {e}"));
                    }
                }
                if self.refresh_then_cluster && self.cluster_source == Some(ClusterSource::Search) {
                    self.refresh_then_cluster = false;
                    self.start_cluster(ClusterSource::Search);
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
                if self.refresh_then_cluster && self.cluster_source == Some(ClusterSource::Mentions)
                {
                    self.refresh_then_cluster = false;
                    self.start_cluster(ClusterSource::Mentions);
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
                if self.refresh_then_cluster
                    && self.cluster_source == Some(ClusterSource::Bookmarks)
                {
                    self.refresh_then_cluster = false;
                    self.start_cluster(ClusterSource::Bookmarks);
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
                self.mark_loading_started();
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
                self.mark_loading_started();
                self.dispatch_embed_and_rank(query, tweets);
            }
            AppEvent::SearchRanked {
                query,
                model_id,
                result,
            } => {
                self.loading = false;
                // Guard: only apply if the query and model still match current state.
                let query_matches = self.search_query == query;
                let model_matches =
                    self.resolved_embed_model().as_deref() == Some(model_id.as_str());
                if !query_matches || !model_matches {
                    self.status_message =
                        Some("Stale ranking result discarded (query or model changed)".into());
                    return;
                }
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
                // Resolve source from the current view. If invoked from within
                // the Cluster view itself (re-cluster), reuse the previously
                // stored source so the user doesn't lose their context.
                let source = match self.current_view() {
                    Some(ViewKind::Cluster) => self.cluster_source,
                    Some(view) => ClusterSource::from_view(view),
                    None => None,
                };
                let Some(source) = source else {
                    self.status_message = Some(
                        "Cluster only supported from Home, Mentions, Search, or Bookmarks views."
                            .into(),
                    );
                    return;
                };
                self.start_cluster(source);
            }
            AppEvent::ClusteringComplete(result) => {
                self.cluster_loading = false;
                match result {
                    Ok(cluster_result) => {
                        self.cluster_generation += 1;
                        self.cluster_result = Some(cluster_result);
                        self.status_message = Some("Clustering complete!".into());
                        // Auto-trigger LLM topic generation if a chat provider is available.
                        if self.has_chat_provider() {
                            self.cluster_topics_loading = true;
                            self.dispatch_generate_cluster_topics();
                        }
                    }
                    Err(e) => {
                        self.set_error(format!("Clustering error: {e}"));
                    }
                }
            }

            // Text models (for chat/topic generation)
            AppEvent::FetchTextModels => {
                self.text_models_loading = true;
                self.mark_loading_started();
                self.dispatch_text_models();
            }
            AppEvent::TextModelsLoaded(result) => {
                self.text_models_loading = false;
                match result {
                    Ok(models) => {
                        self.status_message = Some(format!("Loaded {} text models", models.len()));
                        self.text_models = models;
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading text models: {e}"));
                    }
                }
            }
            AppEvent::SelectChatModel { model_id } => {
                self.selected_chat_model = Some(model_id.clone());
                self.status_message = Some(format!("Selected chat model: {model_id}"));
                self.pop_view();
            }

            // MLX capability probe
            AppEvent::ProbeMLXCapabilities => {
                self.dispatch_probe_mlx();
            }
            AppEvent::MLXCapabilitiesProbed { embed, chat } => {
                self.mlx_embed_supported = embed;
                self.mlx_chat_supported = chat;

                if embed || chat {
                    let mut caps = Vec::new();
                    if embed {
                        caps.push("embeddings");
                    }
                    if chat {
                        caps.push("chat");
                    }
                    self.status_message = Some(format!("MLX server detected: {}", caps.join(", ")));
                } else {
                    self.status_message = Some("MLX server not reachable.".into());
                }
            }

            // HuggingFace Hub models
            AppEvent::FetchHuggingFaceModels => {
                self.hf_models_loading = true;
                self.mark_loading_started();
                self.dispatch_hf_models();
            }
            AppEvent::HuggingFaceModelsLoaded { query, result } => {
                // Discard stale responses from a previous search.
                if query != self.hf_search {
                    return;
                }
                self.hf_models_loading = false;
                match result {
                    Ok(models) => {
                        self.status_message =
                            Some(format!("Loaded {} HuggingFace models", models.len()));
                        self.hf_models = models;
                    }
                    Err(e) => {
                        self.set_error(format!("Error loading HF models: {e}"));
                    }
                }
            }

            // LLM cluster topic generation
            AppEvent::GenerateClusterTopics => {
                if self.cluster_result.is_none() {
                    self.status_message = Some("No cluster result. Use :cluster first.".into());
                    return;
                }
                if !self.has_chat_provider() {
                    self.status_message = Some(
                        "No chat provider configured. Set mlx_server_url in config \
                         or use :openrouter-auth + :text-models."
                            .into(),
                    );
                    return;
                }
                self.cluster_generation += 1;
                self.cluster_topics_loading = true;
                self.dispatch_generate_cluster_topics();
            }
            AppEvent::ClusterTopicsGenerated(request_generation, result) => {
                // Discard stale responses from a previous cluster/generation.
                if request_generation != self.cluster_generation {
                    return;
                }
                self.cluster_topics_loading = false;
                match result {
                    Ok(labels) => {
                        if let Some(ref mut cr) = self.cluster_result {
                            let cluster_count = cr.cluster_topics.len();
                            let mut applied = 0usize;
                            for (i, label) in labels.into_iter().enumerate() {
                                if i < cluster_count && !label.is_empty() {
                                    cr.cluster_topics[i] = label;
                                    applied += 1;
                                }
                            }
                            let provider = self.resolved_chat_provider_name().unwrap_or("LLM");
                            self.status_message = Some(format!(
                                "{provider} generated {applied}/{cluster_count} topic labels"
                            ));
                        }
                    }
                    Err(e) => {
                        self.set_error(format!("Topic generation error: {e}"));
                    }
                }
            }
        }
    }

    /// Kick off a clustering run for the given source. Shared by `:cluster`
    /// (view-resolved source) and the refresh-then-cluster gates (stored
    /// source), so both paths converge on the same setup.
    ///
    /// Always clears `refresh_then_cluster` so a fresh `:cluster` invocation
    /// cancels any in-flight refresh-cluster promise left over from an earlier
    /// `R` in the Cluster view. Without this, a user could press `R` on
    /// Cluster-Mentions, navigate away before the response arrives, run
    /// `:cluster` on a different source, and later get yanked back into the
    /// Cluster view when an unrelated fetch for the new source (pagination,
    /// manual refresh, etc.) completes. Clearing here is a no-op for the
    /// refresh-gate callers — they already clear the flag before calling.
    fn start_cluster(&mut self, source: ClusterSource) {
        self.refresh_then_cluster = false;
        let tweets_empty = match source {
            ClusterSource::Home => self.home_timeline.tweets.is_empty(),
            ClusterSource::Mentions => self.mentions.tweets.is_empty(),
            ClusterSource::Search => self.search_results.tweets.is_empty(),
            ClusterSource::Bookmarks => self.bookmarks.tweets.is_empty(),
        };
        if tweets_empty {
            self.status_message = Some(format!("No tweets to cluster in {source}. Load it first."));
            return;
        }
        self.cluster_source = Some(source);
        self.cluster_loading = true;
        self.mark_loading_started();
        self.selected_cluster = None;
        if self.current_view() != Some(&ViewKind::Cluster) {
            self.push_view(ViewKind::Cluster);
        }
        self.dispatch_cluster_timeline();
    }
}
