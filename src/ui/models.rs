use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, StatefulWidget, Widget,
};

use crate::app::App;
use crate::event::ViewKind;
use crate::openrouter::extract_provider;
use crate::ui::input::TextInput;
use crate::ui::skeleton::render_models_skeleton;

/// Model selection list view for OpenRouter models (embedding or text),
/// grouped by provider with an optional filter popup.
pub struct ModelsView<'a> {
    pub app: &'a App,
}

impl<'a> ModelsView<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }

    /// Render a centered filter popup listing providers with optional search.
    fn render_filter_popup(app: &App, area: Rect, buf: &mut Buffer) {
        let providers = app.filtered_model_providers();

        let width = 40u16.min(area.width.saturating_sub(4));
        let item_count = providers.len() + 1; // "All" + each provider
        let show_search = app.model_filter_search_active || !app.model_filter_search.is_empty();
        let search_rows: u16 = if show_search { 1 } else { 0 };
        let height = (item_count as u16 + 2 + search_rows).min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup = Rect::new(x, y, width, height);

        Clear.render(popup, buf);

        let filter_label = app.model_filter.as_deref().unwrap_or("All");
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " Filter by Provider (current: {filter_label}) [/]search "
            ))
            .title_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Yellow));

        let inner = block.inner(popup);
        block.render(popup, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        // Split inner area: optional search row at top, provider list below
        let (search_area, list_area) = if show_search {
            let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
            (Some(chunks[0]), chunks[1])
        } else {
            (None, inner)
        };

        // Render search input if visible
        if let Some(sa) = search_area {
            let cursor = if app.model_filter_search_active {
                "\u{2588}"
            } else {
                ""
            };
            let search_display = format!("/ {}{cursor}", app.model_filter_search);
            buf.set_string(
                sa.x,
                sa.y,
                &search_display,
                Style::default().fg(Color::Yellow),
            );
        }

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
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▸ ");

        let mut state = ListState::default().with_selected(Some(app.model_filter_index));
        StatefulWidget::render(list, list_area, buf, &mut state);
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
            // Model fetches are always network-bound — show skeleton immediately.
            let elapsed_ms = self.app.skeleton_elapsed_ms_immediate();
            let title = format!("{kind} (loading...)");
            render_models_skeleton(elapsed_ms, &title, area, buf);
            return;
        }

        let filter_hint = match &self.app.model_filter {
            Some(p) => format!(" [{p}]"),
            None => String::new(),
        };
        let search_hint = if !self.app.model_search.is_empty() {
            format!(" search:\"{}\"", self.app.model_search)
        } else {
            String::new()
        };
        let title = if let Some(selected) = selected {
            format!(" {kind}{filter_hint}{search_hint} (selected: {selected}) [f]ilter [/]search ")
        } else {
            format!(" {kind}{filter_hint}{search_hint} (Enter to select) [f]ilter [/]search ")
        };

        let block = Block::default().title(title).borders(Borders::ALL);

        // Determine if we need a search input row at the bottom
        let show_search = self.app.model_search_active || !self.app.model_search.is_empty();

        // When search is active, render block ourselves and split inner area.
        // Otherwise, pass block to the List widget below.
        let (list_area, search_area, block_for_list) = if show_search {
            let outer = block.inner(area);
            block.render(area, buf);
            if outer.height < 2 {
                return;
            }
            let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(outer);
            (chunks[0], Some(chunks[1]), None)
        } else {
            (area, None, Some(block))
        };

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
            let empty_msg = "No models match the current filter.";
            let mut para = Paragraph::new(empty_msg);
            if let Some(b) = block_for_list {
                para = para.block(b);
            }
            para.render(list_area, buf);
        } else {
            let display_selected = model_to_display
                .get(self.app.selected_index())
                .copied()
                .unwrap_or(0);

            let mut list = List::new(items)
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▸ ");

            if let Some(b) = block_for_list {
                list = list.block(b);
            }

            let mut state = ListState::default().with_selected(Some(display_selected));
            StatefulWidget::render(list, list_area, buf, &mut state);
        }

        // Render search input at bottom if active
        if let Some(sa) = search_area {
            TextInput::new("/ ", &self.app.model_search).render(sa, buf);
        }

        // Render filter popup overlay if open
        if self.app.model_filter_open {
            Self::render_filter_popup(self.app, area, buf);
        }
    }
}
