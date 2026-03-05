/// Compute the cosine similarity between two vectors.
///
/// Returns 0.0 if either vector has zero magnitude.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

/// Rank items by cosine similarity to a query embedding.
///
/// Takes `(index, embedding)` pairs and returns `(index, score)` sorted
/// descending by similarity score.
pub fn rank_by_similarity(query: &[f64], items: &[(usize, Vec<f64>)]) -> Vec<(usize, f64)> {
    let mut scored: Vec<(usize, f64)> = items
        .iter()
        .map(|(idx, emb)| (*idx, cosine_similarity(query, emb)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_vectors_have_similarity_one() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-10);
    }

    #[test]
    fn orthogonal_vectors_have_similarity_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-10);
    }

    #[test]
    fn rank_returns_descending_order() {
        let query = vec![1.0, 0.0];
        let items = vec![
            (0, vec![0.0, 1.0]), // orthogonal
            (1, vec![1.0, 0.0]), // identical
            (2, vec![1.0, 1.0]), // partial
        ];
        let ranked = rank_by_similarity(&query, &items);
        assert_eq!(ranked[0].0, 1); // most similar
        assert_eq!(ranked[2].0, 0); // least similar
    }
}
