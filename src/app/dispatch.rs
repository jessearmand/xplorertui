use std::sync::Arc;

use super::App;
use crate::api::types::{Includes, Tweet, User};
use crate::event::{ApiResult, AppEvent, Event, ViewKind};
use crate::mlx::client::MlxClient;
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
                // Build texts: query + all tweet texts
                let mut texts: Vec<String> = vec![query_clone.clone()];
                texts.extend(tweets.iter().map(|t| t.text.clone()));

                let resp = provider.embed(&model, &texts).await?;

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

        if embed_provider.is_none() && mlx_fallback.is_none() && openrouter_fallback.is_none() {
            self.events.send(AppEvent::ClusteringComplete(Err(Arc::new(
                "No embedding provider configured. Set mlx_server_url in config \
                 or use :openrouter-auth + :models."
                    .into(),
            ))));
            return;
        }

        let sender = self.events.sender();
        let tweets = self.home_timeline.tweets.clone();

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
                    let _ = sender.send(Event::App(Box::new(AppEvent::ClusteringComplete(Err(
                        Arc::new(
                            "MLX server not reachable and no OpenRouter fallback configured."
                                .into(),
                        ),
                    )))));
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

                let resp = provider.embed(&model, &texts).await?;

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
        let generation = self.cluster_generation;
        let Some((provider, model)) = self.resolve_chat_provider() else {
            self.events.send(AppEvent::ClusterTopicsGenerated(
                generation,
                Err(Arc::new(
                    "No chat provider configured. Set mlx_server_url in config \
                     or use :openrouter-auth + :text-models."
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

        // Build the prompt: collect up to 8 representative tweets per cluster.
        let num_clusters = result.num_clusters();
        let max_tokens = Some(cluster_topic_max_tokens(num_clusters));
        let sender = self.events.sender();
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
                use crate::openrouter::types::ReasoningConfig;
                let resp = provider
                    .chat_completion(
                        &model,
                        messages,
                        max_tokens,
                        Some(0.3),
                        Some(ReasoningConfig { exclude: true }),
                    )
                    .await?;

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
                generation, result,
            ))));
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
        self.resolve_chat_provider().map(|(p, _)| match p {
            ChatProvider::Mlx(_) => "MLX",
            ChatProvider::OpenRouter(_) => "OpenRouter",
        })
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
    async fn embed(&self, model: &str, texts: &[String]) -> Result<EmbeddingResponse, Arc<String>> {
        match self {
            Self::OpenRouter(client) => client
                .embed(model, texts)
                .await
                .map_err(|e| Arc::new(e.to_string())),
            Self::Mlx(client) => client
                .embed(model, texts)
                .await
                .map_err(|e| Arc::new(e.to_string())),
        }
    }
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
}
