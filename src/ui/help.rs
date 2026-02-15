use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

/// Help overlay showing keybindings.
#[derive(Default)]
pub struct HelpView;

impl HelpView {
    pub fn new() -> Self {
        Self
    }
}

impl Widget for HelpView {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Center a panel that's 60 wide, 22 tall (or fit to area)
        let width = 60u16.min(area.width.saturating_sub(4));
        let height = 22u16.min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let panel = Rect::new(x, y, width, height);

        Clear.render(panel, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Help - Keybindings ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(panel);
        block.render(panel, buf);

        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(Color::White);
        let section_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);

        let bindings: Vec<Line<'_>> = vec![
            Line::from(Span::styled("Navigation", section_style)),
            binding_line("j/Down", "Move down", key_style, desc_style),
            binding_line("k/Up", "Move up", key_style, desc_style),
            binding_line("Enter", "Open selected item", key_style, desc_style),
            binding_line("Esc/q", "Go back / close", key_style, desc_style),
            binding_line("n", "Load next page", key_style, desc_style),
            Line::from(""),
            Line::from(Span::styled("Views", section_style)),
            binding_line("1", "Home timeline", key_style, desc_style),
            binding_line("2", "Mentions", key_style, desc_style),
            binding_line("3", "Bookmarks", key_style, desc_style),
            binding_line("4", "Search", key_style, desc_style),
            binding_line("?", "This help screen", key_style, desc_style),
            Line::from(""),
            Line::from(Span::styled("Input", section_style)),
            binding_line(":", "Command mode", key_style, desc_style),
            binding_line("/", "Search tweets", key_style, desc_style),
            binding_line("@", "Look up user", key_style, desc_style),
            binding_line("Ctrl-C", "Quit", key_style, desc_style),
        ];

        let paragraph = Paragraph::new(bindings);

        let [content_area] = Layout::vertical([Constraint::Min(0)]).areas(inner);
        paragraph.render(content_area, buf);
    }
}

fn binding_line<'a>(key: &'a str, desc: &'a str, key_style: Style, desc_style: Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {key:<12}"), key_style),
        Span::styled(desc, desc_style),
    ])
}
