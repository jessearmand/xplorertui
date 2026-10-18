//! Download, prepare and keep X photos, so the thread view, Jev and the
//! embedding models share one download per photo.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use image::DynamicImage;
use thiserror::Error;

use super::prepare;

/// Photos kept in memory. A decoded 680 px photo takes about 1.4 MB.
const CAPACITY: usize = 64;

/// In a batch, a photo download slower than this is skipped, so one slow
/// photo does not hold the batch. Most X photos arrive in under 0.1 s;
/// a few take 10 s or more.
pub const BATCH_TIMEOUT: Duration = Duration::from_secs(3);
/// Photo downloads in flight at once in a batch.
pub const BATCH_CONCURRENCY: usize = 8;

/// One photo, decoded once and encoded once.
#[derive(Debug)]
pub struct PreparedPhoto {
    /// Upright pixels of the `name=small` variant, for the terminal.
    pub image: Arc<DynamicImage>,
    /// Baseline JPEG inside Jev's size limits, as a `data:` URL.
    pub data_url: Arc<str>,
}

#[derive(Debug, Error)]
pub enum PhotoError {
    #[error("download failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("download took longer than {0:?}")]
    Timeout(Duration),
    #[error("not a readable image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("image task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// A bounded, shared cache of prepared photos, keyed by the photo URL from
/// the X API. Least recently used photos go first.
pub struct PhotoCache {
    http: reqwest::Client,
    entries: Mutex<Lru>,
}

#[derive(Default)]
struct Lru {
    map: HashMap<String, Arc<PreparedPhoto>>,
    order: VecDeque<String>,
}

impl Lru {
    fn get(&mut self, key: &str) -> Option<Arc<PreparedPhoto>> {
        let photo = self.map.get(key).cloned()?;
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_string());
        Some(photo)
    }

    fn insert(&mut self, key: String, photo: Arc<PreparedPhoto>) {
        if self.map.insert(key.clone(), photo).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CAPACITY {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }
}

impl Default for PhotoCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PhotoCache {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            entries: Mutex::new(Lru::default()),
        }
    }

    fn cached(&self, url: &str) -> Option<Arc<PreparedPhoto>> {
        self.entries.lock().ok()?.get(url)
    }

    /// The prepared photo for an X photo URL. Downloads the `name=small`
    /// variant on a miss and gives up after `timeout`.
    pub async fn get(
        &self,
        url: &str,
        timeout: Duration,
    ) -> Result<Arc<PreparedPhoto>, PhotoError> {
        if let Some(photo) = self.cached(url) {
            return Ok(photo);
        }
        let download = async {
            let resp = self
                .http
                .get(prepare::small_variant_url(url))
                .send()
                .await?
                .error_for_status()?;
            resp.bytes().await
        };
        let bytes = tokio::time::timeout(timeout, download)
            .await
            .map_err(|_| PhotoError::Timeout(timeout))??;
        let photo = tokio::task::spawn_blocking(move || {
            let image = prepare::decode_upright(&bytes)?;
            let data_url = prepare::jpeg_data_url(&prepare::fit_for_jev(&image))?;
            Ok::<_, image::ImageError>(PreparedPhoto {
                image: Arc::new(image),
                data_url: data_url.into(),
            })
        })
        .await??;
        let photo = Arc::new(photo);
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(url.to_string(), Arc::clone(&photo));
        }
        Ok(photo)
    }

    /// Prepare many photos at once, `concurrency` downloads at a time.
    /// Results keep input order; a photo that fails or times out is `None`,
    /// so one slow photo costs at most `timeout`.
    pub async fn get_many(
        &self,
        urls: Vec<String>,
        timeout: Duration,
        concurrency: usize,
    ) -> Vec<Option<Arc<PreparedPhoto>>> {
        // Owned URLs: a stream over borrowed ones is not `Send` enough for
        // `tokio::spawn`.
        futures::stream::iter(urls)
            .map(|url: String| async move {
                match self.get(&url, timeout).await {
                    Ok(photo) => Some(photo),
                    Err(e) => {
                        tracing::debug!("photo {url} skipped: {e}");
                        None
                    }
                }
            })
            .buffered(concurrency.max(1))
            .collect()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo() -> Arc<PreparedPhoto> {
        Arc::new(PreparedPhoto {
            image: Arc::new(DynamicImage::new_rgb8(1, 1)),
            data_url: "data:".into(),
        })
    }

    #[test]
    fn lru_drops_the_least_recently_used_photo() {
        let mut lru = Lru::default();
        for i in 0..CAPACITY {
            lru.insert(format!("u{i}"), photo());
        }
        // Touch the oldest so the second oldest goes instead.
        assert!(lru.get("u0").is_some());
        lru.insert("new".into(), photo());
        assert!(lru.get("u0").is_some());
        assert!(lru.get("u1").is_none());
        assert_eq!(lru.map.len(), CAPACITY);
        assert_eq!(lru.order.len(), CAPACITY);
    }

    #[test]
    fn reinserting_a_key_does_not_duplicate_it() {
        let mut lru = Lru::default();
        lru.insert("a".into(), photo());
        lru.insert("a".into(), photo());
        assert_eq!(lru.order.len(), 1);
    }
}
