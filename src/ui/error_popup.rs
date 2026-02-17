use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

/// A dismissible centered popup overlay that displays the full API error message.
pub struct ErrorPopup<'a> {
    text: &'a str,
}

impl<'a> ErrorPopup<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text }
    }
}

impl Widget for ErrorPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let max_width = 70u16.min(area.width.saturating_sub(4));
        // Inner width available for text (subtract 2 for border)
        let inner_width = max_width.saturating_sub(2) as usize;

        // Estimate wrapped line count
        let text_lines: usize = self
            .text
            .lines()
            .map(|line| {
                if line.is_empty() || inner_width == 0 {
                    1
                } else {
                    line.len().div_ceil(inner_width)
                }
            })
            .sum();

        // +2 for border top/bottom, +2 for hint line + blank line above hint
        let content_height = (text_lines as u16) + 4;
        let max_height = (area.height * 3 / 5).max(8);
        let height = content_height
            .min(max_height)
            .min(area.height.saturating_sub(2));

        let x = area.x + (area.width.saturating_sub(max_width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let panel = Rect::new(x, y, max_width, height);

        Clear.render(panel, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Error Details ")
            .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(Color::Red));

        let inner = block.inner(panel);
        block.render(panel, buf);

        // Reserve the last line of inner area for the dismiss hint
        if inner.height < 2 {
            return;
        }
        let text_area = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
        let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);

        let paragraph = Paragraph::new(self.text).wrap(Wrap { trim: true });
        paragraph.render(text_area, buf);

        let hint = Line::from(Span::styled(
            " Press Esc or Enter to dismiss ",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ));
        Paragraph::new(hint).render(hint_area, buf);
    }
}
