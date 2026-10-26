use std::sync::Arc;

use super::{App, ClusterSource};
use crate::api::types::{Includes, Media, Tweet, User};
use crate::config::TaskPrompts;
use crate::event::{ApiResult, AppEvent, Event, TopicLabels, ViewKind};
use crate::llamacpp::client::LlamaCppClient;
use crate::mlx::client::{EmbedTask, MlxClient};
use crate::openrouter;
use crate::openrouter::client::OpenRouterClient;
use crate::openrouter::types::{EmbeddingResponse, Model};

pub(super) const DEFAULT_MLX_EMBEDDING_MODEL: &str = "mlx-community/Qwen3-Embedding-0.6B-mxfp8";
const DEFAULT_MLX_CHAT_MODEL: &str = "mlx-community/Qwen3.5-0.8B-OptiQ-4bit";

pub use crate::config::ChatProviderKind;

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

    /// Start labelling the current clusters. The loading hint names no
    /// labeller until the request reports the one it is using.
    pub(super) fn start_cluster_topics(&mut self) {
        self.cluster_topics_loading = true;
        self.cluster_topics_labeller = None;
        self.dispatch_generate_cluster_topics();
    }

    fn dispatch_generate_cluster_topics(&self) {
        if let Some((client, jev)) = self.resolve_jev() {
            self.dispatch_jev_cluster_topics(client, jev);
            return;
        }
        let generation = self.cluster_generation;
        let Some((provider, model)) = self.resolve_chat_provider() else {
            self.events.send(AppEvent::ClusterTopicsGenerated(
                generation,
                Err(Arc::new(
                    "No chat provider configured. Set mlx_server_url or \
                     [llama_cpp] chat_url in config, or use :openrouter-auth + \
                     :openrouter-models."
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

        self.events.send(AppEvent::ClusterTopicsLabeller(
            generation,
            provider.name().to_string(),
        ));
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
    /// topic taxonomy. Jev requests go out concurrently. Each step that
    /// writes labels reports its labeller first, for the loading hint.
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
                    .representative_texts(c, decisions::MAX_POSTS_PER_CLUSTER)
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            })
            .collect();
        let photo_urls =
            super::clustering::topic_photo_urls(result, decisions::MAX_POSTS_PER_CLUSTER);
        let photo_cache = Arc::clone(&self.photo_cache);
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
            let started = |labeller: &str| {
                let _ = sender.send(Event::App(Box::new(AppEvent::ClusterTopicsLabeller(
                    generation,
                    labeller.to_string(),
                ))));
            };
            let posts =
                |c: usize| -> Vec<&str> { cluster_posts[c].iter().map(String::as_str).collect() };

            if let Some((provider, model, body)) = chat {
                // Photos only go to the pick round. Download them while
                // the tech check and the label proposal run.
                let photos = tokio::spawn(super::clustering::fetch_topic_photos(
                    photo_cache,
                    photo_urls,
                ));
                let tech_requests = (0..num_clusters)
                    .map(|c| decisions::tech_request(&jev.model, &posts(c)))
                    .collect();
                let tech = client.decide_all(tech_requests, jev.concurrency).await;
                let ok: Vec<_> = tech.iter().map(|r| r.as_ref().ok()).collect();
                let tech_share = decisions::tech_share(&ok, &cluster_sizes);

                if tech_share.is_some_and(|s| s >= decisions::MIN_TECH_SHARE) {
                    let labeller = format!("{} + Jev", provider.name());
                    started(&labeller);
                    match propose_then_pick(
                        &client,
                        &jev,
                        &provider,
                        &model,
                        body,
                        &cluster_posts,
                        photos,
                    )
                    .await
                    {
                        Ok(Some(labels)) => {
                            return send(labels.map(|labels| TopicLabels { labels, labeller }));
                        }
                        Ok(None) => tracing::warn!(
                            "label proposal did not cover every cluster; using the topic taxonomy"
                        ),
                        Err(e) => {
                            tracing::warn!("label proposal failed ({e}); using the topic taxonomy")
                        }
                    }
                } else {
                    photos.abort();
                    // Non-tech timeline (or Jev couldn't tell): the chat
                    // model's own labels beat Jev's picks there.
                    // Partial labels would leave placeholders; the taxonomy
                    // can still label every cluster.
                    started(provider.name());
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

            started(JEV_LABELLER);
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
                    labeller: JEV_LABELLER.to_string(),
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
        let search_query = self.search_query.clone();

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
                AppEvent::FetchForCluster {
                    source,
                    page_size,
                    pagination_token,
                } => {
                    let token = pagination_token.as_deref();
                    match source {
                        ClusterSource::Home => {
                            let mut api = client.lock().await;
                            let result = api.get_home_timeline(page_size, token).await;
                            send_result(&sender, result, AppEvent::HomeTimelineLoaded);
                        }
                        ClusterSource::Mentions => {
                            let mut api = client.lock().await;
                            let result = api.get_mentions(page_size, token).await;
                            send_result(&sender, result, AppEvent::MentionsLoaded);
                        }
                        ClusterSource::Bookmarks => {
                            let mut api = client.lock().await;
                            let result = api.get_bookmarks(page_size, token).await;
                            send_result(&sender, result, AppEvent::BookmarksLoaded);
                        }
                        ClusterSource::Search => {
                            let api = client.lock().await;
                            let result = api.search_tweets(&search_query, page_size, token).await;
                            send_result(&sender, result, |r| AppEvent::SearchLoaded {
                                query: search_query,
                                result: r,
                            });
                        }
                    }
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

    /// Cache the users and media of a response for author and photo lookup.
    pub(super) fn cache_includes(&mut self, includes: &Option<Includes>) {
        let Some(inc) = includes else {
            return;
        };
        for user in inc.users.iter().flatten() {
            self.users_cache.insert(user.id.clone(), user.clone());
        }
        for media in inc.media.iter().flatten() {
            self.media_cache
                .insert(media.media_key.clone(), media.clone());
        }
    }

    /// Look up a photo or video by its media key from the includes cache.
    pub fn lookup_media(&self, media_key: &str) -> Option<&Media> {
        self.media_cache.get(media_key)
    }

    /// Look up a user by their ID from the includes cache.
    pub fn lookup_user(&self, user_id: &str) -> Option<&User> {
        self.users_cache.get(user_id)
    }

    /// Returns `true` if any embedding provider (MLX, llama.cpp or
    /// OpenRouter) is available.
    pub(super) fn has_embed_provider(&self) -> bool {
        self.resolve_embed_provider().is_some()
    }

    /// Returns the model ID of the currently resolved embedding provider, if any.
    pub(super) fn resolved_embed_model(&self) -> Option<String> {
        self.resolve_embed_provider().map(|(_, model)| model)
    }

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

    /// Returns `true` if any chat provider (MLX, llama.cpp or OpenRouter)
    /// is available.
    pub(super) fn has_chat_provider(&self) -> bool {
        self.resolve_chat_provider().is_some()
    }

    /// Resolve which chat provider to use.
    ///
    /// Respects `preferred_chat_provider` when set.  Default priority:
    /// MLX server (if configured and chat-capable) > llama.cpp server (if
    /// configured and reachable) > OpenRouter (if authenticated and a model
    /// is selected). The local servers come first: they are free and keep
    /// posts on this machine.
    fn resolve_chat_provider(&self) -> Option<(ChatProvider, String)> {
        match self.preferred_chat_provider {
            // Explicit preference: no fallback — return None so the user
            // gets a clear error instead of silent rerouting to a
            // potentially paid/remote provider.
            Some(ChatProviderKind::Mlx) => self.resolve_mlx_chat(),
            Some(ChatProviderKind::LlamaCpp) => self.resolve_llama_cpp_chat(),
            Some(ChatProviderKind::OpenRouter) => self.resolve_openrouter_chat(),
            None => self
                .resolve_mlx_chat()
                .or_else(|| self.resolve_llama_cpp_chat())
                .or_else(|| self.resolve_openrouter_chat()),
        }
    }

    fn resolve_llama_cpp_chat(&self) -> Option<(ChatProvider, String)> {
        let client = self.llama_cpp.chat.as_ref()?;
        let model = self.llama_cpp.chat_model.clone()?;
        Some((ChatProvider::LlamaCpp(Arc::clone(client)), model))
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

    /// Same priority as chat: MLX > llama.cpp > OpenRouter.
    pub(super) fn resolve_embed_provider(&self) -> Option<(EmbedProvider, String)> {
        self.resolve_mlx_embed()
            .or_else(|| self.resolve_llama_cpp_embed())
            .or_else(|| self.resolve_openrouter_embed())
    }

    pub(super) fn resolve_llama_cpp_embed(&self) -> Option<(EmbedProvider, String)> {
        let client = self.llama_cpp.embed.as_ref()?;
        let model = self.llama_cpp.embed_model.clone()?;
        Some((
            EmbedProvider::LlamaCpp(Arc::clone(client), Arc::clone(&self.llama_cpp.task_prompts)),
            model,
        ))
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

    pub(super) fn resolve_openrouter_embed(&self) -> Option<(EmbedProvider, String)> {
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

/// A unified embedding provider that wraps OpenRouter, a local MLX server
/// or a llama.cpp server.  All return `EmbeddingResponse` in the same
/// OpenAI-compatible format.
#[derive(Clone)]
pub(super) enum EmbedProvider {
    OpenRouter(Arc<OpenRouterClient>),
    Mlx(Arc<MlxClient>),
    /// The client and the task prompts from `[llama_cpp.task_prompts]`.
    LlamaCpp(Arc<LlamaCppClient>, Arc<TaskPrompts>),
}

impl EmbedProvider {
    /// Embed `texts` for `task`, returning one vector per text in input order.
    ///
    /// The MLX server takes the task and picks the prompt itself; for
    /// llama.cpp the configured prompt is put in front of each text here.
    /// OpenRouter embeds the text as given.
    pub(super) async fn embed(
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
            Self::LlamaCpp(client, prompts) => client
                .embed(model, &prompts.apply(task, texts))
                .await
                .map_err(|e| Arc::new(e.to_string())),
        }?;
        ordered_embeddings(resp, texts.len())
    }

    /// Embed `(text, photo data URL)` documents, one vector each, in the
    /// space of `embed` vectors from the same model and task.
    pub(super) async fn embed_documents(
        &self,
        model: &str,
        documents: &[(String, Arc<str>)],
        task: EmbedTask,
    ) -> Result<Vec<Vec<f64>>, Arc<String>> {
        let resp = match self {
            Self::OpenRouter(client) => client
                .embed_documents(model, documents)
                .await
                .map_err(|e| Arc::new(e.to_string())),
            Self::Mlx(client) => client
                .embed_documents(model, documents, task)
                .await
                .map_err(|e| Arc::new(e.to_string())),
            // `/v1/embeddings` on llama-server takes text only.
            Self::LlamaCpp(..) => Err(Arc::new(
                "llama.cpp embeddings take text only, not photos".to_string(),
            )),
        }?;
        ordered_embeddings(resp, documents.len())
    }

    /// Embed a search query and the documents to score against it.
    ///
    /// Queries and documents take different task prompts, so the MLX server
    /// gets one request for each, and so does llama.cpp when its configured
    /// prompts differ. Otherwise both go in a single batch.
    async fn embed_query_and_documents(
        &self,
        model: &str,
        query: &str,
        documents: &[String],
    ) -> Result<(Vec<f64>, Vec<Vec<f64>>), Arc<String>> {
        let query = [query.to_string()];
        let separate = match self {
            Self::Mlx(_) => true,
            Self::LlamaCpp(_, prompts) => prompts.splits_search(),
            Self::OpenRouter(_) => false,
        };
        let (mut query_embs, doc_embs) = match separate {
            true => tokio::try_join!(
                self.embed(model, &query, EmbedTask::SearchQuery),
                self.embed(model, documents, EmbedTask::Document),
            )?,
            false => {
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

/// A unified chat provider that wraps OpenRouter, a local MLX server or a
/// llama.cpp server.  All return `ChatCompletionResponse` in the same
/// OpenAI-compatible format.
#[derive(Clone)]
enum ChatProvider {
    OpenRouter(Arc<OpenRouterClient>),
    Mlx(Arc<MlxClient>),
    LlamaCpp(Arc<LlamaCppClient>),
}

impl ChatProvider {
    fn name(&self) -> &'static str {
        match self {
            Self::Mlx(_) => "MLX",
            Self::LlamaCpp(_) => "llama.cpp",
            Self::OpenRouter(_) => "OpenRouter",
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
            // No reasoning setting to send: the MLX server turns thinking
            // off itself, and gives a model that thinks anyway extra room
            // on top of `max_tokens`.
            Self::Mlx(client) => client
                .chat_completion(model, messages, Some(max_tokens), Some(0.3))
                .await
                .map_err(|e| Arc::new(e.to_string()))?,
            // Thinking is turned off in the chat template. llama-server
            // puts the thinking of a model that thinks anyway in
            // `reasoning_content`, so `content` holds only the answer.
            Self::LlamaCpp(client) => client
                .chat_completion(model, messages, Some(max_tokens), Some(0.3))
                .await
                .map_err(|e| Arc::new(e.to_string()))?,
        };

        let choice = resp
            .choices
            .first()
            .ok_or_else(|| Arc::new("Chat model returned no choices".to_string()))?;

        let reason = choice.finish_reason.as_deref().unwrap_or("unknown");
        let out_of_budget = || {
            Arc::new(
                "Model exhausted token budget on reasoning \
                 before producing content (finish_reason: length)"
                    .to_string(),
            )
        };

        // Only use `content` -- reasoning/reasoning_content are
        // chain-of-thought fields and must NOT be treated as output.
        let raw = choice.message.content.clone().ok_or_else(|| {
            if reason == "length" {
                out_of_budget()
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
            // The MLX server strips reasoning itself and sends "" for a
            // reply that was all reasoning, even with its extra room.
            return Err(if reason == "length" {
                out_of_budget()
            } else {
                Arc::new(
                    "Chat model returned empty content \
                     (after stripping reasoning tags)"
                        .to_string(),
                )
            });
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
    photos: tokio::task::JoinHandle<Vec<Vec<Option<Arc<str>>>>>,
) -> Result<Option<ApiResult<Vec<String>>>, Arc<String>> {
    use crate::openrouter::{decision_photos, decisions};

    let num_clusters = cluster_posts.len();
    let content = provider
        .complete_text(
            model,
            decisions::PROPOSE_LABELS_PROMPT,
            body,
            decisions::propose_max_tokens(num_clusters),
        )
        .await
        .inspect_err(|_| photos.abort())?;
    let Some(candidates) = decisions::parse_label_candidates(&content, num_clusters) else {
        photos.abort();
        return Ok(None);
    };
    let photos = photos.await.unwrap_or_else(|e| {
        tracing::warn!("photo downloads failed ({e}); labelling from text only");
        Vec::new()
    });
    let sent = photos.iter().flatten().filter(|p| p.is_some()).count();
    tracing::info!("{sent} photos go with the Jev pick round");
    let requests = cluster_posts
        .iter()
        .enumerate()
        .map(|(c, posts)| {
            let posts: Vec<&str> = posts.iter().map(String::as_str).collect();
            let mut req = decisions::pick_label_request(&jev.model, &posts, &candidates);
            if let Some(cluster_photos) = photos.get(c) {
                let refs: Vec<Option<&str>> = cluster_photos.iter().map(Option::as_deref).collect();
                decision_photos::attach_photos(&mut req, &refs);
            }
            req
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

/// Labeller name when Jev picks from the topic taxonomy on its own.
const JEV_LABELLER: &str = "Jev";

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
        for text in result.representative_texts(c, openrouter::decisions::MAX_POSTS_PER_CLUSTER) {
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

    /// Local stand-in for the MLX chat endpoint: answers each request with
    /// the next canned reply and records the request bodies.
    async fn fake_chat_server(
        replies: Vec<serde_json::Value>,
    ) -> (String, Arc<std::sync::Mutex<Vec<serde_json::Value>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        tokio::spawn(async move {
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let body = loop {
                    let mut chunk = [0u8; 4096];
                    let n = stream.read(&mut chunk).await.unwrap();
                    assert!(n > 0, "request ended early");
                    raw.extend_from_slice(&chunk[..n]);
                    let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                        continue;
                    };
                    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
                    let length: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .map(|v| v.trim().parse().unwrap())
                        .unwrap_or(0);
                    if raw.len() >= split + 4 + length {
                        break raw[split + 4..split + 4 + length].to_vec();
                    }
                };
                seen.lock()
                    .unwrap()
                    .push(serde_json::from_slice(&body).unwrap());
                let reply = reply.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        (url, requests)
    }

    fn chat_reply(content: &str, finish_reason: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {"role": "assistant", "content": content},
                "finish_reason": finish_reason,
            }],
        })
    }

    async fn mlx_complete(
        reply: serde_json::Value,
    ) -> (Result<String, Arc<String>>, Vec<serde_json::Value>) {
        let (url, requests) = fake_chat_server(vec![reply]).await;
        let provider = ChatProvider::Mlx(Arc::new(MlxClient::new(url)));
        let result = provider
            .complete_text("local/model", "Label it.", "posts".into(), 32)
            .await;
        let requests = requests.lock().unwrap().clone();
        (result, requests)
    }

    #[tokio::test]
    async fn mlx_chat_sends_the_budget_and_no_reasoning_setting() {
        let (result, requests) = mlx_complete(chat_reply("Rust tooling", "stop")).await;
        assert_eq!(result.unwrap(), "Rust tooling");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["max_tokens"], 32);
        assert!(requests[0].get("reasoning").is_none());
    }

    #[tokio::test]
    async fn mlx_reply_that_was_all_reasoning_reports_the_budget() {
        // The server already gave the reasoning extra room; asking again
        // would not help.
        let (result, requests) = mlx_complete(chat_reply("", "length")).await;
        let error = result.unwrap_err();
        assert!(error.contains("exhausted token budget"), "{error}");
        assert_eq!(requests.len(), 1);
    }

    fn llama_embedder(url: String, prompts: TaskPrompts) -> EmbedProvider {
        EmbedProvider::LlamaCpp(Arc::new(LlamaCppClient::new(url)), Arc::new(prompts))
    }

    /// Inputs of each `/v1/embeddings` request the fake server saw.
    fn embedding_inputs(seen: &crate::llamacpp::client::tests::Seen) -> Vec<serde_json::Value> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(_, path, _)| path == "/v1/embeddings")
            .map(|(_, _, body)| body["input"].clone())
            .collect()
    }

    #[tokio::test]
    async fn llama_cpp_embeddings_carry_the_configured_task_prompt() {
        use crate::llamacpp::client::tests::{embedding_server, fake_llama_server};
        let (url, seen) = fake_llama_server(embedding_server).await;
        let prompts = TaskPrompts {
            clustering: Some("task: clustering | query: ".into()),
            ..Default::default()
        };
        let provider = llama_embedder(url, prompts);
        let texts = vec!["Rust".to_string(), "Go".to_string()];
        let embs = provider
            .embed("bge-small", &texts, EmbedTask::Clustering)
            .await
            .unwrap();
        assert_eq!(embs.len(), 2);
        assert_eq!(
            embedding_inputs(&seen),
            vec![serde_json::json!([
                "task: clustering | query: Rust",
                "task: clustering | query: Go"
            ])]
        );
    }

    #[tokio::test]
    async fn llama_cpp_search_splits_only_when_query_and_document_prompts_differ() {
        use crate::llamacpp::client::tests::{embedding_server, fake_llama_server};
        let docs = vec!["doc a".to_string(), "doc b".to_string()];

        let (url, seen) = fake_llama_server(embedding_server).await;
        let provider = llama_embedder(url, TaskPrompts::default());
        let (_, doc_embs) = provider
            .embed_query_and_documents("m", "query", &docs)
            .await
            .unwrap();
        assert_eq!(doc_embs.len(), 2);
        assert_eq!(
            embedding_inputs(&seen),
            vec![serde_json::json!(["query", "doc a", "doc b"])]
        );

        let (url, seen) = fake_llama_server(embedding_server).await;
        let prompts = TaskPrompts {
            search_query: Some("q: ".into()),
            document: Some("d: ".into()),
            ..Default::default()
        };
        let provider = llama_embedder(url, prompts);
        provider
            .embed_query_and_documents("m", "query", &docs)
            .await
            .unwrap();
        let mut inputs = embedding_inputs(&seen);
        inputs.sort_by_key(|v| v.to_string());
        assert_eq!(
            inputs,
            vec![
                serde_json::json!(["d: doc a", "d: doc b"]),
                serde_json::json!(["q: query"]),
            ]
        );
    }

    #[tokio::test]
    async fn llama_cpp_does_not_take_photos() {
        let provider = llama_embedder("http://127.0.0.1:9".into(), TaskPrompts::default());
        let docs = vec![(
            "text".to_string(),
            Arc::<str>::from("data:image/jpeg;base64,"),
        )];
        let error = provider
            .embed_documents("m", &docs, EmbedTask::Clustering)
            .await
            .unwrap_err();
        assert!(error.contains("text only"), "{error}");
    }

    #[tokio::test]
    async fn local_servers_come_before_openrouter_and_mlx_before_llama_cpp() {
        use crate::auth::credentials::CredentialSet;
        use crate::config::AppConfig;

        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.openrouter_client = Some(Arc::new(OpenRouterClient::new("key".into())));
        app.selected_chat_model = Some("remote/model".into());
        assert_eq!(app.resolved_chat_provider_name(), Some("OpenRouter"));

        app.llama_cpp.chat = Some(Arc::new(LlamaCppClient::new("http://127.0.0.1:9".into())));
        // Configured but not probed yet: not used.
        assert_eq!(app.resolved_chat_provider_name(), Some("OpenRouter"));
        app.llama_cpp.chat_model = Some("gemma-4-12b".into());
        assert_eq!(app.resolved_chat_provider_name(), Some("llama.cpp"));
        assert_eq!(app.resolved_chat_model().as_deref(), Some("gemma-4-12b"));

        app.mlx_client = Some(Arc::new(MlxClient::new("http://127.0.0.1:9".into())));
        app.mlx_chat_supported = true;
        assert_eq!(app.resolved_chat_provider_name(), Some("MLX"));

        // An explicit preference wins over the order.
        app.preferred_chat_provider = Some(ChatProviderKind::LlamaCpp);
        assert_eq!(app.resolved_chat_provider_name(), Some("llama.cpp"));
    }

    #[tokio::test]
    async fn chat_only_topics_report_their_labeller_before_the_labels() {
        use crate::auth::credentials::CredentialSet;
        use crate::config::AppConfig;
        use crate::embeddings::cluster::ClusterResult;

        let (url, _) = fake_chat_server(vec![chat_reply("Cluster 0: Rust", "stop")]).await;
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.mlx_client = Some(Arc::new(MlxClient::new(url)));
        app.mlx_chat_supported = true;
        app.cluster_result = Some(ClusterResult {
            points: vec![(0.0, 0.0)],
            labels: vec![0],
            tweet_ids: vec!["1".into()],
            tweet_texts: vec!["Async Rust finally clicked".into()],
            conversation_ids: vec![None],
            author_ids: vec![None],
            cluster_topics: vec![String::new()],
            ..Default::default()
        });

        app.start_cluster_topics();
        assert!(app.cluster_topics_loading);
        assert_eq!(app.cluster_topics_labeller, None);

        let mut hints = Vec::new();
        while app.cluster_topics_loading {
            let event = tokio::time::timeout(std::time::Duration::from_secs(5), app.events.next())
                .await
                .expect("topics finish")
                .unwrap();
            if let Event::App(event) = event {
                app.handle_app_event(*event);
                hints.push(app.cluster_topics_labeller.clone());
            }
        }
        assert_eq!(hints, vec![Some("MLX".to_string()), None]);
        assert_eq!(app.cluster_result.unwrap().cluster_topics, vec!["Rust"]);
        assert_eq!(
            app.status_message.as_deref(),
            Some("MLX generated 1/1 topic labels")
        );
    }

    /// Live check against two `llama-server` processes: probe both, embed
    /// and rank a search, and label a cluster. Run with:
    /// `LLAMA_EMBED_URL=http://localhost:8091 LLAMA_CHAT_URL=http://localhost:8092 \
    ///  cargo test llama_cpp_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn llama_cpp_live() {
        use crate::app::llama_cpp::LlamaCppState;
        use crate::config::LlamaCppConfig;
        use crate::embeddings::similarity::cosine_similarity;

        let config = LlamaCppConfig {
            embedding_url: std::env::var("LLAMA_EMBED_URL").ok(),
            chat_url: std::env::var("LLAMA_CHAT_URL").ok(),
            ..Default::default()
        };
        let mut state = LlamaCppState::from_config(Some(&config));
        let found = state.probe().await;
        println!("probe: {found:?}");
        state.apply(found);

        if let (Some(client), Some(model)) = (&state.embed, &state.embed_model) {
            let provider =
                EmbedProvider::LlamaCpp(Arc::clone(client), Arc::clone(&state.task_prompts));
            let docs = vec![
                "Tokio makes async Rust networking fast".to_string(),
                "My cat sleeps in the sun all afternoon".to_string(),
            ];
            let (query, doc_embs) = provider
                .embed_query_and_documents(model, "async runtimes in Rust", &docs)
                .await
                .unwrap();
            let scores: Vec<f64> = doc_embs
                .iter()
                .map(|d| cosine_similarity(&query, d))
                .collect();
            println!("embed: {} dims, scores {scores:?}", query.len());
            assert!(
                scores[0] > scores[1],
                "the Rust post ranks first: {scores:?}"
            );
        }

        if let (Some(client), Some(model)) = (&state.chat, &state.chat_model) {
            let provider = ChatProvider::LlamaCpp(Arc::clone(client));
            let body = "Cluster 0:\n- Tokio 2.0 is out\n- Async Rust finally clicked\n".to_string();
            let labels = chat_labels(&provider, model, body, 1).await.unwrap();
            println!("labels: {labels:?}");
            assert!(!labels[0].is_empty());
        }
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
            ..Default::default()
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
