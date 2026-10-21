use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};

use crate::api::types::Tweet;
use crate::app::App;
use crate::app::photos::PHOTO_ROWS;
use crate::ui::photo::PhotoPanel;

/// Rows kept for replies below the photo, so a tall photo cannot push
/// every reply out of a short window.
const REPLY_ROWS: u16 = 6;
use crate::ui::tweet::{TweetCard, tweet_card_height};

/// Thread/conversation view: root tweet at top, replies below.
pub struct ThreadView<'a> {
    pub root: Option<&'a Tweet>,
    pub replies: &'a [Tweet],
    pub selected_index: usize,
    pub app: &'a App,
}

impl<'a> ThreadView<'a> {
    pub fn new(root: Option<&'a Tweet>, replies: &'a [Tweet], app: &'a App) -> Self {
        Self {
            root,
            replies,
            selected_index: app.selected_index(),
            app,
        }
    }
}

impl Widget for ThreadView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Thread ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::DarkGray));

        let inner = block.inner(area);
        block.render(area, buf);

        let content_width = inner.width.saturating_sub(1);
        let mut y = inner.y;

        // Render root tweet (if available)
        if let Some(root) = self.root {
            let root_h = tweet_card_height(root, content_width);
            let remaining = (inner.y + inner.height).saturating_sub(y);
            let render_h = root_h.min(remaining);

            if render_h > 0 {
                let root_area = Rect::new(inner.x + 1, y, content_width, render_h);
                let author = root
                    .author_id
                    .as_ref()
                    .and_then(|id| self.app.lookup_user(id));
                TweetCard::new(root, author).render(root_area, buf);
                y += render_h;
            }
        }

        // The focused post's photo, above the replies.
        let mut photo_shown = false;
        if let Some(focused) = self.app.focused_photo() {
            let remaining = (inner.y + inner.height).saturating_sub(y);
            // Below the photo: its caption, the separator, then the replies
            // (or the one-line "No replies").
            let below = 2 + if self.replies.is_empty() {
                1
            } else {
                REPLY_ROWS
            };
            let max_rows = remaining.saturating_sub(below).min(PHOTO_ROWS);
            // The next tick encodes the photo for this height.
            self.app.photo_box_rows.set(max_rows);
            let panel = PhotoPanel::new(
                self.app.photo_views.get(&focused.url),
                &focused,
                self.app.picker.protocol_type(),
                max_rows,
            );
            let panel_h = panel.height().min(remaining);
            if panel_h > 0 {
                panel.render(Rect::new(inner.x + 1, y, content_width, panel_h), buf);
                y += panel_h;
                photo_shown = true;
            }
        }

        // Separator
        if (self.root.is_some() || photo_shown) && y < inner.y + inner.height {
            let sep = "\u{2550}".repeat(content_width as usize);
            buf.set_string(inner.x + 1, y, &sep, Style::default().fg(Color::Cyan));
            y += 1;
        }

        if self.replies.is_empty() {
            if y < inner.y + inner.height {
                buf.set_string(
                    inner.x + 2,
                    y,
                    "No replies",
                    Style::default().fg(Color::DarkGray),
                );
            }
            return;
        }

        // Render replies
        for (i, tweet) in self.replies.iter().enumerate() {
            if y >= inner.y + inner.height {
                break;
            }

            let card_h = tweet_card_height(tweet, content_width.saturating_sub(2)); // indent replies
            let remaining = (inner.y + inner.height).saturating_sub(y);
            let render_h = card_h.min(remaining);

            if render_h > 0 {
                // Thread connector
                buf.set_string(
                    inner.x + 1,
                    y,
                    "\u{2502}",
                    Style::default().fg(Color::DarkGray),
                );

                let reply_area =
                    Rect::new(inner.x + 3, y, content_width.saturating_sub(2), render_h);
                let author = tweet
                    .author_id
                    .as_ref()
                    .and_then(|id| self.app.lookup_user(id));
                TweetCard::new(tweet, author)
                    .selected(i == self.selected_index)
                    .render(reply_area, buf);

                y += render_h;
            }

            // Separator between replies
            if y < inner.y + inner.height && i + 1 < self.replies.len() {
                buf.set_string(
                    inner.x + 1,
                    y,
                    "\u{251C}",
                    Style::default().fg(Color::DarkGray),
                );
                let sep = "\u{2500}".repeat(content_width.saturating_sub(1) as usize);
                buf.set_string(inner.x + 2, y, &sep, Style::default().fg(Color::DarkGray));
                y += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ratatui::layout::Size;
    use ratatui_image::Resize;
    use ratatui_image::picker::Picker;

    use super::*;
    use crate::api::types::{Attachments, Media};
    use crate::app::PhotoView;
    use crate::auth::credentials::CredentialSet;
    use crate::config::AppConfig;
    use crate::event::{PhotoProtocol, ViewKind};

    fn tweet(id: &str, text: &str, media_key: Option<&str>) -> Tweet {
        let mut t: Tweet =
            serde_json::from_value(serde_json::json!({ "id": id, "text": text })).unwrap();
        t.attachments = media_key.map(|key| Attachments {
            media_keys: Some(vec![key.to_string()]),
            poll_ids: None,
        });
        t
    }

    /// An 80 x 24 terminal: a root post that wraps to four lines, a photo
    /// encoded for the full 14-row box, and replies.
    async fn short_terminal_thread() -> App {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        let url = "https://pbs.twimg.com/media/p.jpg".to_string();
        app.media_cache.insert(
            "p".into(),
            Media {
                media_key: "p".into(),
                type_: "photo".into(),
                url: Some(url.clone()),
                preview_image_url: None,
                width: None,
                height: None,
                alt_text: None,
            },
        );
        app.thread_root = Some(tweet("root", &"word ".repeat(60), Some("p")));
        app.thread_tweets = vec![tweet("r1", "first reply is visible", None)];
        app.push_view(ViewKind::Thread("root".into()));
        let protocol = Picker::halfblocks()
            .new_protocol(
                image::DynamicImage::new_rgb8(400, 600),
                Size::new(48, PHOTO_ROWS),
                Resize::Fit(None),
            )
            .unwrap();
        app.photo_views
            .insert(url, PhotoView::Ready(PhotoProtocol(Arc::new(protocol))));
        app
    }

    fn render(app: &App) -> String {
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        ThreadView::new(app.thread_root.as_ref(), &app.thread_tweets, app).render(area, &mut buf);
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn a_tall_photo_leaves_room_for_replies() {
        let app = short_terminal_thread().await;
        let screen = render(&app);
        assert!(screen.contains("first reply is visible"), "{screen}");
        // The box shrank, so the next tick encodes the photo for fewer rows.
        assert!(app.photo_box_rows.get() < PHOTO_ROWS);
    }

    #[tokio::test]
    async fn too_little_room_shows_a_note_instead_of_the_photo() {
        let mut app = short_terminal_thread().await;
        app.thread_root = Some(tweet("root", &"word ".repeat(200), Some("p")));
        let screen = render(&app);
        assert!(screen.contains("the window is too short"), "{screen}");
        assert!(screen.contains("first reply is visible"), "{screen}");
    }
}
