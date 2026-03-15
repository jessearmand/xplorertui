use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, StatefulWidget, Widget,
};

use crate::app::App;
use crate::event::ViewKind;
use crate::openrouter::extract_provider;

/// Model selection list view for OpenRouter models (embedding or text),
/// grouped by provider with an optional filter popup.
pub struct ModelsView<'a> {
    pub app: &'a App,
}

impl<'a> ModelsView<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }

    /// Render a centered filter popup listing providers.
    fn render_filter_popup(app: &App, area: Rect, buf: &mut Buffer) {
        let providers = app.model_providers();

        let width = 40u16.min(area.width.saturating_sub(4));
        let item_count = providers.len() + 1; // "All" + each provider
        let height = (item_count as u16 + 2).min(area.height.saturating_sub(2)); // +2 for borders
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup = Rect::new(x, y, width, height);

        Clear.render(popup, buf);

        let filter_label = app.model_filter.as_deref().unwrap_or("All");
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Filter by Provider (current: {filter_label}) "))
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Yellow));

        let mut items: Vec<ListItem> = Vec::with_capacity(item_count);

        // "All" option
        let all_style = if app.model_filter.is_none() {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        items.push(ListItem::new(Line::from(Span::styled("All", all_style))));

        // Provider options
        for provider in &providers {
            let is_active = app.model_filter.as_deref() == Some(provider.as_str());
            let style = if is_active {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            items.push(ListItem::new(Line::from(Span::styled(
                provider.clone(),
                style,
            ))));
        }

        let list = List::new(items)
            .block(block)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        let mut state = ListState::default().with_selected(Some(app.model_filter_index));
        StatefulWidget::render(list, popup, buf, &mut state);
    }
}

impl Widget for ModelsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let is_text = self.app.current_view() == Some(&ViewKind::TextModels);
        let (kind, loading, selected) = if is_text {
            (
                "Text Models",
                self.app.text_models_loading,
                self.app.selected_chat_model.as_ref(),
            )
        } else {
            (
                "Embedding Models",
                self.app.models_loading,
                self.app.selected_embedding_model.as_ref(),
            )
        };

        if loading {
            let block = Block::default()
                .title(format!(" {kind} (loading...) "))
                .borders(Borders::ALL);
            block.render(area, buf);
            return;
        }

        let filter_hint = match &self.app.model_filter {
            Some(p) => format!(" [{p}]"),
            None => String::new(),
        };
        let title = if let Some(selected) = selected {
            format!(" {kind}{filter_hint} (selected: {selected}) [f]ilter ")
        } else {
            format!(" {kind}{filter_hint} (Enter to select) [f]ilter ")
        };

        let block = Block::default().title(title).borders(Borders::ALL);

        // Build grouped model list with provider headers
        let filtered = self.app.filtered_model_list();

        let mut items: Vec<ListItem> = Vec::new();
        // Maps each model's position in `filtered` to its display row in `items`
        let mut model_to_display: Vec<usize> = Vec::with_capacity(filtered.len());
        let mut current_provider = "";

        for (i, model) in filtered.iter().enumerate() {
            let provider = extract_provider(&model.id);

            if provider != current_provider {
                // Insert provider header
                let count = filtered[i..]
                    .iter()
                    .take_while(|m| extract_provider(&m.id) == provider)
                    .count();
                items.push(ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("── {provider} "),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("({count}) ──"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])));
                current_provider = provider;
            }

            model_to_display.push(items.len());

            let name = model
                .name
                .as_deref()
                .unwrap_or(&model.id)
                .strip_prefix(&format!("{provider}/"))
                .unwrap_or(model.name.as_deref().unwrap_or(&model.id));

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
                Span::styled("  ", Style::default()),
                Span::styled(name, Style::default().fg(Color::Cyan)),
                Span::styled(ctx, Style::default().fg(Color::DarkGray)),
                Span::styled(price, Style::default().fg(Color::Yellow)),
            ]);
            items.push(ListItem::new(line));
        }

        if items.is_empty() {
            let inner_block = block;
            let empty = Paragraph::new("No models match the current filter.").block(inner_block);
            empty.render(area, buf);
        } else {
            let display_selected = model_to_display
                .get(self.app.selected_index())
                .copied()
                .unwrap_or(0);

            let list = List::new(items)
                .block(block)
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▸ ");

            let mut state = ListState::default().with_selected(Some(display_selected));
            StatefulWidget::render(list, area, buf, &mut state);
        }

        // Render filter popup overlay if open
        if self.app.model_filter_open {
            Self::render_filter_popup(self.app, area, buf);
        }
    }
}
