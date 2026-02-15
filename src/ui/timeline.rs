use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};

use crate::api::types::Tweet;
use crate::app::App;
use crate::ui::tweet::{TweetCard, tweet_card_height};

/// A scrollable list of tweets with selection highlight.
///
/// Used by home timeline, mentions, bookmarks, search results, and user timeline.
pub struct TimelineView<'a> {
    pub title: &'a str,
    pub tweets: &'a [Tweet],
    pub selected_index: usize,
    pub app: &'a App,
    pub loading: bool,
}

impl<'a> TimelineView<'a> {
    pub fn new(title: &'a str, tweets: &'a [Tweet], app: &'a App) -> Self {
        Self {
            title,
            tweets,
            selected_index: app.selected_index(),
            app,
            loading: false,
        }
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }
}

impl Widget for TimelineView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.title))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::DarkGray));

        let inner = block.inner(area);
        block.render(area, buf);

        if self.tweets.is_empty() {
            let msg = if self.loading {
                "Loading..."
            } else {
                "No tweets to display"
            };
            buf.set_string(
                inner.x + 1,
                inner.y,
                msg,
                Style::default().fg(Color::DarkGray),
            );
            return;
        }

        let content_width = inner.width.saturating_sub(1); // 1 char left margin
        let available_height = inner.height;

        // Pre-compute heights for each tweet card (including separator).
        let heights: Vec<u16> = self
            .tweets
            .iter()
            .map(|t| tweet_card_height(t, content_width) + 1)
            .collect();

        // Find the scroll start: the first tweet index such that the selected
        // tweet is visible within the available height.
        let scroll_start = compute_scroll_start(&heights, self.selected_index, available_height);

        // Render from scroll_start
        let mut y = inner.y;
        let mut tweet_idx = scroll_start;
        while tweet_idx < self.tweets.len() && y < inner.y + inner.height {
            let tweet = &self.tweets[tweet_idx];
            let card_h = heights[tweet_idx];
            let remaining = inner.y + inner.height - y;
            let render_h = card_h.min(remaining);

            let tweet_area = Rect::new(inner.x + 1, y, content_width, render_h.saturating_sub(1));

            let author = tweet
                .author_id
                .as_ref()
                .and_then(|id| self.app.lookup_user(id));

            TweetCard::new(tweet, author)
                .selected(tweet_idx == self.selected_index)
                .render(tweet_area, buf);

            y += render_h;

            // Draw separator line
            if y < inner.y + inner.height && tweet_idx + 1 < self.tweets.len() {
                let sep = "\u{2500}".repeat(content_width as usize);
                buf.set_string(
                    inner.x + 1,
                    y.saturating_sub(1),
                    &sep,
                    Style::default().fg(Color::DarkGray),
                );
            }

            tweet_idx += 1;
        }
    }
}

/// Find the smallest scroll start index so that the selected item fits
/// within the available height.
fn compute_scroll_start(heights: &[u16], selected: usize, available: u16) -> usize {
    if heights.is_empty() {
        return 0;
    }

    let selected = selected.min(heights.len() - 1);

    // Start from 0 and accumulate. If the selected item would overflow,
    // advance the start.
    let mut start = 0;
    loop {
        let mut used: u16 = 0;
        let mut fits = false;
        for (i, &h) in heights.iter().enumerate().skip(start) {
            let next = used.saturating_add(h);
            if next > available && i <= selected {
                start += 1;
                break;
            }
            used = next;
            if i == selected {
                fits = true;
                break;
            }
            if used >= available {
                break;
            }
        }
        if fits || start > selected {
            break;
        }
    }
    start
}
