use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, StatefulWidget, Widget};

use crate::app::App;
use crate::ui::skeleton::render_models_skeleton;

/// View for browsing HuggingFace Hub models, grouped by organization.
pub struct HfModelsView<'a> {
    pub app: &'a App,
}

impl<'a> HfModelsView<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl Widget for HfModelsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Show skeleton immediately when loading (network-bound, always slow).
        if self.app.hf_models_loading {
            let elapsed_ms = self.app.skeleton_elapsed_ms_immediate();
            render_models_skeleton(elapsed_ms, "HuggingFace MLX Models (loading...)", area, buf);
            return;
        }

        let filtered = self.app.filtered_hf_models();

        // Build title with filter/search hints
        let filter_hint = match &self.app.hf_org_filter {
            Some(org) => format!(" [{org}]"),
            None => String::new(),
        };
        let search_hint = if self.app.hf_search.is_empty() {
            String::new()
        } else {
            format!(" search:\"{}\"", self.app.hf_search)
        };

        let title = format!(
            " HuggingFace MLX Models ({}){filter_hint}{search_hint} [/]search [f]ilter ",
            filtered.len()
        );

        let block = Block::default().title(title).borders(Borders::ALL);
        let inner = block.inner(area);
        block.render(area, buf);

        // Reserve bottom row for search input when active
        let (list_area, search_area) = if self.app.hf_search_active {
            let [list, search] =
                ratatui::layout::Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
                    .areas(inner);
            (list, Some(search))
        } else {
            (inner, None)
        };

        if !filtered.is_empty() {
            let mut items: Vec<ListItem> = Vec::new();
            let mut model_to_display: Vec<usize> = Vec::with_capacity(filtered.len());
            let mut current_org = "";

            for (i, model) in filtered.iter().enumerate() {
                let org = model.org();

                // Insert org header when org changes
                if org != current_org {
                    let count = filtered[i..].iter().take_while(|m| m.org() == org).count();
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("── {org} "),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("({count}) ──"),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ])));
                    current_org = org;
                }

                model_to_display.push(items.len());

                let quant = model.quant_tag().unwrap_or("fp");
                let pipeline = model.pipeline_tag.as_deref().unwrap_or("");
                let downloads = format_downloads(model.downloads);

                items.push(ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        model.short_name(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("  [{quant}]"), Style::default().fg(Color::Yellow)),
                    Span::styled(
                        format!("  {pipeline}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("  ⬇ {downloads}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])));
            }

            // Map the logical selection index to the display index (skipping headers)
            let selected = self.app.selected_index();
            let display_idx = model_to_display.get(selected).copied().unwrap_or(0);

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▸ ");

            let mut state = ListState::default().with_selected(Some(display_idx));
            StatefulWidget::render(list, list_area, buf, &mut state);
        }

        // Render search input bar
        if let Some(search_area) = search_area {
            let cursor = "█";
            let line = Line::from(vec![
                Span::styled("/ ", Style::default().fg(Color::Yellow)),
                Span::styled(&self.app.hf_search, Style::default().fg(Color::White)),
                Span::styled(cursor, Style::default().fg(Color::Yellow)),
            ]);
            buf.set_line(search_area.x, search_area.y, &line, search_area.width);
        }

        // Render org filter popup overlay
        if self.app.hf_org_filter_open {
            render_org_filter_popup(self.app, area, buf);
        }
    }
}

fn render_org_filter_popup(app: &App, area: Rect, buf: &mut Buffer) {
    let orgs = app.hf_orgs();

    // "All" + each org
    let mut items: Vec<ListItem> = Vec::new();
    items.push(ListItem::new(Line::from(Span::styled(
        "All",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))));
    for org in &orgs {
        let count = app
            .hf_models
            .iter()
            .filter(|m| m.org() == org.as_str())
            .count();
        items.push(ListItem::new(Line::from(vec![
            Span::styled(org.as_str(), Style::default().fg(Color::Cyan)),
            Span::styled(format!(" ({count})"), Style::default().fg(Color::DarkGray)),
        ])));
    }

    let width = 40u16.min(area.width.saturating_sub(4));
    let height = ((items.len() + 2) as u16).min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    Clear.render(popup, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Filter by Organization ")
        .title_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(popup);
    block.render(popup, buf);

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut state = ListState::default().with_selected(Some(app.hf_org_filter_index));
    StatefulWidget::render(list, inner, buf, &mut state);
}

fn format_downloads(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
