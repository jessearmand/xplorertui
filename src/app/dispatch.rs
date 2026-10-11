use std::sync::Arc;

use super::{App, ClusterSource};
use crate::api::types::{Includes, Tweet, User};
use crate::event::{ApiResult, AppEvent, Event, TopicLabels, ViewKind};
use crate::mlx::client::{EmbedTask, MlxClient};
use crate::openrouter;
use crate::openrouter::client::OpenRouterClient;
use crate::openrouter::types::{EmbeddingResponse, Model};

const DEFAULT_MLX_EMBEDDING_MODEL: &str = "mlx-community/Qwen3-Embedding-0.6B-mxfp8";
const DEFAULT_MLX_CHAT_MODEL: &str = "mlx-community/Qwen3.5-0.8B-OptiQ-4bit";

/// Identifies which chat provider the user prefers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatProviderKind {
    Mlx,
    OpenRouter,
}

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
        let embed_provider = self.resolve_embed_provider();
        let Some((provider, model)) = embed_provider else {
            return;
        };
        let sender = self.events.sender();
        let query_clone = query.clone();

        tokio::spawn(async move {
            let result = async {
                let texts: Vec<String> = tweets.iter().map(|t| t.text.clone()).collect();
                let (query_emb, doc_embs) = provider
                    .embed_query_and_documents(&model, &query_clone, &texts)
                    .await?;
                let tweet_embs: Vec<(usize, Vec<f64>)> = doc_embs.into_iter().enumerate().collect();

                let ranked =
                    crate::embeddings::similarity::rank_by_similarity(&query_emb, &tweet_embs);

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
        // Try to resolve an embed provider now; if none is available but
        // an MLX client exists, pass it along so the async task can
        // re-probe the server (it may have started after the TUI).
        let embed_provider = self.resolve_embed_provider();
        let mlx_fallback = if embed_provider.is_none() {
            self.mlx_client.as_ref().map(|mlx| {
                let model = self
                    .config
                    .mlx_embedding_model
                    .clone()
                    .unwrap_or_else(|| DEFAULT_MLX_EMBEDDING_MODEL.to_string());
                (Arc::clone(mlx), model)
            })
        } else {
            None
        };
        let openrouter_fallback = if embed_provider.is_none() {
            self.resolve_openrouter_embed()
        } else {
            None
        };

        let sender = self.events.sender();
        let Some(source) = self.cluster_source else {
            self.events.send(AppEvent::ClusteringComplete(Err(Arc::new(
                "Internal error: cluster_source not set before dispatch.".into(),
            ))));
            return;
        };

        if embed_provider.is_none() && mlx_fallback.is_none() && openrouter_fallback.is_none() {
            self.events.send(AppEvent::EmbeddingProviderMissing(source));
            return;
        }
        let tweets = match source {
            ClusterSource::Home => self.home_timeline.tweets.clone(),
            ClusterSource::Mentions => self.mentions.tweets.clone(),
            ClusterSource::Search => self.search_results.tweets.clone(),
            ClusterSource::Bookmarks => self.bookmarks.tweets.clone(),
        };

        tokio::spawn(async move {
            // If we had a resolved provider, use it. Otherwise try MLX
            // with a live probe, falling back to OpenRouter.
            let (provider, model) = if let Some((p, m)) = embed_provider {
                (p, m)
            } else if let Some((mlx_client, mlx_model)) = mlx_fallback {
                // Re-probe: check if server is now reachable.
                let caps = mlx_client.capabilities().await;
                if caps.iter().any(|c| c == "embeddings") {
                    // Update flags via event so future calls don't need to re-probe.
                    let chat = caps.iter().any(|c| c == "chat");
                    let _ = sender.send(Event::App(Box::new(AppEvent::MLXCapabilitiesProbed {
                        embed: true,
                        chat,
                    })));
                    (EmbedProvider::Mlx(mlx_client), mlx_model)
                } else if let Some((p, m)) = openrouter_fallback {
                    (p, m)
                } else {
                    let _ = sender.send(Event::App(Box::new(AppEvent::EmbeddingProviderMissing(
                        source,
                    ))));
                    return;
                }
            } else if let Some((p, m)) = openrouter_fallback {
                (p, m)
            } else {
                unreachable!("checked above");
            };
            let result = async {
                let texts: Vec<String> = tweets.iter().map(|t| t.text.clone()).collect();
                let ids: Vec<String> = tweets.iter().map(|t| t.id.clone()).collect();
                let conv_ids: Vec<Option<String>> =
                    tweets.iter().map(|t| t.conversation_id.clone()).collect();
                let author_ids: Vec<Option<String>> =
                    tweets.iter().map(|t| t.author_id.clone()).collect();

                let embeddings = provider
                    .embed(&model, &texts, EmbedTask::Clustering)
                    .await?;

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

    pub(super) fn dispatch_probe_mlx(&self) {
        let Some(ref mlx) = self.mlx_client else {
            return;
        };
        let mlx = Arc::clone(mlx);
        let sender = self.events.sender();
        tokio::spawn(async move {
            let caps = mlx.capabilities().await;
            let embed = caps.iter().any(|c| c == "embeddings");
            let chat = caps.iter().any(|c| c == "chat");
            let _ = sender.send(Event::App(Box::new(AppEvent::MLXCapabilitiesProbed {
                embed,
                chat,
            })));
        });
    }

    pub(super) fn dispatch_hf_models(&self) {
        let sender = self.events.sender();
        let search = self.hf_search.clone();

        tokio::spawn(async move {
            let client = crate::huggingface::client::HfHubClient::new();
            let api_query = if search.is_empty() {
                None
            } else {
                Some(search.as_str())
            };
            let result = client
                .search_mlx_models(api_query, 50)
                .await
                .map_err(|e| Arc::new(e.to_string()));
            let _ = sender.send(Event::App(Box::new(AppEvent::HuggingFaceModelsLoaded {
                query: search,
                result,
            })));
        });
    }

    pub(super) fn dispatch_generate_cluster_topics(&self) {
        if let Some((client, jev)) = self.resolve_jev() {
            self.dispatch_jev_cluster_topics(client, jev);
            return;
        }
        let generation = self.cluster_generation;
        let Some((provider, model)) = self.resolve_chat_provider() else {
            self.events.send(AppEvent::ClusterTopicsGenerated(
                generation,
                Err(Arc::new(
                    "No chat provider configured. Set mlx_server_url in config \
                     or use :openrouter-auth + :openrouter-models."
                        .into(),
                )),
            ));
            return;
        };
        let Some(ref result) = self.cluster_result else {
            self.events.send(AppEvent::ClusterTopicsGenerated(
                generation,
                Err(Arc::new("No cluster result. Use :cluster first.".into())),
            ));
            return;
        };

        let num_clusters = result.num_clusters();
        let user_content = cluster_prompt_body(result);
        let sender = self.events.sender();

        tokio::spawn(async move {
            let result = chat_labels(&provider, &model, user_content, num_clusters)
                .await
                .map(|labels| TopicLabels {
                    labels,
                    labeller: provider.name().to_string(),
                });
            let _ = sender.send(Event::App(Box::new(AppEvent::ClusterTopicsGenerated(
                generation, result,
            ))));
        });
    }

    /// Label clusters with Jev's help. With a chat provider available (and
    /// `[jev] propose_labels` on), Jev first asks whether each cluster is
    /// tech. On a mostly tech timeline the chat model proposes candidate
    /// labels and Jev picks a distinct one per cluster; otherwise the chat
    /// model writes the labels itself, which scores higher there. Without a
    /// chat provider, or if those steps fail, Jev picks from the fixed
    /// topic taxonomy. Jev requests go out concurrently.
    fn dispatch_jev_cluster_topics(
        &self,
        client: Arc<OpenRouterClient>,
        jev: crate::config::JevConfig,
    ) {
        use crate::openrouter::decisions;

        let generation = self.cluster_generation;
        let sender = self.events.sender();
        let Some(ref result) = self.cluster_result else {
            self.events.send(AppEvent::ClusterTopicsGenerated(
                generation,
                Err(Arc::new("No cluster result. Use :cluster first.".into())),
            ));
            return;
        };

        let num_clusters = result.num_clusters();
        let cluster_sizes: Vec<usize> = (0..num_clusters)
            .map(|c| result.tweet_indices_for_cluster(c).len())
            .collect();
        let cluster_posts: Vec<Vec<String>> = (0..num_clusters)
            .map(|c| {
                result
                    .texts_for_cluster(c)
                    .into_iter()
                    .take(decisions::MAX_POSTS_PER_CLUSTER)
                    .map(|(_, t)| t.to_string())
                    .collect()
            })
            .collect();
        let chat = self
            .resolve_chat_provider()
            .filter(|_| jev.propose_labels)
            .map(|(provider, model)| (provider, model, cluster_prompt_body(result)));

        tokio::spawn(async move {
            let send = |result: ApiResult<TopicLabels>| {
                let _ = sender.send(Event::App(Box::new(AppEvent::ClusterTopicsGenerated(
                    generation, result,
                ))));
            };
            let posts =
                |c: usize| -> Vec<&str> { cluster_posts[c].iter().map(String::as_str).collect() };

            if let Some((provider, model, body)) = chat {
                let tech_requests = (0..num_clusters)
                    .map(|c| decisions::tech_request(&jev.model, &posts(c)))
                    .collect();
                let tech = client.decide_all(tech_requests, jev.concurrency).await;
                let ok: Vec<_> = tech.iter().map(|r| r.as_ref().ok()).collect();
                let tech_share = decisions::tech_share(&ok, &cluster_sizes);

                if tech_share.is_some_and(|s| s >= decisions::MIN_TECH_SHARE) {
                    match propose_then_pick(&client, &jev, &provider, &model, body, &cluster_posts)
                        .await
                    {
                        Ok(Some(labels)) => {
                            return send(labels.map(|labels| TopicLabels {
                                labels,
                                labeller: format!("{} + Jev", provider.name()),
                            }));
                        }
                        Ok(None) => tracing::warn!(
                            "label proposal did not cover every cluster; using the topic taxonomy"
                        ),
                        Err(e) => {
                            tracing::warn!("label proposal failed ({e}); using the topic taxonomy")
                        }
                    }
                } else {
                    // Non-tech timeline (or Jev couldn't tell): the chat
                    // model's own labels beat Jev's picks there.
                    // Partial labels would leave placeholders; the taxonomy
                    // can still label every cluster.
                    match chat_labels(&provider, &model, body, num_clusters).await {
                        Ok(labels) if labels.iter().all(|l| !l.is_empty()) => {
                            return send(Ok(TopicLabels {
                                labels,
                                labeller: provider.name().to_string(),
                            }));
                        }
                        Ok(_) => tracing::warn!(
                            "chat labels missed some clusters; using the topic taxonomy"
                        ),
                        Err(e) => {
                            tracing::warn!("chat labels failed ({e}); using the topic taxonomy")
                        }
                    }
                }
            }

            let topics = jev.topics();
            let requests = (0..num_clusters)
                .map(|c| decisions::cluster_topic_request(&jev.model, &posts(c), &topics))
                .collect();
            let responses = client.decide_all(requests, jev.concurrency).await;
            let mut first_error = None;
            let labels: Vec<String> = responses
                .into_iter()
                .map(|r| match r {
                    Ok(resp) => decisions::cluster_label(&resp, &topics).unwrap_or_default(),
                    Err(e) => {
                        first_error.get_or_insert(e);
                        String::new()
                    }
                })
                .collect();
            send(
                labels_or_error(labels, first_error).map(|labels| TopicLabels {
                    labels,
                    labeller: "Jev".to_string(),
                }),
            );
        });
    }

    // -- API dispatch -------------------------------------------------------

    pub(super) fn dispatch_api_request(&self, event: AppEvent) {
        let Some(ref client) = self.api_client else {
            // No API client -- emit the matching *Loaded(Err) so loading
            // flags get cleared through the normal response path.
            let err: Arc<String> = Arc::new("No API client configured. Use :auth first.".into());
            if let Some(response) = event.into_error_response(err) {
                self.events.send(response);
            }
            return;
        };
        let client = Arc::clone(client);
        let sender = self.events.sender();
        let max_results = self.config.default_max_results;

        tokio::spawn(async move {
            /// Map an API result to an `AppEvent` and send it through the channel.
            fn send_result<T: Send + 'static>(
                sender: &tokio::sync::mpsc::UnboundedSender<Event>,
                result: Result<T, impl std::fmt::Display>,
                wrap: impl FnOnce(ApiResult<T>) -> AppEvent,
            ) {
                let mapped = result.map_err(|e| Arc::new(e.to_string()));
                let _ = sender.send(Event::App(Box::new(wrap(mapped))));
            }

            match event {
                AppEvent::FetchHomeTimeline { pagination_token } => {
                    let mut api = client.lock().await;
                    let result = api
                        .get_home_timeline(max_results, pagination_token.as_deref())
                        .await;
                    send_result(&sender, result, AppEvent::HomeTimelineLoaded);
                }
                AppEvent::FetchUserTimeline {
                    user_id,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .get_timeline(&user_id, max_results, pagination_token.as_deref())
                        .await;
                    send_result(&sender, result, |r| AppEvent::UserTimelineLoaded {
                        user_id,
                        result: r,
                    });
                }
                AppEvent::FetchTweet { tweet_id } => {
                    let api = client.lock().await;
                    let result = api.get_tweet(&tweet_id).await;
                    send_result(&sender, result, |r| AppEvent::TweetLoaded(Box::new(r)));
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
                    send_result(&sender, result, |r| AppEvent::ThreadLoaded {
                        conversation_id,
                        result: r,
                    });
                }
                AppEvent::FetchUser { username } => {
                    let api = client.lock().await;
                    let result = api.get_user(&username).await;
                    send_result(&sender, result, AppEvent::UserLoaded);
                }
                AppEvent::FetchSearch {
                    query,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .search_tweets(&query, max_results, pagination_token.as_deref())
                        .await;
                    send_result(&sender, result, |r| AppEvent::SearchLoaded {
                        query,
                        result: r,
                    });
                }
                AppEvent::FetchMentions { pagination_token } => {
                    let mut api = client.lock().await;
                    let result = api
                        .get_mentions(max_results, pagination_token.as_deref())
                        .await;
                    send_result(&sender, result, AppEvent::MentionsLoaded);
                }
                AppEvent::FetchBookmarks { pagination_token } => {
                    let mut api = client.lock().await;
                    let result = api
                        .get_bookmarks(max_results, pagination_token.as_deref())
                        .await;
                    send_result(&sender, result, AppEvent::BookmarksLoaded);
                }
                AppEvent::FetchFollowers {
                    user_id,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .get_followers(&user_id, max_results, pagination_token.as_deref())
                        .await;
                    send_result(&sender, result, |r| AppEvent::FollowersLoaded {
                        user_id,
                        result: r,
                    });
                }
                AppEvent::FetchFollowing {
                    user_id,
                    pagination_token,
                } => {
                    let api = client.lock().await;
                    let result = api
                        .get_following(&user_id, max_results, pagination_token.as_deref())
                        .await;
                    send_result(&sender, result, |r| AppEvent::FollowingLoaded {
                        user_id,
                        result: r,
                    });
                }
                _ => unreachable!("dispatch_api_request called with non-Fetch variant"),
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

    /// Returns `true` if any embedding provider (MLX or OpenRouter) is available.
    pub(super) fn has_embed_provider(&self) -> bool {
        self.resolve_embed_provider().is_some()
    }

    /// Returns the model ID of the currently resolved embedding provider, if any.
    pub(super) fn resolved_embed_model(&self) -> Option<String> {
        self.resolve_embed_provider().map(|(_, model)| model)
    }

    /// Returns `true` if any chat provider (MLX or OpenRouter) is available.
    /// Jev labels cluster topics when `[jev]` is configured and OpenRouter
    /// is authenticated; it takes precedence over the chat LLM.
    fn resolve_jev(&self) -> Option<(Arc<OpenRouterClient>, crate::config::JevConfig)> {
        let jev = self.config.jev.clone()?;
        let client = self.openrouter_client.as_ref()?;
        Some((Arc::clone(client), jev))
    }

    /// Whether `:topics` has anything to label clusters with.
    pub(super) fn has_topic_labeller(&self) -> bool {
        self.resolve_jev().is_some() || self.has_chat_provider()
    }

    /// Name of the backend `:topics` will use ("Jev", "MLX + Jev", "MLX",
    /// "OpenRouter", ...).
    pub(crate) fn topic_labeller_name(&self) -> Option<&'static str> {
        let Some((_, jev)) = self.resolve_jev() else {
            return self.resolved_chat_provider_name();
        };
        if !jev.propose_labels {
            return Some("Jev");
        }
        Some(match self.resolved_chat_provider_name() {
            Some("MLX") => "MLX + Jev",
            Some(_) => "OpenRouter + Jev",
            None => "Jev",
        })
    }

    pub(super) fn has_chat_provider(&self) -> bool {
        self.resolve_chat_provider().is_some()
    }

    /// Resolve which chat provider to use.
    ///
    /// Respects `preferred_chat_provider` when set.  Default priority:
    /// MLX server (if configured and chat-capable) > OpenRouter (if
    /// authenticated and a model is selected).
    fn resolve_chat_provider(&self) -> Option<(ChatProvider, String)> {
        let mlx = self.resolve_mlx_chat();
        let openrouter = self.resolve_openrouter_chat();

        match self.preferred_chat_provider {
            // Explicit preference: no fallback — return None so the user
            // gets a clear error instead of silent rerouting to a
            // potentially paid/remote provider.
            Some(ChatProviderKind::Mlx) => mlx,
            Some(ChatProviderKind::OpenRouter) => openrouter,
            // Auto: MLX preferred, OpenRouter fallback.
            None => mlx.or(openrouter),
        }
    }

    fn resolve_mlx_chat(&self) -> Option<(ChatProvider, String)> {
        if !self.mlx_chat_supported {
            return None;
        }
        let mlx = self.mlx_client.as_ref()?;
        let model = self
            .config
            .mlx_chat_model
            .clone()
            .unwrap_or_else(|| DEFAULT_MLX_CHAT_MODEL.to_string());
        Some((ChatProvider::Mlx(Arc::clone(mlx)), model))
    }

    fn resolve_openrouter_chat(&self) -> Option<(ChatProvider, String)> {
        let or_client = self.openrouter_client.as_ref()?;
        let model_id = self.selected_chat_model.as_ref()?;
        Some((
            ChatProvider::OpenRouter(Arc::clone(or_client)),
            model_id.clone(),
        ))
    }

    /// Returns the name of the currently resolved chat provider, if any.
    pub(crate) fn resolved_chat_provider_name(&self) -> Option<&'static str> {
        self.resolve_chat_provider().map(|(p, _)| p.name())
    }

    /// Returns the model ID of the currently resolved chat provider, if any.
    pub(super) fn resolved_chat_model(&self) -> Option<String> {
        self.resolve_chat_provider().map(|(_, model)| model)
    }

    fn resolve_embed_provider(&self) -> Option<(EmbedProvider, String)> {
        let mlx = self.resolve_mlx_embed();
        let openrouter = self.resolve_openrouter_embed();
        mlx.or(openrouter)
    }

    fn resolve_mlx_embed(&self) -> Option<(EmbedProvider, String)> {
        if !self.mlx_embed_supported {
            return None;
        }
        let mlx = self.mlx_client.as_ref()?;
        let model = self
            .config
            .mlx_embedding_model
            .clone()
            .unwrap_or_else(|| DEFAULT_MLX_EMBEDDING_MODEL.to_string());
        Some((EmbedProvider::Mlx(Arc::clone(mlx)), model))
    }

    fn resolve_openrouter_embed(&self) -> Option<(EmbedProvider, String)> {
        let or_client = self.openrouter_client.as_ref()?;
        let model_id = self.selected_embedding_model.as_ref()?;
        Some((
            EmbedProvider::OpenRouter(Arc::clone(or_client)),
            model_id.clone(),
        ))
    }
}

fn cluster_topic_max_tokens(num_clusters: usize) -> u32 {
    // Labels are only 3-5 words, but leave room for punctuation, occasional
    // extra tokens per word, and a little drift before we cut the model off.
    let per_cluster_budget = (num_clusters as u32).saturating_mul(16);
    per_cluster_budget.clamp(32, 256)
}

// ---------------------------------------------------------------------------
// Embedding provider abstraction
// ---------------------------------------------------------------------------

/// A unified embedding provider that wraps either OpenRouter or a local MLX
/// server.  Both return `EmbeddingResponse` in the same OpenAI-compatible
/// format.
#[derive(Clone)]
enum EmbedProvider {
    OpenRouter(Arc<OpenRouterClient>),
    Mlx(Arc<MlxClient>),
}

impl EmbedProvider {
    /// Embed `texts` for `task`, returning one vector per text in input order.
    ///
    /// Only the MLX server takes a task; OpenRouter embeds the text as given.
    async fn embed(
        &self,
        model: &str,
        texts: &[String],
        task: EmbedTask,
    ) -> Result<Vec<Vec<f64>>, Arc<String>> {
        let resp = match self {
            Self::OpenRouter(client) => client
                .embed(model, texts)
                .await
                .map_err(|e| Arc::new(e.to_string())),
            Self::Mlx(client) => client
                .embed(model, texts, task)
                .await
                .map_err(|e| Arc::new(e.to_string())),
        }?;
        ordered_embeddings(resp, texts.len())
    }

    /// Embed a search query and the documents to score against it.
    ///
    /// Queries and documents take different task prompts, so the MLX server
    /// gets one request for each. OpenRouter has no task prompts and takes
    /// both in a single batch.
    async fn embed_query_and_documents(
        &self,
        model: &str,
        query: &str,
        documents: &[String],
    ) -> Result<(Vec<f64>, Vec<Vec<f64>>), Arc<String>> {
        let query = [query.to_string()];
        let (mut query_embs, doc_embs) = match self {
            Self::Mlx(_) => tokio::try_join!(
                self.embed(model, &query, EmbedTask::SearchQuery),
                self.embed(model, documents, EmbedTask::Document),
            )?,
            Self::OpenRouter(_) => {
                let texts: Vec<String> = query.iter().chain(documents).cloned().collect();
                let mut embs = self.embed(model, &texts, EmbedTask::Document).await?;
                let doc_embs = embs.split_off(1);
                (embs, doc_embs)
            }
        };
        let query_emb = query_embs
            .pop()
            .ok_or_else(|| Arc::new("No embedding returned for the query".to_string()))?;
        Ok((query_emb, doc_embs))
    }
}

/// Unpack an embedding response into vectors in input order, checking that
/// the provider returned one per input.
fn ordered_embeddings(
    resp: EmbeddingResponse,
    expected: usize,
) -> Result<Vec<Vec<f64>>, Arc<String>> {
    if resp.data.len() != expected {
        return Err(Arc::new(format!(
            "Expected {expected} embeddings, got {}",
            resp.data.len()
        )));
    }
    let mut data = resp.data;
    data.sort_by_key(|d| d.index);
    Ok(data.into_iter().map(|d| d.embedding).collect())
}

// ---------------------------------------------------------------------------
// Chat provider abstraction
// ---------------------------------------------------------------------------

/// A unified chat provider that wraps either OpenRouter or a local MLX
/// server.  Both return `ChatCompletionResponse` in the same OpenAI-compatible
/// format.
#[derive(Clone)]
enum ChatProvider {
    OpenRouter(Arc<OpenRouterClient>),
    Mlx(Arc<MlxClient>),
}

impl ChatProvider {
    fn name(&self) -> &'static str {
        match self {
            Self::Mlx(_) => "MLX",
            Self::OpenRouter(_) => "OpenRouter",
        }
    }

    async fn chat_completion(
        &self,
        model: &str,
        messages: Vec<openrouter::types::ChatMessage>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        reasoning: Option<openrouter::types::ReasoningConfig>,
    ) -> Result<openrouter::types::ChatCompletionResponse, Arc<String>> {
        match self {
            Self::OpenRouter(client) => client
                .chat_completion(model, messages, max_tokens, temperature, reasoning)
                .await
                .map_err(|e| Arc::new(e.to_string())),
            Self::Mlx(client) => client
                .chat_completion(model, messages, max_tokens, temperature, reasoning)
                .await
                .map_err(|e| Arc::new(e.to_string())),
        }
    }

    /// One system + user turn with reasoning turned off, returning the
    /// answer text. Hiding reasoning (`exclude`) is not enough for short
    /// budgets: hidden reasoning still counts against `max_tokens` and can
    /// use all of it before the answer starts. Models that refuse to turn
    /// reasoning off are retried with low, hidden reasoning and extra room.
    async fn complete_text(
        &self,
        model: &str,
        system: &str,
        user: String,
        max_tokens: u32,
    ) -> Result<String, Arc<String>> {
        use openrouter::types::{ChatMessage, ReasoningConfig};
        let messages = vec![
            ChatMessage {
                role: "system".into(),
                content: system.into(),
            },
            ChatMessage {
                role: "user".into(),
                content: user,
            },
        ];
        let resp = match self {
            Self::OpenRouter(client) => match client
                .chat_completion(
                    model,
                    messages.clone(),
                    Some(max_tokens),
                    Some(0.3),
                    Some(ReasoningConfig::disabled()),
                )
                .await
            {
                Err(openrouter::OpenRouterError::ApiError {
                    status: 400,
                    detail,
                }) if detail.to_ascii_lowercase().contains("reasoning") => {
                    tracing::debug!("{model} cannot disable reasoning: {detail}");
                    client
                        .chat_completion(
                            model,
                            messages,
                            Some(max_tokens.saturating_add(REASONING_HEADROOM_TOKENS)),
                            Some(0.3),
                            Some(ReasoningConfig::hidden_low()),
                        )
                        .await
                }
                other => other,
            }
            .map_err(|e| Arc::new(e.to_string()))?,
            Self::Mlx(_) => {
                self.chat_completion(model, messages, Some(max_tokens), Some(0.3), None)
                    .await?
            }
        };

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
        Ok(content)
    }
}

/// The chat model writes one label per cluster.
async fn chat_labels(
    provider: &ChatProvider,
    model: &str,
    body: String,
    num_clusters: usize,
) -> ApiResult<Vec<String>> {
    let content = provider
        .complete_text(
            model,
            CLUSTER_LABEL_PROMPT,
            body,
            cluster_topic_max_tokens(num_clusters),
        )
        .await?;
    let labels = parse_cluster_topic_labels(&content, num_clusters);
    if labels.iter().all(|l| l.is_empty()) {
        return Err(Arc::new(format!(
            "No labels parsed from response: {content}"
        )));
    }
    Ok(labels)
}

/// The chat model proposes candidate labels and Jev picks a distinct one
/// per cluster. `Ok(None)` when the proposal doesn't cover every cluster;
/// `Err` when the chat request fails. The inner result is the Jev outcome.
async fn propose_then_pick(
    client: &OpenRouterClient,
    jev: &crate::config::JevConfig,
    provider: &ChatProvider,
    model: &str,
    body: String,
    cluster_posts: &[Vec<String>],
) -> Result<Option<ApiResult<Vec<String>>>, Arc<String>> {
    use crate::openrouter::decisions;

    let num_clusters = cluster_posts.len();
    let content = provider
        .complete_text(
            model,
            decisions::PROPOSE_LABELS_PROMPT,
            body,
            decisions::propose_max_tokens(num_clusters),
        )
        .await?;
    let Some(candidates) = decisions::parse_label_candidates(&content, num_clusters) else {
        return Ok(None);
    };
    let requests = cluster_posts
        .iter()
        .map(|posts| {
            let posts: Vec<&str> = posts.iter().map(String::as_str).collect();
            decisions::pick_label_request(&jev.model, &posts, &candidates)
        })
        .collect();
    let responses = client.decide_all(requests, jev.concurrency).await;
    let ok: Vec<_> = responses.iter().map(|r| r.as_ref().ok()).collect();
    let labels: Vec<String> = decisions::assign_unique_labels(&ok, candidates.len())
        .into_iter()
        .map(|pick| match pick {
            decisions::LabelPick::Candidate(i) => candidates[i].clone(),
            decisions::LabelPick::Mixed => "Mixed".to_string(),
            decisions::LabelPick::Unlabelled => String::new(),
        })
        .collect();
    Ok(Some(labels_or_error(
        labels,
        responses.into_iter().find_map(Result::err),
    )))
}

/// Partial success still applies (empty labels keep the placeholder);
/// all-empty is an error.
fn labels_or_error(
    labels: Vec<String>,
    first_error: Option<openrouter::OpenRouterError>,
) -> ApiResult<Vec<String>> {
    if labels.iter().all(|l| l.is_empty()) {
        Err(Arc::new(format!(
            "Jev labelling failed: {}",
            first_error.map_or_else(|| "no topic answers".into(), |e| e.to_string())
        )))
    } else {
        Ok(labels)
    }
}

/// Extra `max_tokens` when a model insists on reasoning (hidden reasoning
/// counts against the budget).
const REASONING_HEADROOM_TOKENS: u32 = 2048;

const CLUSTER_LABEL_PROMPT: &str = "For each cluster of tweets, analyze a common topic and \
     generate a short descriptive topic label (3-5 words). \
     Reply with exactly one line per cluster in the format \
     `Cluster <index>: <label>`. Include every cluster 0 \
     through N-1. No numbering, no preamble, no trailing \
     explanations.";

/// Up to `MAX_POSTS_PER_CLUSTER` posts per cluster under `## Cluster <n>`
/// headings, the user turn for both chat labelling and Jev proposals.
fn cluster_prompt_body(result: &crate::embeddings::cluster::ClusterResult) -> String {
    let mut body = String::new();
    for c in 0..result.num_clusters() {
        body.push_str(&format!("## Cluster {c}\n"));
        for (_, text) in result
            .texts_for_cluster(c)
            .iter()
            .take(openrouter::decisions::MAX_POSTS_PER_CLUSTER)
        {
            let truncated: String = text.chars().take(280).collect();
            body.push_str(&format!("- {}\n", truncated.replace('\n', " ")));
        }
        body.push('\n');
    }
    body
}

/// Clean a raw label line: strip bullet/numbering prefixes and trim.
/// Returns `None` for empty strings or lines that are too long to be a
/// 3-5 word topic label (reasoning leakage, prose explanations).
fn clean_label_line(line: &str) -> Option<String> {
    let t = line.trim();
    let t = t.strip_prefix("- ").unwrap_or(t);
    let t = t.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')');
    let t = t.trim();
    if t.is_empty() || t.len() > 80 {
        None
    } else {
        Some(t.to_string())
    }
}

/// Extract a `(cluster_index, label)` pair from a line that starts with
/// `Cluster <n>:` (case-insensitive). Returns `None` if the line has no
/// such prefix or the label portion is empty/oversized.
fn parse_prefixed_label(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim();
    let (head, rest) = trimmed.split_once(':')?;
    let head = head.trim();
    let (tag, index_str) = head.split_once(|c: char| c.is_whitespace())?;
    if !tag.eq_ignore_ascii_case("cluster") {
        return None;
    }
    let idx: usize = index_str.trim().parse().ok()?;
    let label = clean_label_line(rest)?;
    Some((idx, label))
}

/// Parse an LLM response into exactly `num_clusters` labels, indexed by
/// cluster ID.
///
/// Pass 1: for every line matching `Cluster <n>: <label>`, place the
/// label at index `n` (first match wins, ignoring extras beyond
/// `num_clusters`).
///
/// Pass 2 (pure-legacy only): if Pass 1 parsed **zero** prefixed labels,
/// fall back to positional assignment over the bare lines. This keeps
/// older models that emit unprefixed output working.
///
/// As soon as any `Cluster <n>:` prefix is parsed we treat the response
/// as structured and refuse to fill missing slots from bare lines —
/// those are almost always preamble ("Sure! Here are the labels:") or
/// explanation, and letting them land in a missing cluster's slot would
/// reintroduce the exact mislabeling this parser was built to prevent.
///
/// Empty `String`s in the result indicate clusters the parser could not
/// confidently label — the UI layer should keep its centroid-closest
/// fallback in those slots.
pub(crate) fn parse_cluster_topic_labels(content: &str, num_clusters: usize) -> Vec<String> {
    let mut labels = vec![String::new(); num_clusters];
    let mut leftover: Vec<String> = Vec::new();
    let mut any_prefixed = false;

    for line in content.lines() {
        match parse_prefixed_label(line) {
            Some((idx, label)) => {
                any_prefixed = true;
                if idx < num_clusters && labels[idx].is_empty() {
                    labels[idx] = label;
                }
                // Out-of-range index or slot already filled — ignore.
            }
            None => {
                if let Some(cleaned) = clean_label_line(line) {
                    leftover.push(cleaned);
                }
            }
        }
    }

    if !any_prefixed {
        let mut leftover_iter = leftover.into_iter();
        for slot in labels.iter_mut() {
            if slot.is_empty()
                && let Some(next) = leftover_iter.next()
            {
                *slot = next;
            }
        }
    }

    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedding_response(data: &[(usize, f64)]) -> EmbeddingResponse {
        let data: Vec<_> = data
            .iter()
            .map(|(index, value)| serde_json::json!({"index": index, "embedding": [value]}))
            .collect();
        serde_json::from_value(serde_json::json!({"model": "m", "data": data})).unwrap()
    }

    #[test]
    fn ordered_embeddings_sorts_by_index() {
        let resp = embedding_response(&[(2, 2.0), (0, 0.0), (1, 1.0)]);
        assert_eq!(
            ordered_embeddings(resp, 3).unwrap(),
            vec![vec![0.0], vec![1.0], vec![2.0]]
        );
    }

    #[test]
    fn ordered_embeddings_rejects_a_short_response() {
        let err = ordered_embeddings(embedding_response(&[(0, 0.0)]), 2).unwrap_err();
        assert_eq!(*err, "Expected 2 embeddings, got 1");
    }

    #[test]
    fn ordered_embeddings_accepts_an_empty_batch() {
        assert!(
            ordered_embeddings(embedding_response(&[]), 0)
                .unwrap()
                .is_empty()
        );
    }

    /// Live check of propose-then-pick against OpenRouter, using the same
    /// helpers as `dispatch_jev_cluster_topics`. Run with:
    /// `OPENROUTER_API_KEY=... cargo test propose_live -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn propose_live_then_jev_picks_distinct_labels() {
        use crate::embeddings::cluster::ClusterResult;
        use crate::openrouter::decisions;

        let key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY");
        let client = Arc::new(OpenRouterClient::new(key));
        // Rust and terminal-tool clusters both map to "Software engineering"
        // in the default taxonomy.
        let clusters: [&[&str]; 3] = [
            &[
                "Rewrote our parser in Rust, the borrow checker caught two real bugs",
                "New crate release: zero-copy deserialization for Rust",
                "Async Rust finally clicked for me after reading the Tokio docs",
            ],
            &[
                "My Neovim + tmux setup after a year of tweaking dotfiles",
                "Switched terminal emulators again, Ghostty feels fast",
                "zsh startup down to 40ms after trimming plugins",
            ],
            &[
                "Bitcoin ETF inflows hit a new weekly record",
                "ETH gas fees spiked during the token launch",
                "Solana DEX volume overtook Ethereum this week",
            ],
        ];
        let mut texts = Vec::new();
        let mut labels = Vec::new();
        for (c, posts) in clusters.iter().enumerate() {
            for p in *posts {
                texts.push(p.to_string());
                labels.push(c);
            }
        }
        let n = texts.len();
        let result = ClusterResult {
            points: vec![(0.0, 0.0); n],
            labels,
            tweet_ids: (0..n).map(|i| i.to_string()).collect(),
            tweet_texts: texts,
            conversation_ids: vec![None; n],
            author_ids: vec![None; n],
            cluster_topics: vec![String::new(); 3],
        };

        // Qwen3.5-9B used to spend the whole budget on hidden reasoning.
        let provider = ChatProvider::OpenRouter(Arc::clone(&client));
        let content = provider
            .complete_text(
                "qwen/qwen3.5-9b",
                decisions::PROPOSE_LABELS_PROMPT,
                cluster_prompt_body(&result),
                decisions::propose_max_tokens(3),
            )
            .await
            .expect("proposal");
        let candidates = decisions::parse_label_candidates(&content, 3).expect(&content);

        let requests = clusters
            .iter()
            .map(|posts| {
                decisions::pick_label_request(decisions::DEFAULT_JEV_MODEL, posts, &candidates)
            })
            .collect();
        let responses = client.decide_all(requests, 3).await;
        let ok: Vec<_> = responses.iter().map(|r| r.as_ref().ok()).collect();
        let picked = decisions::assign_unique_labels(&ok, candidates.len());
        let names: Vec<&str> = picked
            .iter()
            .map(|pick| match pick {
                decisions::LabelPick::Candidate(i) => candidates[*i].as_str(),
                other => panic!("every cluster here has a clear topic: {other:?}"),
            })
            .collect();
        eprintln!("candidates: {candidates:?}\npicked: {names:?}");

        // Routing: these clusters are tech; sports and cooking are not.
        async fn share(client: &OpenRouterClient, sets: &[&[&str]]) -> f64 {
            let sizes: Vec<usize> = sets.iter().map(|p| p.len()).collect();
            let requests = sets
                .iter()
                .map(|posts| decisions::tech_request(decisions::DEFAULT_JEV_MODEL, posts))
                .collect();
            let responses = client.decide_all(requests, 3).await;
            let ok: Vec<_> = responses.iter().map(|r| r.as_ref().ok()).collect();
            decisions::tech_share(&ok, &sizes).expect("tech answers")
        }
        let tech = share(&client, &clusters).await;
        assert!(tech >= decisions::MIN_TECH_SHARE, "tech share {tech}");
        let non_tech = share(
            &client,
            &[
                &[
                    "Arsenal won 2-0, Saka scored again",
                    "NBA season tips off tonight, Lakers vs Celtics",
                    "Marathon PR today, 3:12",
                ],
                &[
                    "Made sourdough with a 48-hour cold proof",
                    "One-pot unstuffed peppers recipe for busy weeknights",
                    "Best ramen in Shibuya, no contest",
                ],
            ],
        )
        .await;
        assert!(
            non_tech < decisions::MIN_TECH_SHARE,
            "non-tech share {non_tech}"
        );
        eprintln!("tech share {tech}, non-tech share {non_tech}");

        // A model that refuses `reasoning.enabled = false` falls back.
        let short = provider
            .complete_text(
                "anthropic/claude-sonnet-5.5",
                "Reply with a 2-word topic label only.",
                "Cargo, crates and the borrow checker".into(),
                32,
            )
            .await
            .expect("fallback");
        assert!(!short.trim().is_empty());
    }

    #[test]
    fn parses_well_formed_prefix_output() {
        let content = "Cluster 0: AI news\nCluster 1: Rust tooling\nCluster 2: Market updates";
        let labels = parse_cluster_topic_labels(content, 3);
        assert_eq!(
            labels,
            vec!["AI news", "Rust tooling", "Market updates"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn binds_labels_by_index_when_out_of_order() {
        let content = "Cluster 2: third topic\nCluster 0: first topic\nCluster 1: second topic";
        let labels = parse_cluster_topic_labels(content, 3);
        assert_eq!(labels[0], "first topic");
        assert_eq!(labels[1], "second topic");
        assert_eq!(labels[2], "third topic");
    }

    #[test]
    fn ignores_preamble_and_still_binds_correctly() {
        let content = "Sure! Here are the topic labels for each cluster:\n\n\
                       Cluster 0: neural signals\n\
                       Cluster 1: open source releases\n\
                       Cluster 2: macroeconomic commentary\n";
        let labels = parse_cluster_topic_labels(content, 3);
        assert_eq!(labels[0], "neural signals");
        assert_eq!(labels[1], "open source releases");
        assert_eq!(labels[2], "macroeconomic commentary");
    }

    #[test]
    fn missing_cluster_leaves_slot_empty_for_ui_fallback() {
        // Model dropped cluster 1 — we must NOT shift cluster 2's label
        // into slot 1 (that was the old bug).
        let content = "Cluster 0: alpha\nCluster 2: gamma";
        let labels = parse_cluster_topic_labels(content, 3);
        assert_eq!(labels[0], "alpha");
        assert_eq!(labels[1], "");
        assert_eq!(labels[2], "gamma");
    }

    #[test]
    fn legacy_positional_fallback_when_no_prefixes() {
        let content = "alpha\nbeta\ngamma";
        let labels = parse_cluster_topic_labels(content, 3);
        assert_eq!(labels, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn bare_lines_do_not_leak_into_missing_slot_once_any_prefix_seen() {
        // When the model emits at least one structured `Cluster N:` line,
        // we must NOT pull bare lines into missing slots — those are
        // almost always preamble or explanation, and letting them land
        // on a dropped cluster reintroduces the mislabel bug.
        let content = "Cluster 0: explicit\nfallback";
        let labels = parse_cluster_topic_labels(content, 2);
        assert_eq!(labels[0], "explicit");
        assert_eq!(labels[1], "");
    }

    #[test]
    fn preamble_plus_dropped_cluster_leaves_slot_empty() {
        // Regression: Codex review flagged that a response containing
        // preamble prose AND partial `Cluster N:` labels would slot the
        // preamble into the missing cluster's position. The missing
        // cluster must stay empty so the centroid-closest tweet
        // fallback in the UI can kick in.
        let content = "Sure! Here are the topic labels for each cluster:\n\
                       \n\
                       Cluster 0: alpha\n\
                       Cluster 2: gamma\n";
        let labels = parse_cluster_topic_labels(content, 3);
        assert_eq!(labels[0], "alpha");
        assert_eq!(labels[1], "");
        assert_eq!(labels[2], "gamma");
    }

    #[test]
    fn drops_overlong_lines() {
        let long = "x".repeat(120);
        let content = format!("Cluster 0: {long}\nCluster 1: ok");
        let labels = parse_cluster_topic_labels(&content, 2);
        assert_eq!(labels[0], "");
        assert_eq!(labels[1], "ok");
    }

    #[test]
    fn duplicate_index_keeps_first_match() {
        let content = "Cluster 0: first\nCluster 0: second";
        let labels = parse_cluster_topic_labels(content, 1);
        assert_eq!(labels[0], "first");
    }

    #[test]
    fn out_of_range_index_ignored() {
        let content = "Cluster 5: ghost\nCluster 0: real";
        let labels = parse_cluster_topic_labels(content, 2);
        assert_eq!(labels[0], "real");
        assert_eq!(labels[1], "");
    }

    #[test]
    fn strips_bullet_and_numbering_prefixes() {
        let content = "- 1. alpha\n2) beta\n- gamma";
        let labels = parse_cluster_topic_labels(content, 3);
        assert_eq!(labels, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn entirely_empty_response_yields_all_empty_slots() {
        let labels = parse_cluster_topic_labels("   \n\n   ", 3);
        assert!(labels.iter().all(|s| s.is_empty()));
    }

    #[test]
    fn case_insensitive_cluster_keyword() {
        let content = "cluster 0: lower\nCLUSTER 1: upper";
        let labels = parse_cluster_topic_labels(content, 2);
        assert_eq!(labels[0], "lower");
        assert_eq!(labels[1], "upper");
    }
}
