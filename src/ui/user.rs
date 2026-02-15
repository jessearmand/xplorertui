use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::api::types::User;

/// User profile view showing stats, bio, and info.
pub struct UserProfileView<'a> {
    pub user: &'a User,
}

impl<'a> UserProfileView<'a> {
    pub fn new(user: &'a User) -> Self {
        Self { user }
    }
}

impl Widget for UserProfileView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" @{} ", self.user.username))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::DarkGray));

        let inner = block.inner(area);
        block.render(area, buf);

        let [info_area, bio_area] =
            Layout::vertical([Constraint::Length(8), Constraint::Min(1)]).areas(inner);

        // -- Info section --
        let mut lines = Vec::new();

        // Display name
        let name_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        lines.push(Line::from(vec![
            Span::styled(&self.user.name, name_style),
            if self.user.verified.unwrap_or(false) {
                Span::styled(" \u{2713}", Style::default().fg(Color::Blue))
            } else {
                Span::raw("")
            },
        ]));

        lines.push(Line::from(Span::styled(
            format!("@{}", self.user.username),
            Style::default().fg(Color::DarkGray),
        )));

        lines.push(Line::from(""));

        // Metrics
        if let Some(ref m) = self.user.public_metrics {
            lines.push(Line::from(vec![
                Span::styled(
                    format_count(m.followers_count),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Followers  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format_count(m.following_count),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Following  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format_count(m.tweet_count),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Posts", Style::default().fg(Color::DarkGray)),
            ]));
        }

        // Location
        if let Some(ref loc) = self.user.location {
            lines.push(Line::from(vec![
                Span::styled("\u{1F4CD} ", Style::default().fg(Color::Red)),
                Span::raw(loc.as_str()),
            ]));
        }

        // URL
        if let Some(ref url) = self.user.url {
            lines.push(Line::from(vec![
                Span::styled("\u{1F517} ", Style::default().fg(Color::Blue)),
                Span::styled(url.as_str(), Style::default().fg(Color::Blue)),
            ]));
        }

        // Joined date
        if let Some(ref dt) = self.user.created_at {
            lines.push(Line::from(vec![
                Span::styled("\u{1F4C5} Joined ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    dt.format("%B %Y").to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }

        let info_para = Paragraph::new(lines);
        info_para.render(info_area, buf);

        // -- Bio section --
        if let Some(ref desc) = self.user.description {
            let bio_block = Block::default()
                .borders(Borders::TOP)
                .title(" Bio ")
                .title_style(Style::default().fg(Color::DarkGray))
                .border_style(Style::default().fg(Color::DarkGray));

            let bio_inner = bio_block.inner(bio_area);
            bio_block.render(bio_area, buf);

            let bio_para =
                Paragraph::new(desc.as_str()).wrap(ratatui::widgets::Wrap { trim: true });
            bio_para.render(bio_inner, buf);
        }
    }
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
