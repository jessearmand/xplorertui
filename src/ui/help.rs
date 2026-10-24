use crate::ui::text::wrap_to_width;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
#[cfg(test)]
use unicode_width::UnicodeWidthStr;

/// Leading spaces before the key label.
const KEY_INDENT: usize = 2;
/// Width of the left-padded key label column.
const KEY_WIDTH: usize = 12;
/// Column at which descriptions begin (and where continuation rows align).
const DESC_COL: usize = KEY_INDENT + KEY_WIDTH;

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
        // Center a panel that's 60 wide, 30 tall (or fit to area)
        let width = 60u16.min(area.width.saturating_sub(4));
        let height = 36u16.min(area.height.saturating_sub(2));
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

        // Width available for description text (after key column).
        // Saturates to 1 if the panel is somehow narrower than DESC_COL,
        // so wrap_text never receives 0 and produces at least one row.
        let desc_width = (inner.width as usize).saturating_sub(DESC_COL).max(1);

        let mut bindings: Vec<Line<'static>> = Vec::new();
        let push_section = |b: &mut Vec<Line<'static>>, name: &str| {
            b.push(Line::from(Span::styled(name.to_string(), section_style)));
        };
        let push_binding = |b: &mut Vec<Line<'static>>, key: &str, desc: &str| {
            b.extend(binding_lines(key, desc, key_style, desc_style, desc_width));
        };

        push_section(&mut bindings, "Navigation");
        push_binding(&mut bindings, "j/Down", "Move down");
        push_binding(&mut bindings, "k/Up", "Move up");
        push_binding(&mut bindings, "Enter", "Open selected item");
        push_binding(&mut bindings, "Esc/q", "Go back / close");
        push_binding(&mut bindings, "n", "Load next page");
        push_binding(&mut bindings, "r", "Refresh current view");
        push_binding(&mut bindings, "y", "Copy tweet URL");
        push_binding(&mut bindings, "o", "Open tweet in browser");
        push_binding(&mut bindings, "m", "Show full status message");
        push_binding(
            &mut bindings,
            "[ / ]",
            "Previous / next photo (thread view)",
        );
        push_binding(&mut bindings, "j/k", "Scroll a long popup message");
        bindings.push(Line::from(""));

        push_section(&mut bindings, "Views");
        push_binding(&mut bindings, "1", "Following timeline");
        push_binding(&mut bindings, "2", "Mentions");
        push_binding(&mut bindings, "3", "Bookmarks");
        push_binding(&mut bindings, "4", "Search");
        push_binding(&mut bindings, "?", "This help screen");
        bindings.push(Line::from(""));

        push_section(&mut bindings, "Input");
        push_binding(&mut bindings, ":", "Command mode");
        push_binding(&mut bindings, "/", "Search tweets");
        push_binding(&mut bindings, "@", "Look up user");
        push_binding(&mut bindings, "Ctrl-C", "Quit");
        bindings.push(Line::from(""));

        push_section(&mut bindings, "Commands");
        push_binding(&mut bindings, ":auth", "Authenticate (X OAuth2 PKCE)");
        push_binding(&mut bindings, ":or-auth", "Authenticate (OpenRouter)");
        push_binding(&mut bindings, ":embeddings", "Select embedding model");
        push_binding(
            &mut bindings,
            ":openrouter",
            "Select OpenRouter chat model (alias :openrouter-models)",
        );
        push_binding(&mut bindings, ":hf-models", "Browse HuggingFace MLX models");
        push_binding(
            &mut bindings,
            ":provider",
            "Set chat provider (mlx|llama|openrouter|auto)",
        );
        push_binding(
            &mut bindings,
            ":cluster",
            "Cluster current view (following/mentions/search/bookmarks)",
        );
        push_binding(
            &mut bindings,
            ":topics",
            "Regenerate cluster topic labels (Jev or chat LLM)",
        );
        push_binding(&mut bindings, ":refresh", "Refresh current view");
        push_binding(&mut bindings, ":quit", "Quit");

        let paragraph = Paragraph::new(bindings);

        let [content_area] = Layout::vertical([Constraint::Min(0)]).areas(inner);
        paragraph.render(content_area, buf);
    }
}

/// Build one or more `Line`s for a single binding, hanging-indenting any
/// wrapped continuation rows so they align under the description column.
fn binding_lines(
    key: &str,
    desc: &str,
    key_style: Style,
    desc_style: Style,
    desc_width: usize,
) -> Vec<Line<'static>> {
    let chunks = wrap_to_width(desc, desc_width);
    let mut lines = Vec::with_capacity(chunks.len().max(1));

    let mut iter = chunks.into_iter();
    let first = iter.next().unwrap_or_default();
    lines.push(Line::from(vec![
        Span::styled(
            format!(
                "{:indent$}{:<width$}",
                "",
                key,
                indent = KEY_INDENT,
                width = KEY_WIDTH
            ),
            key_style,
        ),
        Span::styled(first, desc_style),
    ]));
    for chunk in iter {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(DESC_COL)),
            Span::styled(chunk, desc_style),
        ]));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(chunks: &[String]) -> Vec<usize> {
        chunks
            .iter()
            .map(|s| UnicodeWidthStr::width(s.as_str()))
            .collect()
    }

    #[test]
    fn long_help_string_wraps_under_budget() {
        // The actual problem case from the help screen — :openrouter desc.
        let desc = "Select OpenRouter chat model (alias :openrouter-models)";
        let out = wrap_to_width(desc, 46); // 60 panel width - 2 borders - DESC_COL(14)
        for w in widths(&out) {
            assert!(w <= 46, "chunk too wide: {w} > 46");
        }
        // Reconstructing with single spaces should give us back the
        // whitespace-collapsed original.
        assert_eq!(out.join(" "), desc);
    }
}
