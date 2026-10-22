use linfa::prelude::*;
use linfa_clustering::KMeans;
use ndarray::Array2;

/// Fewest clusters `:cluster` makes when there are enough posts.
pub const MIN_CLUSTERS: usize = 4;
/// Most clusters `:cluster` makes: the number of cluster colours in the UI.
pub const MAX_CLUSTERS: usize = 10;

/// A cluster gets photo embeddings when more than this share of its posts
/// are photos with thin text: their text vectors say little, so they group
/// by having no text rather than by what they show.
pub const PHOTO_HEAVY_SHARE: f64 = 0.8;

/// Number of clusters for `n` posts: √(n/2), between `MIN_CLUSTERS` and
/// `MAX_CLUSTERS`, and never more than `n`. 20 posts give 4, 50 give 5,
/// 100 give 7, 200 give 10. A configured value replaces the rule; it is
/// capped the same way.
pub fn cluster_k(n: usize, configured: Option<usize>) -> usize {
    let k = configured.unwrap_or_else(|| ((n as f64 / 2.0).sqrt().round()) as usize);
    k.clamp(MIN_CLUSTERS, MAX_CLUSTERS).min(n)
}

/// The posts to cluster, as parallel vectors (one entry per post).
#[derive(Debug, Clone, Default)]
pub struct ClusterPosts {
    pub texts: Vec<String>,
    pub ids: Vec<String>,
    pub conversation_ids: Vec<Option<String>>,
    pub author_ids: Vec<Option<String>>,
    /// URL of each post's first photo, from the X API.
    pub photo_urls: Vec<Option<String>>,
}

/// Result of clustering tweet embeddings.
#[derive(Debug, Clone, Default)]
pub struct ClusterResult {
    /// PCA-projected 2D coordinates for each tweet.
    pub points: Vec<(f64, f64)>,
    /// Cluster label for each tweet.
    pub labels: Vec<usize>,
    /// Original tweet texts.
    pub tweet_texts: Vec<String>,
    /// Tweet ID for each entry (parallel to `tweet_texts`).
    pub tweet_ids: Vec<String>,
    /// Conversation ID per tweet (for opening threads).
    pub conversation_ids: Vec<Option<String>>,
    /// Author ID per tweet (for building tweet URLs).
    pub author_ids: Vec<Option<String>>,
    /// Representative label per cluster (tweet closest to centroid).
    pub cluster_topics: Vec<String>,
    /// URL of each tweet's first photo (parallel to `tweet_texts`).
    pub photo_urls: Vec<Option<String>>,
    /// Per cluster, its tweet indices nearest the centroid first. Empty
    /// when the result was not built from embeddings.
    pub representatives: Vec<Vec<usize>>,
    /// Whether photo-heavy clusters got photo embeddings.
    pub photo_embedding: PhotoEmbedding,
}

/// What the photo-embedding step did for one clustering run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PhotoEmbedding {
    /// No cluster was photo-heavy.
    #[default]
    NotNeeded,
    /// This many posts got a vector of their text and photo, and k-means
    /// ran again on all posts.
    Applied(usize),
    /// Photo-heavy clusters exist, but the step did not run (for example,
    /// the text model cannot read photos).
    Skipped(String),
}

impl ClusterResult {
    /// Get the 2D points belonging to a specific cluster.
    pub fn points_for_cluster(&self, cluster: usize) -> Vec<(f64, f64)> {
        self.points
            .iter()
            .zip(self.labels.iter())
            .filter(|(_, label)| **label == cluster)
            .map(|(pt, _)| *pt)
            .collect()
    }

    /// Number of distinct clusters.
    pub fn num_clusters(&self) -> usize {
        self.labels.iter().copied().max().map_or(0, |m| m + 1)
    }

    /// Returns original indices of tweets belonging to a cluster.
    pub fn tweet_indices_for_cluster(&self, cluster: usize) -> Vec<usize> {
        self.labels
            .iter()
            .enumerate()
            .filter(|(_, label)| **label == cluster)
            .map(|(i, _)| i)
            .collect()
    }

    /// Up to `n` tweet indices of a cluster, nearest the centroid first.
    /// Falls back to timeline order when there is no centroid ranking.
    pub fn representative_indices(&self, cluster: usize, n: usize) -> Vec<usize> {
        match self.representatives.get(cluster) {
            Some(order) if !order.is_empty() => order.iter().copied().take(n).collect(),
            _ => self
                .tweet_indices_for_cluster(cluster)
                .into_iter()
                .take(n)
                .collect(),
        }
    }

    /// Up to `n` texts of a cluster, nearest the centroid first.
    pub fn representative_texts(&self, cluster: usize, n: usize) -> Vec<&str> {
        self.representative_indices(cluster, n)
            .into_iter()
            .map(|i| self.tweet_texts[i].as_str())
            .collect()
    }

    /// Whether tweet `i` has a photo and thin text.
    pub fn wants_photo(&self, i: usize) -> bool {
        self.photo_urls.get(i).is_some_and(Option::is_some)
            && crate::media::prepare::is_thin_text(&self.tweet_texts[i])
    }

    /// Clusters where more than `share` of the posts want their photo.
    pub fn photo_heavy_clusters(&self, share: f64) -> Vec<usize> {
        (0..self.num_clusters())
            .filter(|&c| {
                let members = self.tweet_indices_for_cluster(c);
                let photo = members.iter().filter(|&&i| self.wants_photo(i)).count();
                !members.is_empty() && photo as f64 > share * members.len() as f64
            })
            .collect()
    }

    /// Returns `(original_index, text)` pairs for tweets in a cluster.
    pub fn texts_for_cluster(&self, cluster: usize) -> Vec<(usize, &str)> {
        self.labels
            .iter()
            .enumerate()
            .filter(|(_, label)| **label == cluster)
            .map(|(i, _)| (i, self.tweet_texts[i].as_str()))
            .collect()
    }
}

/// Run k-means clustering on a set of embedding vectors.
///
/// Returns a cluster label for each input embedding.
pub fn run_kmeans(embeddings: &[Vec<f64>], k: usize) -> Vec<usize> {
    if embeddings.is_empty() {
        return Vec::new();
    }

    let n = embeddings.len();
    let dim = embeddings[0].len();
    let flat: Vec<f64> = embeddings.iter().flatten().copied().collect();
    let array = Array2::from_shape_vec((n, dim), flat).expect("shape matches flattened data");
    let dataset = DatasetBase::from(array);

    let model = KMeans::params(k)
        .max_n_iterations(200)
        .tolerance(1e-5)
        .fit(&dataset)
        .expect("KMeans fitted");

    let result = model.predict(dataset);
    result.targets().to_vec()
}

/// Per cluster, its member indices sorted by cosine similarity to the
/// cluster centroid, most similar first.
fn rank_by_centroid(embeddings: &[Vec<f64>], labels: &[usize], k: usize) -> Vec<Vec<usize>> {
    let dim = embeddings.first().map_or(0, |e| e.len());

    // Compute centroid (mean embedding) per cluster.
    let mut sums = vec![vec![0.0f64; dim]; k];
    let mut counts = vec![0usize; k];
    for (i, &label) in labels.iter().enumerate() {
        if label < k {
            counts[label] += 1;
            for (j, val) in embeddings[i].iter().enumerate() {
                sums[label][j] += val;
            }
        }
    }
    for c in 0..k {
        if counts[c] > 0 {
            let n = counts[c] as f64;
            for val in &mut sums[c] {
                *val /= n;
            }
        }
    }

    (0..k)
        .map(|c| {
            let mut members: Vec<(usize, f64)> = labels
                .iter()
                .enumerate()
                .filter(|&(_, &label)| label == c)
                .map(|(i, _)| {
                    let sim = super::similarity::cosine_similarity(&sums[c], &embeddings[i]);
                    (i, sim)
                })
                .collect();
            members.sort_by(|a, b| b.1.total_cmp(&a.1));
            members.into_iter().map(|(i, _)| i).collect()
        })
        .collect()
}

/// Placeholder topic per cluster: the tweet nearest its centroid.
fn centroid_topics(representatives: &[Vec<usize>], tweet_texts: &[String]) -> Vec<String> {
    representatives
        .iter()
        .map(|order| {
            let Some(&idx) = order.first() else {
                return String::new();
            };
            let text = &tweet_texts[idx];
            if text.len() > 140 {
                let truncated: String = text.chars().take(137).collect();
                format!("{truncated}...")
            } else {
                text.clone()
            }
        })
        .collect()
}

/// Build a complete `ClusterResult` from embeddings, one per post.
pub fn build_cluster_result(
    embeddings: &[Vec<f64>],
    posts: ClusterPosts,
    k: usize,
) -> ClusterResult {
    let labels = run_kmeans(embeddings, k);
    let points = super::reduce::pca_2d(embeddings);
    let representatives = rank_by_centroid(embeddings, &labels, k);
    let cluster_topics = centroid_topics(&representatives, &posts.texts);

    ClusterResult {
        points,
        labels,
        tweet_texts: posts.texts,
        tweet_ids: posts.ids,
        conversation_ids: posts.conversation_ids,
        author_ids: posts.author_ids,
        cluster_topics,
        photo_urls: posts.photo_urls,
        representatives,
        photo_embedding: PhotoEmbedding::NotNeeded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k_follows_the_square_root_rule_within_bounds() {
        assert_eq!(cluster_k(20, None), 4);
        assert_eq!(cluster_k(50, None), 5);
        assert_eq!(cluster_k(100, None), 7);
        assert_eq!(cluster_k(200, None), 10);
        assert_eq!(cluster_k(1000, None), MAX_CLUSTERS);
    }

    #[test]
    fn k_never_exceeds_the_post_count() {
        assert_eq!(cluster_k(3, None), 3);
        assert_eq!(cluster_k(0, None), 0);
    }

    #[test]
    fn configured_k_is_capped_like_the_rule() {
        assert_eq!(cluster_k(100, Some(8)), 8);
        assert_eq!(cluster_k(100, Some(30)), MAX_CLUSTERS);
        assert_eq!(cluster_k(100, Some(1)), MIN_CLUSTERS);
    }

    fn two_clusters() -> ClusterResult {
        // Cluster 0: three photo posts with thin text and one text post.
        // Cluster 1: two photo posts with long text.
        let long = "a long post that says plenty about its subject in its own words";
        ClusterResult {
            labels: vec![0, 0, 0, 0, 1, 1],
            tweet_texts: vec![
                "ラーメン".into(),
                "#food https://t.co/x".into(),
                "lunch".into(),
                long.into(),
                long.into(),
                long.into(),
            ],
            photo_urls: vec![
                Some("p0".into()),
                Some("p1".into()),
                Some("p2".into()),
                None,
                Some("p4".into()),
                Some("p5".into()),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn photo_heavy_needs_more_than_the_share() {
        let result = two_clusters();
        // Cluster 0 is 3/4 = 0.75 photo-with-thin-text.
        assert_eq!(result.photo_heavy_clusters(0.8), Vec::<usize>::new());
        assert_eq!(result.photo_heavy_clusters(0.7), vec![0]);
    }

    #[test]
    fn long_text_photo_posts_do_not_count() {
        let result = two_clusters();
        assert!(!result.wants_photo(4));
        assert!(result.wants_photo(1));
    }

    #[test]
    fn representatives_come_nearest_the_centroid_first() {
        let embeddings = vec![
            vec![1.0, 0.0],
            vec![0.8, 0.6],
            vec![0.6, 0.8],
            vec![0.0, 1.0],
        ];
        let order = rank_by_centroid(&embeddings, &[0, 0, 0, 0], 1);
        // The centroid points along (0.6, 0.6): the middle two are nearest.
        assert_eq!(&order[0][2..], &[0, 3]);
        assert!(order[0][..2].contains(&1) && order[0][..2].contains(&2));
    }

    #[test]
    fn representative_texts_fall_back_to_timeline_order() {
        let result = two_clusters();
        assert_eq!(
            result.representative_texts(1, 8),
            vec![result.tweet_texts[4].as_str(); 2]
        );
    }
}
