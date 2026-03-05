use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget};

use crate::app::App;

/// Model selection list view for OpenRouter embedding models.
pub struct ModelsView<'a> {
    pub app: &'a App,
}

impl<'a> ModelsView<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl Widget for ModelsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.app.models_loading {
            let block = Block::default()
                .title(" Embedding Models (loading...) ")
                .borders(Borders::ALL);
            block.render(area, buf);
            return;
        }

        let title = if let Some(ref selected) = self.app.selected_embedding_model {
            format!(" Embedding Models (selected: {selected}) ")
        } else {
            " Embedding Models (Enter to select) ".to_string()
        };

        let block = Block::default().title(title).borders(Borders::ALL);

        let items: Vec<ListItem> = self
            .app
            .openrouter_models
            .iter()
            .map(|model| {
                let name = model.name.as_deref().unwrap_or(&model.id);
                let ctx = model
                    .context_length
                    .map(|c| format!(" ({c} ctx)"))
                    .unwrap_or_default();
                let price = model
                    .pricing
                    .as_ref()
                    .and_then(|p| p.prompt.as_ref())
                    .map(|p| format!(" ${p}/1K"))
                    .unwrap_or_default();

                let line = Line::from(vec![
                    Span::styled(name, Style::default().fg(Color::Cyan)),
                    Span::styled(ctx, Style::default().fg(Color::DarkGray)),
                    Span::styled(price, Style::default().fg(Color::Yellow)),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        let mut state = ListState::default().with_selected(Some(self.app.selected_index()));
        StatefulWidget::render(list, area, buf, &mut state);
    }
}
