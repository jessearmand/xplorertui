use linfa::prelude::*;
use linfa_reduction::Pca;
use ndarray::Array2;

/// Project high-dimensional embeddings to 2D using PCA.
///
/// Returns a `(x, y)` coordinate for each input embedding.
pub fn pca_2d(embeddings: &[Vec<f64>]) -> Vec<(f64, f64)> {
    if embeddings.is_empty() {
        return Vec::new();
    }

    let n = embeddings.len();
    let dim = embeddings[0].len();

    // PCA needs at least 2 dimensions and 2 samples.
    if n < 2 || dim < 2 {
        return vec![(0.0, 0.0); n];
    }

    let flat: Vec<f64> = embeddings.iter().flatten().copied().collect();
    let array = Array2::from_shape_vec((n, dim), flat).expect("shape matches flattened data");
    let dataset = DatasetBase::from(array);

    let pca = Pca::params(2).fit(&dataset).expect("PCA fitted");
    let reduced = pca.transform(dataset);
    let records = reduced.records();

    (0..n).map(|i| (records[[i, 0]], records[[i, 1]])).collect()
}
