use std::sync::Arc;

use super::App;
use crate::api::types::{Includes, Tweet, User};
use crate::event::{ApiResult, AppEvent, Event, ViewKind};
use crate::openrouter;
use crate::openrouter::types::Model;

impl App {
    // -- OpenRouter dispatch methods ------------------------------------------

    pub(super) fn dispatch_openrouter_models(&self) {
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

    pub(super) fn dispatch_embed_and_rank(&self, query: String, tweets: Vec<Tweet>) {
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
                model_id: model,
                result,
            })));
        });
    }

    pub(super) fn dispatch_cluster_timeline(&self) {
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
                let author_ids: Vec<Option<String>> =
                    tweets.iter().map(|t| t.author_id.clone()).collect();

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
                    author_ids,
                    k,
                );

                Ok(cluster_result)
            }
            .await;

            let _ = sender.send(Event::App(Box::new(AppEvent::ClusteringComplete(result))));
        });
    }

    pub(super) fn dispatch_text_models(&self) {
        let Some(ref client) = self.openrouter_client else {
            self.events.send(AppEvent::TextModelsLoaded(Err(Arc::new(
                "OpenRouter not configured. Use :openrouter-auth first.".into(),
            ))));
            return;
        };
        let client = Arc::clone(client);
        let sender = self.events.sender();

        tokio::spawn(async move {
            let result: Result<crate::openrouter::types::ModelsResponse, _> =
                client.get("/models").await;
            let mapped: ApiResult<Vec<Model>> = result
                .map(|r| {
                    r.data
                        .into_iter()
                        .filter(|m| {
                            // Include any model that produces text output
                            // (text->text, text+image->text, etc.)
                            m.architecture
                                .as_ref()
                                .and_then(|a| a.modality.as_deref())
                                .is_some_and(|modality| modality.contains("->text"))
                        })
                        .collect()
                })
                .map_err(|e| Arc::new(e.to_string()));
            let _ = sender.send(Event::App(Box::new(AppEvent::TextModelsLoaded(mapped))));
        });
    }

    pub(super) fn dispatch_generate_cluster_topics(&self) {
        let Some(ref or_client) = self.openrouter_client else {
            self.events
                .send(AppEvent::ClusterTopicsGenerated(Err(Arc::new(
                    "OpenRouter not configured.".into(),
                ))));
            return;
        };
        let Some(ref model_id) = self.selected_chat_model else {
            self.events
                .send(AppEvent::ClusterTopicsGenerated(Err(Arc::new(
                    "No chat model selected. Use :text-models first.".into(),
                ))));
            return;
        };
        let Some(ref result) = self.cluster_result else {
            self.events
                .send(AppEvent::ClusterTopicsGenerated(Err(Arc::new(
                    "No cluster result. Use :cluster first.".into(),
                ))));
            return;
        };

        let client = Arc::clone(or_client);
        let model = model_id.clone();
        let sender = self.events.sender();

        // Build the prompt: collect up to 8 representative tweets per cluster.
        let num_clusters = result.num_clusters();
        let mut user_content = String::new();
        for c in 0..num_clusters {
            user_content.push_str(&format!("## Cluster {c}\n"));
            let texts = result.texts_for_cluster(c);
            for (_, text) in texts.iter().take(8) {
                let truncated: String = text.chars().take(280).collect();
                let cleaned = truncated.replace('\n', " ");
                user_content.push_str(&format!("- {cleaned}\n"));
            }
            user_content.push('\n');
        }

        use crate::openrouter::types::ChatMessage;
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: "For each cluster of tweets, analyze a common topic, generate a short \
                          descriptive topic label (3-5 words). Reply with one label per line, \
                          in order, with no numbering or extra text."
                    .into(),
            },
            ChatMessage {
                role: "user".into(),
                content: user_content,
            },
        ];

        tokio::spawn(async move {
            let result = async {
                // Exclude reasoning tokens -- we only need the final labels.
                // Use a generous max_tokens because reasoning models
                // allocate tokens to internal chain-of-thought first;
                // 256 would be exhausted before any content is produced.
                use crate::openrouter::types::ReasoningConfig;
                let resp = client
                    .chat_completion(
                        &model,
                        messages,
                        Some(16384),
                        Some(0.3),
                        Some(ReasoningConfig { exclude: true }),
                    )
                    .await
                    .map_err(|e| Arc::new(e.to_string()))?;

                let choice = resp
                    .choices
                    .first()
                    .ok_or_else(|| Arc::new("Chat model returned no choices".to_string()))?;

                // Only use `content` -- reasoning/reasoning_content are
                // chain-of-thought fields and must NOT be treated as output.
                let raw = choice.message.content.clone().ok_or_else(|| {
                    let reason = choice.finish_reason.as_deref().unwrap_or("unknown");
                    if reason == "length" {
                        Arc::new(
                            "Model exhausted token budget on reasoning \
                             before producing content (finish_reason: length)"
                                .to_string(),
                        )
                    } else {
                        Arc::new(format!(
                            "Chat model returned null content \
                             (finish_reason: {reason})"
                        ))
                    }
                })?;

                // Strip <think>...</think> blocks that reasoning models
                // may embed in content.
                let content = openrouter::strip_think_tags(&raw);

                if content.trim().is_empty() {
                    return Err(Arc::new(
                        "Chat model returned empty content \
                         (after stripping reasoning tags)"
                            .to_string(),
                    ));
                }

                // Parse labels: filter out lines that are too long to be
                // a 3-5 word topic label (reasoning leakage, explanations).
                let labels: Vec<String> = content
                    .lines()
                    .map(|l| {
                        // Strip leading numbering like "1.", "1)", "- "
                        let t = l.trim();
                        let t = t.strip_prefix("- ").unwrap_or(t);
                        let t = t.trim_start_matches(|c: char| {
                            c.is_ascii_digit() || c == '.' || c == ')'
                        });
                        t.trim().to_string()
                    })
                    .filter(|l| !l.is_empty() && l.len() <= 80)
                    .collect();

                if labels.is_empty() {
                    return Err(Arc::new(format!(
                        "No labels parsed from response: {content}"
                    )));
                }

                // If the model returned more lines than clusters
                // (e.g. prefaced with explanatory text), take the
                // last N lines which are most likely the actual labels.
                let labels = if labels.len() > num_clusters {
                    labels[labels.len() - num_clusters..].to_vec()
                } else {
                    labels
                };

                Ok(labels)
            }
            .await;

            let _ = sender.send(Event::App(Box::new(AppEvent::ClusterTopicsGenerated(
                result,
            ))));
        });
    }

    // -- API dispatch -------------------------------------------------------

    pub(super) fn dispatch_api_request(&self, event: AppEvent) {
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

    pub(super) fn fetch_for_view(&mut self, kind: &ViewKind) {
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

    pub(super) fn cache_users_from_includes(&mut self, includes: &Option<Includes>) {
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
