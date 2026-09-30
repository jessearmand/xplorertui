use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::app::{Popup, PopupTone};

/// A centered modal overlay: a message (error or info) or a yes/no prompt.
pub struct PopupView<'a> {
    popup: &'a Popup,
    /// Requested first visible text row; clamped to the text length.
    scroll: u16,
}

impl<'a> PopupView<'a> {
    pub fn new(popup: &'a Popup, scroll: u16) -> Self {
        Self { popup, scroll }
    }
}

fn tone_color(tone: PopupTone) -> Color {
    match tone {
        PopupTone::Error => Color::Red,
        PopupTone::Info => Color::Cyan,
        PopupTone::Prompt => Color::Yellow,
    }
}

/// Number of rows `text` needs when word-wrapped to `width` columns, as
/// `Paragraph` with `Wrap { trim: true }` lays it out: words move to the
/// next row when they don't fit, and words wider than a row are split.
/// Errs high when unsure, so scrolling can always reach the last line.
fn wrapped_line_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return text.lines().count().max(1);
    }
    text.lines()
        .map(|line| {
            let mut rows = 1;
            let mut used = 0;
            for word in line.split_whitespace() {
                let w = Span::raw(word).width();
                let needed = if used == 0 { w } else { used + 1 + w };
                if needed <= width {
                    used = needed;
                } else if w <= width {
                    rows += 1;
                    used = w;
                } else {
                    // A word wider than a row is split across rows. Start it
                    // on a fresh row so the count errs high, never low.
                    if used > 0 {
                        rows += 1;
                    }
                    rows += w.div_ceil(width) - 1;
                    used = w - (w.div_ceil(width) - 1) * width;
                }
            }
            rows
        })
        .sum::<usize>()
        .max(1)
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

        let total_rows = wrapped_line_count(text, inner_width) as u16;
        let max_scroll = total_rows.saturating_sub(text_area.height);
        let scroll = self.scroll.min(max_scroll);

        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .scroll((scroll, 0))
            .render(text_area, buf);

        let hint_text = if max_scroll > 0 {
            let last = (scroll + text_area.height).min(total_rows);
            format!(
                "{}   j/k: scroll ({}-{last}/{total_rows})",
                self.popup.hint(),
                scroll + 1
            )
        } else {
            self.popup.hint()
        };
        let hint = Line::from(Span::styled(
            format!(" {hint_text} "),
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
        PopupView::new(&popup, 0).render(area, &mut buf);

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

    fn render_to_string(popup: &Popup, scroll: u16, area: Rect) -> String {
        let mut buf = Buffer::empty(area);
        PopupView::new(popup, scroll).render(area, &mut buf);
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    + "\n"
            })
            .collect()
    }

    #[test]
    fn long_message_scrolls_to_its_last_line() {
        let text: String = (1..=40).map(|i| format!("line {i}\n")).collect();
        let popup = Popup::info(text);
        let area = Rect::new(0, 0, 80, 20);

        let top = render_to_string(&popup, 0, area);
        assert!(top.contains("line 1 "));
        assert!(!top.contains("line 40"));
        assert!(top.contains("j/k: scroll (1-"), "{top}");

        // Over-scrolling clamps so the last line is still on screen.
        let bottom = render_to_string(&popup, u16::MAX, area);
        assert!(bottom.contains("line 40"), "{bottom}");
        assert!(!bottom.contains("line 1 "));
        assert!(bottom.contains("/40)"), "{bottom}");
    }

    #[test]
    fn short_message_has_no_scroll_hint() {
        let popup = Popup::info("short");
        let rendered = render_to_string(&popup, 0, Rect::new(0, 0, 80, 20));
        assert!(!rendered.contains("j/k: scroll"));
    }

    #[test]
    fn wrapped_line_count_wraps_by_word() {
        use super::wrapped_line_count;
        assert_eq!(wrapped_line_count("", 10), 1);
        assert_eq!(wrapped_line_count("aaaa bbbb", 10), 1);
        // 9 columns fits "aaaa bbbb" exactly; 8 forces "bbbb" to row 2.
        assert_eq!(wrapped_line_count("aaaa bbbb", 9), 1);
        assert_eq!(wrapped_line_count("aaaa bbbb", 8), 2);
        // Per-character counting would say 2 rows here; word wrap needs 3.
        assert_eq!(wrapped_line_count("aaaaaa bbbbbb cccccc", 10), 3);
        // A word wider than the row is split across rows.
        assert_eq!(wrapped_line_count(&"x".repeat(25), 10), 3);
        assert_eq!(wrapped_line_count("a\n\nb", 10), 3);
    }
}
