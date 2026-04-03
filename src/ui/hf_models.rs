use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, StatefulWidget, Widget};

use crate::app::App;

/// View for browsing HuggingFace Hub models.
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
        let search_hint = if self.app.hf_search.is_empty() {
            " [/]search".to_string()
        } else {
            format!(" search:\"{}\"", self.app.hf_search)
        };

        let title = if self.app.hf_models_loading {
            format!(" HuggingFace MLX Models (loading...){search_hint} ")
        } else {
            format!(
                " HuggingFace MLX Models ({}){search_hint} ",
                self.app.hf_models.len()
            )
        };

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

        if !self.app.hf_models.is_empty() {
            let items: Vec<ListItem> = self
                .app
                .hf_models
                .iter()
                .map(|m| {
                    let quant = m.quant_tag().unwrap_or("fp");
                    let pipeline = m.pipeline_tag.as_deref().unwrap_or("");
                    let downloads = format_downloads(m.downloads);

                    ListItem::new(Line::from(vec![
                        Span::styled(
                            m.short_name(),
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
                    ]))
                })
                .collect();

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▸ ");

            let mut state = ListState::default().with_selected(Some(self.app.selected_index()));
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
    }
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
