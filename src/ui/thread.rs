use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};

use crate::api::types::Tweet;
use crate::app::App;
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

            // Separator
            if y < inner.y + inner.height {
                let sep = "\u{2550}".repeat(content_width as usize);
                buf.set_string(inner.x + 1, y, &sep, Style::default().fg(Color::Cyan));
                y += 1;
            }
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
