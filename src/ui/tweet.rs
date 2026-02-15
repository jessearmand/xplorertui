use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::api::types::{Tweet, User};

/// Renders a single tweet as a compact card (2-4 lines).
///
/// Layout:
///   @username · 2h ago                   [RT] [Reply]
///   Tweet text (may wrap) ...
///   ♥ 12  🔁 3  💬 5  🔖 1
pub struct TweetCard<'a> {
    pub tweet: &'a Tweet,
    pub author: Option<&'a User>,
    pub selected: bool,
}

impl<'a> TweetCard<'a> {
    pub fn new(tweet: &'a Tweet, author: Option<&'a User>) -> Self {
        Self {
            tweet,
            author,
            selected: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Widget for TweetCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let highlight_style = if self.selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };

        let mut y = area.y;

        // -- Line 1: author info + indicators --
        let username = self
            .author
            .map(|u| format!("@{}", u.username))
            .or_else(|| self.tweet.author_id.clone().map(|id| format!("@{id}")))
            .unwrap_or_else(|| "@unknown".into());

        let time_ago = self
            .tweet
            .created_at
            .map(format_time_ago)
            .unwrap_or_default();

        let mut header_spans = vec![Span::styled(
            &username,
            highlight_style.add_modifier(Modifier::BOLD),
        )];

        if let Some(name) = self.author.map(|u| &u.name) {
            header_spans.push(Span::raw(" "));
            header_spans.push(Span::styled(
                name.as_str(),
                Style::default().fg(Color::DarkGray),
            ));
        }

        if !time_ago.is_empty() {
            header_spans.push(Span::styled(
                format!(" · {time_ago}"),
                Style::default().fg(Color::DarkGray),
            ));
        }

        // Indicators for RT/reply
        if let Some(ref refs) = self.tweet.referenced_tweets {
            for rt in refs {
                match rt.type_.as_str() {
                    "retweeted" => {
                        header_spans.push(Span::styled(" [RT]", Style::default().fg(Color::Green)));
                    }
                    "replied_to" => {
                        header_spans
                            .push(Span::styled(" [Reply]", Style::default().fg(Color::Blue)));
                    }
                    "quoted" => {
                        header_spans
                            .push(Span::styled(" [Quote]", Style::default().fg(Color::Yellow)));
                    }
                    _ => {}
                }
            }
        }

        let header_line = Line::from(header_spans);
        buf.set_line(area.x, y, &header_line, area.width);
        y += 1;

        if y >= area.y + area.height {
            return;
        }

        // -- Line 2+: tweet text (wrapped) --
        let text = self
            .tweet
            .note_tweet
            .as_ref()
            .map(|nt| nt.text.as_str())
            .unwrap_or(&self.tweet.text);

        let width = area.width as usize;
        let max_text_lines = (area.height - (y - area.y) - 1).max(1) as usize; // Reserve 1 line for metrics

        for (i, line_text) in wrap_text(text, width).into_iter().enumerate() {
            if i >= max_text_lines || y >= area.y + area.height {
                break;
            }
            let text_style = if self.selected {
                Style::default().fg(Color::White)
            } else {
                Style::default()
            };
            buf.set_string(area.x, y, &line_text, text_style);
            y += 1;
        }

        if y >= area.y + area.height {
            return;
        }

        // -- Last line: metrics --
        if let Some(ref metrics) = self.tweet.public_metrics {
            let metrics_line = Line::from(vec![
                Span::styled(
                    format!("\u{2665} {}", format_count(metrics.like_count)),
                    Style::default().fg(Color::Red),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("\u{21BB} {}", format_count(metrics.retweet_count)),
                    Style::default().fg(Color::Green),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("\u{1F4AC} {}", format_count(metrics.reply_count)),
                    Style::default().fg(Color::Blue),
                ),
            ]);
            buf.set_line(area.x, y, &metrics_line, area.width);
        }
    }
}

/// Height in lines needed for a tweet card.
pub fn tweet_card_height(tweet: &Tweet, width: u16) -> u16 {
    let text = tweet
        .note_tweet
        .as_ref()
        .map(|nt| nt.text.as_str())
        .unwrap_or(&tweet.text);
    let text_lines = wrap_text(text, width as usize).len() as u16;
    // header + text + metrics
    1 + text_lines + 1
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![];
    }
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn format_time_ago(dt: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(dt);

    if diff.num_seconds() < 60 {
        format!("{}s", diff.num_seconds())
    } else if diff.num_minutes() < 60 {
        format!("{}m", diff.num_minutes())
    } else if diff.num_hours() < 24 {
        format!("{}h", diff.num_hours())
    } else if diff.num_days() < 30 {
        format!("{}d", diff.num_days())
    } else {
        dt.format("%b %d").to_string()
    }
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
