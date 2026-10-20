//! Photos in the thread view: which photo to show, and encoding it for the
//! terminal off the UI task.

use std::sync::Arc;
use std::time::Duration;

use ratatui::layout::Size;
use ratatui_image::Resize;

use super::App;
use crate::api::types::Tweet;
use crate::event::{AppEvent, Event, PhotoProtocol, ViewKind};
use crate::media::prepare::photos;

/// Size of the thread view's photo box, in terminal cells. The photo keeps
/// its aspect ratio inside it.
pub const PHOTO_COLS: u16 = 48;
pub const PHOTO_ROWS: u16 = 14;

/// A photo waits this long for its download in the thread view.
const VIEW_TIMEOUT: Duration = Duration::from_secs(15);
/// Encoded photos kept for the thread view.
const MAX_PHOTO_VIEWS: usize = 32;

/// The state of one photo in the thread view.
#[derive(Debug, Clone)]
pub enum PhotoView {
    Loading,
    Ready(PhotoProtocol),
    Failed(String),
}

/// The photo the thread view shows now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedPhoto {
    pub url: String,
    /// 0-based position among the post's photos.
    pub index: usize,
    pub count: usize,
}

impl App {
    /// Photo URLs of a tweet, in attachment order.
    pub fn photo_urls(&self, tweet: &Tweet) -> Vec<String> {
        photos(tweet, |key| self.lookup_media(key))
            .filter_map(|m| m.url.clone())
            .collect()
    }

    /// The post whose photos the thread view shows: the selected reply when
    /// it has photos, otherwise the root post (the post the thread was
    /// opened from).
    fn photo_post(&self) -> Option<&Tweet> {
        if !matches!(self.current_view(), Some(ViewKind::Thread(_))) {
            return None;
        }
        self.thread_tweets
            .get(self.selected_index())
            .filter(|reply| !self.photo_urls(reply).is_empty())
            .or(self.thread_root.as_ref())
    }

    /// The photo the thread view shows, if the focused post has any.
    pub fn focused_photo(&self) -> Option<FocusedPhoto> {
        let urls = self.photo_urls(self.photo_post()?);
        let count = urls.len();
        let index = self.photo_index % count.max(1);
        Some(FocusedPhoto {
            url: urls.into_iter().nth(index)?,
            index,
            count,
        })
    }

    /// Show the next (`+1`) or previous (`-1`) photo of the focused post.
    pub(super) fn cycle_photo(&mut self, step: isize) {
        let Some(focused) = self.focused_photo() else {
            return;
        };
        let count = focused.count as isize;
        self.photo_index = (focused.index as isize + step).rem_euclid(count) as usize;
    }

    /// Start encoding the focused photo if nothing has started it yet.
    /// Called on every tick, so it follows selection changes.
    pub(super) fn ensure_focused_photo(&mut self) {
        let Some(focused) = self.focused_photo() else {
            return;
        };
        if self.photo_views.contains_key(&focused.url) {
            return;
        }
        if self.photo_views.len() >= MAX_PHOTO_VIEWS {
            self.photo_views.clear();
        }
        self.photo_views
            .insert(focused.url.clone(), PhotoView::Loading);

        let url = focused.url;
        let cache = Arc::clone(&self.photo_cache);
        let picker = self.picker.clone();
        let sender = self.events.sender();
        tokio::spawn(async move {
            let result = async {
                let photo = cache
                    .get(&url, VIEW_TIMEOUT)
                    .await
                    .map_err(|e| e.to_string())?;
                tokio::task::spawn_blocking(move || {
                    picker
                        .new_protocol(
                            (*photo.image).clone(),
                            Size::new(PHOTO_COLS, PHOTO_ROWS),
                            Resize::Fit(None),
                        )
                        .map(|p| PhotoProtocol(Arc::new(p)))
                        .map_err(|e| e.to_string())
                })
                .await
                .map_err(|e| e.to_string())?
            }
            .await
            .map_err(Arc::new);
            let _ = sender.send(Event::App(Box::new(AppEvent::PhotoReady { url, result })));
        });
    }

    pub(super) fn on_photo_ready(
        &mut self,
        url: String,
        result: crate::event::ApiResult<PhotoProtocol>,
    ) {
        let view = match result {
            Ok(protocol) => PhotoView::Ready(protocol),
            Err(e) => {
                tracing::warn!("photo {url} failed: {e}");
                PhotoView::Failed(e.to_string())
            }
        };
        self.photo_views.insert(url, view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{Attachments, Media};
    use crate::auth::credentials::CredentialSet;
    use crate::config::AppConfig;

    fn photo(key: &str) -> Media {
        Media {
            media_key: key.into(),
            type_: "photo".into(),
            url: Some(format!("https://pbs.twimg.com/media/{key}.jpg")),
            preview_image_url: None,
            width: None,
            height: None,
            alt_text: None,
        }
    }

    fn tweet(id: &str, keys: &[&str]) -> Tweet {
        let mut t: Tweet =
            serde_json::from_value(serde_json::json!({ "id": id, "text": "t" })).unwrap();
        t.attachments = Some(Attachments {
            media_keys: Some(keys.iter().map(|k| k.to_string()).collect()),
            poll_ids: None,
        });
        t
    }

    fn thread_app() -> App {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        for key in ["a", "b", "c"] {
            app.media_cache.insert(key.into(), photo(key));
        }
        app.thread_root = Some(tweet("root", &["a", "b"]));
        app.thread_tweets = vec![tweet("r1", &[]), tweet("r2", &["c"])];
        app.push_view(ViewKind::Thread("root".into()));
        app
    }

    #[tokio::test]
    async fn root_photo_shows_when_the_selected_reply_has_none() {
        let app = thread_app();
        let focused = app.focused_photo().unwrap();
        assert!(focused.url.ends_with("/a.jpg"));
        assert_eq!((focused.index, focused.count), (0, 2));
    }

    #[tokio::test]
    async fn a_selected_reply_with_a_photo_takes_the_focus() {
        let mut app = thread_app();
        app.view_stack.last_mut().unwrap().selected_index = 1;
        assert!(app.focused_photo().unwrap().url.ends_with("/c.jpg"));
    }

    #[tokio::test]
    async fn cycling_wraps_around_the_post_photos() {
        let mut app = thread_app();
        app.cycle_photo(1);
        assert!(app.focused_photo().unwrap().url.ends_with("/b.jpg"));
        app.cycle_photo(1);
        assert!(app.focused_photo().unwrap().url.ends_with("/a.jpg"));
        app.cycle_photo(-1);
        assert!(app.focused_photo().unwrap().url.ends_with("/b.jpg"));
    }

    #[tokio::test]
    async fn no_photo_outside_the_thread_view() {
        let mut app = thread_app();
        app.pop_view();
        assert_eq!(app.focused_photo(), None);
    }
}
