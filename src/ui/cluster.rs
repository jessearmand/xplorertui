use ansi_to_tui::IntoText;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::app::App;
use crate::embeddings::cluster::ClusterResult;

// Bright colors chosen for visibility on dark terminal backgrounds.
// Each entry is (hex for kuva, ratatui Color) so both renderers stay in sync.
const CLUSTER_COLORS: &[(&str, Color)] = &[
    ("#6baadc", Color::Rgb(107, 170, 220)), // light blue
    ("#ff4466", Color::Rgb(255, 68, 102)),  // bright red
    ("#50d050", Color::Rgb(80, 208, 80)),   // bright green
    ("#ffaa33", Color::Rgb(255, 170, 51)),  // bright orange
    ("#b58adf", Color::Rgb(181, 138, 223)), // light purple
    ("#d4856a", Color::Rgb(212, 133, 106)), // light coral
    ("#f0a0d8", Color::Rgb(240, 160, 216)), // light pink
    ("#aaaaaa", Color::Rgb(170, 170, 170)), // light gray
    ("#d8d840", Color::Rgb(216, 216, 64)),  // bright yellow-green
    ("#40dde8", Color::Rgb(64, 221, 232)),  // bright cyan
];

/// Scatter plot view for displaying tweet clustering results.
pub struct ClusterView<'a> {
    pub app: &'a App,
}

impl<'a> ClusterView<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }

    fn render_scatter(result: &ClusterResult, cols: usize, rows: usize) -> String {
        use kuva::prelude::*;

        let num_clusters = result.num_clusters();
        let plots: Vec<Plot> = (0..num_clusters)
            .map(|c| {
                let points = result.points_for_cluster(c);
                let (color, _) = CLUSTER_COLORS[c % CLUSTER_COLORS.len()];
                let legend = format!("C{c}");

                ScatterPlot::new()
                    .with_data(points)
                    .with_color(color)
                    .with_size(3.0)
                    .with_legend(&legend)
                    .into()
            })
            .collect();

        if plots.is_empty() {
            return String::from("No data to plot");
        }

        let layout = Layout::auto_from_plots(&plots)
            .with_title("Topic Clusters")
            .with_x_label("PC1")
            .with_y_label("PC2")
            .with_theme(Theme::dark());

        let scene = render_multiple(plots, layout);
        TerminalBackend::new(cols, rows).render_scene(&scene)
    }

    fn render_legend(result: &ClusterResult, area: Rect, buf: &mut Buffer) {
        let num_clusters = result.num_clusters();
        let lines: Vec<Line<'_>> = (0..num_clusters)
            .map(|c| {
                let (_, color) = CLUSTER_COLORS[c % CLUSTER_COLORS.len()];
                let topic =
                    if c < result.cluster_topics.len() && !result.cluster_topics[c].is_empty() {
                        result.cluster_topics[c].as_str()
                    } else {
                        "(no data)"
                    };

                // Truncate to fit available width (leave room for "█ C0: " prefix).
                let max_topic_len = area.width.saturating_sub(8) as usize;
                let display_topic = if topic.len() > max_topic_len && max_topic_len > 3 {
                    format!("{}...", &topic[..max_topic_len - 3])
                } else {
                    topic.to_string()
                };

                Line::from(vec![
                    Span::styled("█ ", Style::default().fg(color)),
                    Span::styled(
                        format!("C{c}: "),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(display_topic, Style::default().fg(Color::White)),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }

    fn render_loading_popup(area: Rect, buf: &mut Buffer) {
        let width = 40u16.min(area.width.saturating_sub(4));
        let height = 5u16.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup = Rect::new(x, y, width, height);

        Clear.render(popup, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Computing Clusters ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(popup);
        block.render(popup, buf);

        let text = Paragraph::new(Line::from(vec![Span::styled(
            "Embedding tweets and clustering...",
            Style::default().fg(Color::Yellow),
        )]))
        .centered();
        text.render(inner, buf);
    }
}

impl Widget for ClusterView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" Topic Clusters (Esc to go back) ")
            .borders(Borders::ALL);

        if self.app.cluster_loading {
            block.render(area, buf);
            Self::render_loading_popup(area, buf);
            return;
        }

        let Some(ref result) = self.app.cluster_result else {
            let empty = Paragraph::new("No cluster data. Use :cluster to compute.").block(block);
            empty.render(area, buf);
            return;
        };

        let num_clusters = result.num_clusters();
        let legend_height = (num_clusters as u16).clamp(1, 10);

        // Split area: chart on top, legend at bottom.
        let inner = block.inner(area);
        if inner.width < 10 || inner.height < 5 {
            block.render(area, buf);
            return;
        }
        block.render(area, buf);

        let [chart_area, legend_area] = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(legend_height + 1), // +1 for spacing
        ])
        .areas(inner);

        let ansi_output = Self::render_scatter(
            result,
            chart_area.width as usize,
            chart_area.height as usize,
        );
        let text = ansi_output.as_bytes().into_text().unwrap_or_default();
        let chart = Paragraph::new(text).wrap(Wrap { trim: false });
        chart.render(chart_area, buf);

        // Render color legend panel.
        let legend_inner = Rect::new(
            legend_area.x + 1,
            legend_area.y,
            legend_area.width.saturating_sub(2),
            legend_area.height,
        );
        Self::render_legend(result, legend_inner, buf);
    }
}
