use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::app::{App, AppMode};
use crate::event::ViewKind;
use crate::ui::text::truncate_for_width;

/// Bottom status bar showing mode, current view, and status messages.
pub struct StatusBar<'a> {
    pub app: &'a App,
}

impl<'a> StatusBar<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Background
        let bg_style = Style::default().bg(Color::DarkGray).fg(Color::White);
        for x in area.x..area.x + area.width {
            buf[(x, area.y)].set_style(bg_style);
        }

        let mut spans = Vec::new();

        // Mode indicator
        let mode_str = match self.app.mode {
            AppMode::Normal => " NORMAL ",
            AppMode::Command => " COMMAND ",
            AppMode::Search => " SEARCH ",
        };
        let mode_style = Style::default()
            .bg(match self.app.mode {
                AppMode::Normal => Color::Blue,
                AppMode::Command => Color::Magenta,
                AppMode::Search => Color::Yellow,
            })
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        spans.push(Span::styled(mode_str, mode_style));
        spans.push(Span::raw(" "));

        // Current view
        let view_name = match self.app.current_view() {
            Some(ViewKind::Home) => "Following".to_string(),
            Some(ViewKind::UserTimeline(id)) => format!("Timeline: {id}"),
            Some(ViewKind::Thread(id)) => format!("Thread: {id}"),
            Some(ViewKind::UserProfile(name)) => format!("@{name}"),
            Some(ViewKind::Search) => {
                if self.app.search_query.is_empty() {
                    "Search".to_string()
                } else {
                    format!("Search: {}", self.app.search_query)
                }
            }
            Some(ViewKind::Mentions) => "Mentions".to_string(),
            Some(ViewKind::Bookmarks) => "Bookmarks".to_string(),
            Some(ViewKind::OpenRouterModels) => "Embedding Models".to_string(),
            Some(ViewKind::TextModels) => "Text Models".to_string(),
            Some(ViewKind::Cluster) => match self.app.cluster_source {
                Some(src) => format!("Clusters ({src})"),
                None => "Clusters".to_string(),
            },
            Some(ViewKind::HuggingFaceModels) => "HuggingFace Models".to_string(),
            Some(ViewKind::Help) => "Help".to_string(),
            None => "xplorertui".to_string(),
        };
        spans.push(Span::styled(view_name, bg_style));

        // Loading indicator
        if self.app.loading {
            spans.push(Span::styled(
                " [loading...]",
                Style::default().bg(Color::DarkGray).fg(Color::Yellow),
            ));
        }

        // Status message (right-aligned)
        if let Some(ref msg) = self.app.status_message {
            let left_width: usize = spans.iter().map(|s| s.width()).sum();
            let available = (area.width as usize).saturating_sub(left_width);
            let msg_style = Style::default().bg(Color::DarkGray).fg(Color::Cyan);
            let (message_spans, used) = status_message_spans(msg, available, msg_style);
            let padding = available.saturating_sub(used);
            if padding > 0 {
                spans.push(Span::styled(" ".repeat(padding), bg_style));
            }
            spans.extend(message_spans);
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

/// Hint appended to a truncated status message: `m` opens the full text.
const MORE_HINT: &str = " [m]ore";

/// Fit `msg` into `available` columns. When it does not fit, truncate it
/// and append [`MORE_HINT`] so the user knows the full text is one key away.
/// Returns the spans and their total width.
fn status_message_spans(
    msg: &str,
    available: usize,
    msg_style: Style,
) -> (Vec<Span<'static>>, usize) {
    // A newline would be drawn as-is and garble the bar; flatten it.
    let flat = msg.replace('\n', " ");
    let full_width = Span::raw(flat.as_str()).width();
    if full_width <= available {
        return (vec![Span::styled(flat, msg_style)], full_width);
    }

    let hint_width = Span::raw(MORE_HINT).width();
    if available <= hint_width {
        let display = truncate_for_width(&flat, available);
        let width = Span::raw(display.as_str()).width();
        return (vec![Span::styled(display, msg_style)], width);
    }

    let display = truncate_for_width(&flat, available - hint_width);
    let width = Span::raw(display.as_str()).width() + hint_width;
    let hint_style = msg_style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
    (
        vec![
            Span::styled(display, msg_style),
            Span::styled(MORE_HINT, hint_style),
        ],
        width,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[Span<'_>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn short_message_has_no_hint() {
        let (spans, width) = status_message_spans("Loaded 5 models", 40, Style::default());
        assert_eq!(text(&spans), "Loaded 5 models");
        assert_eq!(width, 15);
    }

    #[test]
    fn long_message_is_truncated_with_hint() {
        let msg = "Clustering error: MLX server not reachable and no fallback";
        let (spans, width) = status_message_spans(msg, 30, Style::default());
        let shown = text(&spans);
        assert!(shown.ends_with(MORE_HINT), "got {shown:?}");
        assert!(shown.contains('…'), "got {shown:?}");
        assert!(width <= 30);
    }

    #[test]
    fn newlines_are_flattened() {
        let (spans, _) = status_message_spans("a\nb", 10, Style::default());
        assert_eq!(text(&spans), "a b");
    }
}
