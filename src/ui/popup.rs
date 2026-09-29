use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::app::{Popup, PopupTone};

/// A centered modal overlay: a message (error or info) or a yes/no prompt.
pub struct PopupView<'a> {
    popup: &'a Popup,
}

impl<'a> PopupView<'a> {
    pub fn new(popup: &'a Popup) -> Self {
        Self { popup }
    }
}

fn tone_color(tone: PopupTone) -> Color {
    match tone {
        PopupTone::Error => Color::Red,
        PopupTone::Info => Color::Cyan,
        PopupTone::Prompt => Color::Yellow,
    }
}

/// Number of rows `text` needs when wrapped to `width` columns (estimate).
fn wrapped_line_count(text: &str, width: usize) -> usize {
    text.lines()
        .map(|line| {
            let cols = Span::raw(line).width();
            if cols == 0 || width == 0 {
                1
            } else {
                cols.div_ceil(width)
            }
        })
        .sum()
}

impl Widget for PopupView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let tone = self.popup.tone();
        let color = tone_color(tone);
        let text = self.popup.text();

        let max_width = 70u16.min(area.width.saturating_sub(4));
        // Inner width available for text (subtract 2 for border)
        let inner_width = max_width.saturating_sub(2) as usize;

        // +2 for border top/bottom, +2 for hint line + blank line above hint
        let content_height = (wrapped_line_count(text, inner_width) as u16) + 4;
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
            .title(format!(" {} ", self.popup.title()))
            .title_style(Style::default().fg(color).add_modifier(Modifier::BOLD))
            .border_style(Style::default().fg(color));

        let inner = block.inner(panel);
        block.render(panel, buf);

        // Reserve the last line of inner area for the key hint
        if inner.height < 2 {
            return;
        }
        let text_area = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
        let hint_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);

        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .render(text_area, buf);

        let hint = Line::from(Span::styled(
            format!(" {} ", self.popup.hint()),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ));
        Paragraph::new(hint).render(hint_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    use super::PopupView;
    use crate::app::{ClusterSource, Popup, PromptAction};

    #[test]
    fn confirm_prompt_renders_title_text_and_hint() {
        let popup = Popup::Confirm {
            title: "Embedding Model Needed",
            text: "Clustering needs an embedding model.\n\nSelect one now?".into(),
            accept_label: "Select model",
            action: PromptAction::SelectEmbeddingModel {
                resume: ClusterSource::Home,
            },
        };
        let area = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(area);
        PopupView::new(&popup).render(area, &mut buf);

        let rendered: String = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    + "\n"
            })
            .collect();
        assert!(rendered.contains("Embedding Model Needed"));
        assert!(rendered.contains("Select one now?"));
        assert!(rendered.contains("Enter/y: Select model"));
    }
}
