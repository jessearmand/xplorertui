use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::app::{App, AppMode};
use crate::event::ViewKind;

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
            Some(ViewKind::Home) => "Home".to_string(),
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
            let msg_width = msg.len().min(area.width as usize);
            let padding = (area.width as usize).saturating_sub(left_width + msg_width);
            if padding > 0 {
                spans.push(Span::styled(" ".repeat(padding), bg_style));
            }
            spans.push(Span::styled(
                &msg[..msg_width],
                Style::default().bg(Color::DarkGray).fg(Color::Red),
            ));
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
