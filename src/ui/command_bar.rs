use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::app::{App, AppMode};
use crate::ui::input::TextInput;

/// Command/search input bar rendered at the bottom when in command or search mode.
pub struct CommandBar<'a> {
    pub app: &'a App,
}

impl<'a> CommandBar<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl Widget for CommandBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.app.mode {
            AppMode::Command => {
                TextInput::new(":", &self.app.command_input).render(area, buf);
            }
            AppMode::Search => {
                TextInput::new("/", &self.app.search_input).render(area, buf);
            }
            AppMode::Normal => {}
        }
    }
}
