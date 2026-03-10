use linfa::prelude::*;
use linfa_clustering::KMeans;
use ndarray::Array2;

/// Result of clustering tweet embeddings.
#[derive(Debug, Clone)]
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

/// For each cluster, find the tweet whose embedding is closest to the centroid.
fn closest_to_centroid(
    embeddings: &[Vec<f64>],
    labels: &[usize],
    tweet_texts: &[String],
    k: usize,
) -> Vec<String> {
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

    // For each cluster, find the member with highest cosine similarity to centroid.
    let mut topics = vec![String::new(); k];
    for c in 0..k {
        let centroid = &sums[c];
        let mut best_idx = None;
        let mut best_sim = f64::NEG_INFINITY;
        for (i, &label) in labels.iter().enumerate() {
            if label == c {
                let sim = super::similarity::cosine_similarity(centroid, &embeddings[i]);
                if sim > best_sim {
                    best_sim = sim;
                    best_idx = Some(i);
                }
            }
        }
        if let Some(idx) = best_idx {
            let text = &tweet_texts[idx];
            topics[c] = if text.len() > 140 {
                format!("{}...", &text[..137])
            } else {
                text.clone()
            };
        }
    }
    topics
}

/// Build a complete `ClusterResult` from embeddings and tweet texts.
pub fn build_cluster_result(
    embeddings: &[Vec<f64>],
    tweet_texts: Vec<String>,
    tweet_ids: Vec<String>,
    conversation_ids: Vec<Option<String>>,
    author_ids: Vec<Option<String>>,
    k: usize,
) -> ClusterResult {
    let labels = run_kmeans(embeddings, k);
    let points = super::reduce::pca_2d(embeddings);
    let cluster_topics = closest_to_centroid(embeddings, &labels, &tweet_texts, k);

    ClusterResult {
        points,
        labels,
        tweet_texts,
        tweet_ids,
        conversation_ids,
        author_ids,
        cluster_topics,
    }
}
