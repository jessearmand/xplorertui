use ansi_to_tui::IntoText;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap,
};

use crate::app::App;
use crate::embeddings::cluster::ClusterResult;
use crate::ui::skeleton::render_cluster_skeleton;
use crate::ui::text::truncate_for_width;

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

    fn render_scatter(
        result: &ClusterResult,
        source_label: Option<&str>,
        cols: usize,
        rows: usize,
    ) -> String {
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

        let title = match source_label {
            Some(label) => format!("Topic Clusters — {label}"),
            None => "Topic Clusters".to_string(),
        };
        let layout = Layout::auto_from_plots(&plots)
            .with_title(&title)
            .with_x_label("PC1")
            .with_y_label("PC2")
            .with_theme(Theme::dark());

        let scene = render_multiple(plots, layout);
        TerminalBackend::new(cols, rows).render_scene(&scene)
    }

    /// Render cluster list mode: scatter plot on top, selectable cluster list on bottom.
    fn render_cluster_list(
        result: &ClusterResult,
        selected_index: usize,
        topics_loading: bool,
        chat_provider_name: Option<&str>,
        source_label: Option<&str>,
        area: Rect,
        buf: &mut Buffer,
    ) {
        let source_suffix = match source_label {
            Some(label) => format!(" — {label}"),
            None => String::new(),
        };
        let title = if topics_loading {
            format!(
                " Topic Clusters{source_suffix} (generating labels via {}...) ",
                chat_provider_name.unwrap_or("…"),
            )
        } else {
            format!(" Topic Clusters{source_suffix} (Enter to browse, Esc to go back) ")
        };
        let block = Block::default().title(title).borders(Borders::ALL);

        let num_clusters = result.num_clusters();
        let legend_height = (num_clusters as u16).clamp(1, 10) + 2; // +2 for list block borders

        let inner = block.inner(area);
        if inner.width < 10 || inner.height < 5 {
            block.render(area, buf);
            return;
        }
        block.render(area, buf);

        let [chart_area, list_area] = ratatui::layout::Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(legend_height),
        ])
        .areas(inner);

        // Render scatter plot
        let ansi_output = Self::render_scatter(
            result,
            source_label,
            chart_area.width as usize,
            chart_area.height as usize,
        );
        let text = ansi_output.as_bytes().into_text().unwrap_or_default();
        let chart = Paragraph::new(text).wrap(Wrap { trim: false });
        chart.render(chart_area, buf);

        // Render selectable cluster list
        let items: Vec<ListItem> = (0..num_clusters)
            .map(|c| {
                let (_, color) = CLUSTER_COLORS[c % CLUSTER_COLORS.len()];
                let topic =
                    if c < result.cluster_topics.len() && !result.cluster_topics[c].is_empty() {
                        result.cluster_topics[c].as_str()
                    } else {
                        "(no data)"
                    };

                let count = result.tweet_indices_for_cluster(c).len();
                let max_topic_len = list_area.width.saturating_sub(18) as usize;
                let display_topic = truncate_for_width(topic, max_topic_len);

                ListItem::new(Line::from(vec![
                    Span::styled("█ ", Style::default().fg(color)),
                    Span::styled(
                        format!("C{c}"),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!(" ({count})"), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!(": {display_topic}"),
                        Style::default().fg(Color::White),
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

        let mut state = ListState::default().with_selected(Some(selected_index));
        StatefulWidget::render(list, list_area, buf, &mut state);
    }

    /// Render tweet list mode: scrollable list of tweets within a selected cluster.
    fn render_tweet_list(
        result: &ClusterResult,
        cluster: usize,
        selected_index: usize,
        area: Rect,
        buf: &mut Buffer,
    ) {
        let (_, color) = CLUSTER_COLORS[cluster % CLUSTER_COLORS.len()];
        let topic = if cluster < result.cluster_topics.len()
            && !result.cluster_topics[cluster].is_empty()
        {
            result.cluster_topics[cluster].as_str()
        } else {
            "(no data)"
        };

        let max_title_len = area.width.saturating_sub(10) as usize;
        let display_topic = truncate_for_width(topic, max_title_len);

        let block = Block::default()
            .title(format!(
                " C{cluster}: {display_topic} (Enter for thread, Esc to go back) "
            ))
            .title_style(Style::default().fg(color).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL);

        let texts = result.texts_for_cluster(cluster);
        let items: Vec<ListItem> = texts
            .iter()
            .map(|(_, text)| {
                let display = text.replace('\n', " ");
                ListItem::new(Line::from(Span::styled(
                    display,
                    Style::default().fg(Color::White),
                )))
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

        let mut state = ListState::default().with_selected(Some(selected_index));
        StatefulWidget::render(list, area, buf, &mut state);
    }
}

impl Widget for ClusterView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let source_label_owned = self.app.cluster_source.map(|s| s.to_string());
        let source_label = source_label_owned.as_deref();
        let title = match source_label {
            Some(label) => format!(" Topic Clusters — {label} "),
            None => " Topic Clusters ".to_string(),
        };

        if self.app.cluster_loading {
            let block = Block::default().title(title).borders(Borders::ALL);
            block.render(area, buf);
            // Clustering is always slow — show skeleton immediately (no debounce).
            let elapsed_ms = self.app.skeleton_elapsed_ms_immediate();
            render_cluster_skeleton(elapsed_ms, area, buf);
            return;
        }

        let Some(ref result) = self.app.cluster_result else {
            let block = Block::default().title(title).borders(Borders::ALL);
            let empty = Paragraph::new("No cluster data. Use :cluster to compute.").block(block);
            empty.render(area, buf);
            return;
        };

        let selected_index = self.app.selected_index();

        if let Some(cluster) = self.app.selected_cluster {
            Self::render_tweet_list(result, cluster, selected_index, area, buf);
        } else {
            Self::render_cluster_list(
                result,
                selected_index,
                self.app.cluster_topics_loading,
                self.app.resolved_chat_provider_name(),
                source_label,
                area,
                buf,
            );
        }
    }
}
