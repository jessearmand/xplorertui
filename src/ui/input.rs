use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

/// A simple single-line text input renderer.
///
/// Renders the prompt + text content, with a cursor indicator at the end.
pub struct TextInput<'a> {
    pub prompt: &'a str,
    pub text: &'a str,
    pub style: Style,
}

impl<'a> TextInput<'a> {
    pub fn new(prompt: &'a str, text: &'a str) -> Self {
        Self {
            prompt,
            text,
            style: Style::default().fg(Color::White),
        }
    }
}

impl Widget for TextInput<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let display = format!("{}{}\u{2588}", self.prompt, self.text);
        let max_width = area.width as usize;
        // If the display is wider than the area, show the rightmost portion.
        let visible = if display.len() > max_width {
            &display[display.len() - max_width..]
        } else {
            &display
        };

        buf.set_string(area.x, area.y, visible, self.style);
    }
}
