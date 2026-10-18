//! `:cluster`: collect posts, embed them, run k-means, and give photo-heavy
//! clusters photo embeddings.

use std::sync::Arc;

use super::dispatch::{DEFAULT_MLX_EMBEDDING_MODEL, EmbedProvider};
use super::{App, ClusterSource};
use crate::api::types::Tweet;
use crate::embeddings::cluster::{
    ClusterPosts, ClusterResult, PHOTO_HEAVY_SHARE, PhotoEmbedding, build_cluster_result, cluster_k,
};
use crate::event::{AppEvent, Event};
use crate::media::PhotoCache;
use crate::media::cache::{BATCH_CONCURRENCY, BATCH_TIMEOUT};
use crate::media::prepare::first_photo;
use crate::mlx::client::EmbedTask;

/// Most posts one X API read returns.
const MAX_PAGE_SIZE: usize = 100;
/// Fewest posts the X API accepts as `max_results` on search.
const MIN_PAGE_SIZE: usize = 10;

impl App {
    /// The fetch that brings `source` up to `config.cluster_posts` posts, or
    /// `None` when enough are loaded or there is nothing more to load.
    pub(super) fn cluster_fill_request(&self, source: ClusterSource) -> Option<AppEvent> {
        let target = self.config.cluster_posts;
        let (loaded, next_token) = match source {
            ClusterSource::Home => (
                self.home_timeline.tweets.len(),
                &self.home_timeline.next_token,
            ),
            ClusterSource::Mentions => (self.mentions.tweets.len(), &self.mentions.next_token),
            ClusterSource::Bookmarks => (self.bookmarks.tweets.len(), &self.bookmarks.next_token),
            ClusterSource::Search => (self.search_results.tweets.len(), &None),
        };
        if loaded >= target {
            return None;
        }
        let (page_size, pagination_token) = match source {
            // A search page replaces the results, so ask again from the
            // start for the whole target.
            ClusterSource::Search if !self.search_query.is_empty() => (target, None),
            ClusterSource::Search => return None,
            // The other sources append the next page.
            _ => (target - loaded, Some(next_token.clone()?)),
        };
        Some(AppEvent::FetchForCluster {
            source,
            page_size: page_size.clamp(MIN_PAGE_SIZE, MAX_PAGE_SIZE) as u32,
            pagination_token,
        })
    }

    /// The posts of `tweets`, with each post's first photo URL.
    fn cluster_posts(&self, tweets: &[Tweet]) -> ClusterPosts {
        ClusterPosts {
            texts: tweets.iter().map(|t| t.text.clone()).collect(),
            ids: tweets.iter().map(|t| t.id.clone()).collect(),
            conversation_ids: tweets.iter().map(|t| t.conversation_id.clone()).collect(),
            author_ids: tweets.iter().map(|t| t.author_id.clone()).collect(),
            photo_urls: tweets
                .iter()
                .map(|t| first_photo(t, |key| self.lookup_media(key)).and_then(|m| m.url.clone()))
                .collect(),
        }
    }

    /// Whether the selected OpenRouter embedding model reads images, from
    /// the model list. `None` when the list does not say.
    fn openrouter_embed_reads_images(&self) -> Option<bool> {
        let model_id = self.selected_embedding_model.as_ref()?;
        let model = self.openrouter_models.iter().find(|m| &m.id == model_id)?;
        let modalities = model.architecture.as_ref()?.input_modalities.as_ref()?;
        Some(modalities.iter().any(|m| m == "image"))
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
            ClusterSource::Home => &self.home_timeline.tweets,
            ClusterSource::Mentions => &self.mentions.tweets,
            ClusterSource::Search => &self.search_results.tweets,
            ClusterSource::Bookmarks => &self.bookmarks.tweets,
        };
        let posts = self.cluster_posts(tweets);
        let k = cluster_k(posts.texts.len(), self.config.cluster_k);
        let openrouter_reads_images = self.openrouter_embed_reads_images();
        let photo_cache = Arc::clone(&self.photo_cache);

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
            let reads_images = match provider {
                EmbedProvider::OpenRouter(_) => openrouter_reads_images,
                // The MLX server answers 400 when the model has no vision tower.
                EmbedProvider::Mlx(_) => None,
            };
            let result = async {
                let embeddings = provider
                    .embed(&model, &posts.texts, EmbedTask::Clustering)
                    .await?;
                let result = build_cluster_result(&embeddings, posts.clone(), k);
                let embedder = PhotoEmbedder {
                    provider: &provider,
                    model: &model,
                    reads_images,
                    photos: &photo_cache,
                };
                Ok(embedder.apply(result, embeddings, posts, k).await)
            }
            .await;

            let _ = sender.send(Event::App(Box::new(AppEvent::ClusteringComplete(result))));
        });
    }
}

/// Gives the posts of photo-heavy clusters a vector of their text and photo,
/// then runs k-means again on all posts with the same k.
///
/// Text and photo vectors must come from the same model to be comparable,
/// so the photo vectors come from the model that embedded the text. A model
/// that cannot read images skips the step.
struct PhotoEmbedder<'a> {
    provider: &'a EmbedProvider,
    model: &'a str,
    /// `Some(false)` when the model is known not to read images.
    reads_images: Option<bool>,
    photos: &'a PhotoCache,
}

impl PhotoEmbedder<'_> {
    async fn apply(
        &self,
        result: ClusterResult,
        mut embeddings: Vec<Vec<f64>>,
        posts: ClusterPosts,
        k: usize,
    ) -> ClusterResult {
        let heavy = result.photo_heavy_clusters(PHOTO_HEAVY_SHARE);
        if heavy.is_empty() {
            return result;
        }
        if self.reads_images == Some(false) {
            return skipped(result, format!("{} cannot read photos", self.model));
        }

        let indices: Vec<usize> = heavy
            .iter()
            .flat_map(|&c| result.tweet_indices_for_cluster(c))
            .filter(|&i| result.wants_photo(i))
            .collect();
        let urls: Vec<String> = indices
            .iter()
            .filter_map(|&i| result.photo_urls[i].clone())
            .collect();
        let photos = self
            .photos
            .get_many(urls, BATCH_TIMEOUT, BATCH_CONCURRENCY)
            .await;
        let (targets, documents): (Vec<usize>, Vec<(String, Arc<str>)>) = indices
            .iter()
            .zip(photos)
            .filter_map(|(&i, photo)| {
                let photo = photo?;
                Some((i, (posts.texts[i].clone(), Arc::clone(&photo.data_url))))
            })
            .unzip();
        if documents.is_empty() {
            return skipped(result, "no photo could be downloaded".into());
        }

        let vectors = match self
            .provider
            .embed_documents(self.model, &documents, EmbedTask::Clustering)
            .await
        {
            Ok(vectors) => vectors,
            Err(e) => return skipped(result, e.to_string()),
        };
        let dim = embeddings.first().map_or(0, Vec::len);
        if vectors.iter().any(|v| v.len() != dim) {
            return skipped(
                result,
                "photo vectors and text vectors differ in size".into(),
            );
        }
        for (&i, vector) in targets.iter().zip(vectors) {
            embeddings[i] = vector;
        }
        let mut rebuilt = build_cluster_result(&embeddings, posts, k);
        rebuilt.photo_embedding = PhotoEmbedding::Applied(targets.len());
        rebuilt
    }
}

/// `result` unchanged, with the reason the photo step did not run.
fn skipped(result: ClusterResult, reason: String) -> ClusterResult {
    tracing::info!("photo embeddings skipped: {reason}");
    ClusterResult {
        photo_embedding: PhotoEmbedding::Skipped(reason),
        ..result
    }
}

/// Photo URLs to send with each cluster's pick request, in the order of
/// the cluster's representative posts. Only posts with thin text send
/// their photo, within the per-cluster and per-run budgets.
pub(super) fn topic_photo_urls(
    result: &ClusterResult,
    posts_per_cluster: usize,
) -> Vec<Vec<Option<String>>> {
    use crate::openrouter::decision_photos::{
        MAX_PHOTOS_PER_CLUSTER, MAX_PHOTOS_PER_RUN, select_photos,
    };

    let representatives: Vec<Vec<usize>> = (0..result.num_clusters())
        .map(|c| result.representative_indices(c, posts_per_cluster))
        .collect();
    let wants: Vec<Vec<bool>> = representatives
        .iter()
        .map(|idx| idx.iter().map(|&i| result.wants_photo(i)).collect())
        .collect();
    let chosen = select_photos(&wants, MAX_PHOTOS_PER_CLUSTER, MAX_PHOTOS_PER_RUN);
    representatives
        .iter()
        .zip(chosen)
        .map(|(idx, chosen)| {
            idx.iter()
                .zip(chosen)
                .map(|(&i, yes)| yes.then(|| result.photo_urls[i].clone()).flatten())
                .collect()
        })
        .collect()
}

/// Download the photos of `topic_photo_urls` and return their `data:` URLs
/// in the same shape. A photo that fails or times out is `None`.
pub(super) async fn fetch_topic_photos(
    cache: Arc<PhotoCache>,
    urls: Vec<Vec<Option<String>>>,
) -> Vec<Vec<Option<Arc<str>>>> {
    let flat: Vec<String> = urls.iter().flatten().flatten().cloned().collect();
    let mut fetched = cache
        .get_many(flat, BATCH_TIMEOUT, BATCH_CONCURRENCY)
        .await
        .into_iter();
    urls.iter()
        .map(|cluster| {
            cluster
                .iter()
                .map(|url| {
                    url.as_ref()?;
                    fetched.next().flatten().map(|p| Arc::clone(&p.data_url))
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::credentials::CredentialSet;
    use crate::config::AppConfig;

    fn tweets(n: usize) -> Vec<Tweet> {
        (0..n)
            .map(|i| {
                serde_json::from_value(serde_json::json!({"id": i.to_string(), "text": "t"}))
                    .unwrap()
            })
            .collect()
    }

    #[tokio::test]
    async fn fill_asks_for_the_rest_of_the_target_from_the_next_page() {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.home_timeline.tweets = tweets(20);
        app.home_timeline.next_token = Some("next".into());
        match app.cluster_fill_request(ClusterSource::Home) {
            Some(AppEvent::FetchForCluster {
                page_size,
                pagination_token,
                ..
            }) => {
                assert_eq!(page_size, 80);
                assert_eq!(pagination_token.as_deref(), Some("next"));
            }
            other => panic!("expected a fill fetch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_fill_at_the_target_or_without_more_pages() {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.home_timeline.tweets = tweets(100);
        app.home_timeline.next_token = Some("next".into());
        assert!(app.cluster_fill_request(ClusterSource::Home).is_none());
        app.mentions.tweets = tweets(20);
        assert!(app.cluster_fill_request(ClusterSource::Mentions).is_none());
    }

    #[tokio::test]
    async fn search_fill_asks_again_from_the_start() {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.search_results.tweets = tweets(20);
        assert!(
            app.cluster_fill_request(ClusterSource::Search).is_none(),
            "no query"
        );
        app.search_query = "rust".into();
        match app.cluster_fill_request(ClusterSource::Search) {
            Some(AppEvent::FetchForCluster {
                page_size,
                pagination_token,
                ..
            }) => {
                assert_eq!((page_size, pagination_token), (100, None));
            }
            other => panic!("expected a fill fetch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn start_cluster_fills_once_then_clusters() {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.home_timeline.tweets = tweets(20);
        app.home_timeline.next_token = Some("next".into());
        app.handle_app_event(AppEvent::ClusterTimeline);
        assert!(app.cluster_fill_in_flight && app.refresh_then_cluster);
        assert!(!app.cluster_loading);
        // The page arrives (no client: as an error); clustering starts with
        // the posts it has and does not fetch again.
        app.handle_app_event(AppEvent::HomeTimelineLoaded(Err(Arc::new(
            "offline".into(),
        ))));
        assert!(!app.cluster_fill_in_flight);
        assert!(!app.refresh_then_cluster);
    }

    #[test]
    fn topic_photos_follow_the_representative_order_and_budget() {
        let result = ClusterResult {
            labels: vec![0, 0, 0, 1],
            tweet_texts: vec![
                "ramen".into(),
                "a long post that needs no photo at all, because its words carry the whole point"
                    .into(),
                "lunch".into(),
                "cat".into(),
            ],
            photo_urls: vec![
                Some("p0".into()),
                Some("p1".into()),
                Some("p2".into()),
                Some("p3".into()),
            ],
            representatives: vec![vec![2, 1, 0], vec![3]],
            ..Default::default()
        };
        let urls = topic_photo_urls(&result, 8);
        assert_eq!(urls[0], vec![Some("p2".into()), None, Some("p0".into())]);
        assert_eq!(urls[1], vec![Some("p3".into())]);
    }
}
