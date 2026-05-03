use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
            "Set chat provider (mlx|openrouter|auto)",
        );
        push_binding(
            &mut bindings,
            ":cluster",
            "Cluster current view (following/mentions/search/bookmarks)",
        );
        push_binding(
            &mut bindings,
            ":topics",
            "Regenerate cluster topic labels via LLM",
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
    let chunks = wrap_text(desc, desc_width);
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

/// Greedy word-wrap that respects terminal display width (CJK-aware).
///
/// Returns chunks each having `UnicodeWidthStr::width(chunk) <= max_cols`,
/// with one documented exception: if a single character is wider than
/// `max_cols` (e.g. a CJK glyph at 2 cols when `max_cols == 1`), it is
/// placed on its own line as best-effort — splitting a single grapheme is
/// not meaningful, and the renderer's `Paragraph` will clip the overflow.
///
/// Always returns at least one chunk (an empty string for empty input)
/// so callers can rely on `chunks[0]` for the first row of a binding.
///
/// Algorithm:
/// 1. Iterate `split_whitespace()` tokens (collapses internal whitespace
///    runs and trims leading/trailing — fine for help text).
/// 2. For each token of width `<= max_cols`, try to append to the current
///    line with one separator space. If it would overflow, flush the
///    current line and start a new one with the token.
/// 3. For a token wider than `max_cols`, flush the current line first to
///    keep word order, then walk the token char-by-char, flushing every
///    time the next char would overflow. After the token, continue
///    packing subsequent tokens onto the trailing partial chunk so we
///    don't waste columns.
///
/// Per-char width uses `UnicodeWidthChar::width(c).unwrap_or(0)` so
/// combining marks (width `None`) and control chars contribute 0 — they
/// piggyback on the previous char's column without consuming budget,
/// matching how terminals actually render them.
fn wrap_text(text: &str, max_cols: usize) -> Vec<String> {
    if max_cols == 0 {
        // Defensive: caller saturates to 1, but never panic on a 0 budget.
        return vec![String::new()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w: usize = 0;

    for token in text.split_whitespace() {
        let token_w = UnicodeWidthStr::width(token);

        if token_w <= max_cols {
            // Token fits on a line by itself — try to append to current.
            let sep = if current.is_empty() { 0 } else { 1 };
            if current_w + sep + token_w <= max_cols {
                if sep == 1 {
                    current.push(' ');
                }
                current.push_str(token);
                current_w += sep + token_w;
            } else {
                lines.push(std::mem::take(&mut current));
                current.push_str(token);
                current_w = token_w;
            }
        } else {
            // Token wider than `max_cols` — hard-break char by char.
            // Flush the current line first so word order stays intact.
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_w = 0;
            }
            for c in token.chars() {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                if current_w + cw > max_cols && !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                    current_w = 0;
                }
                current.push(c);
                current_w += cw;
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
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
    fn empty_input_returns_single_empty_chunk() {
        assert_eq!(wrap_text("", 10), vec![String::new()]);
    }

    #[test]
    fn whitespace_only_returns_single_empty_chunk() {
        assert_eq!(wrap_text("   \t \n  ", 10), vec![String::new()]);
    }

    #[test]
    fn fits_in_one_line() {
        assert_eq!(wrap_text("hello world", 20), vec!["hello world"]);
    }

    #[test]
    fn greedy_packs_words() {
        let out = wrap_text("the quick brown fox jumps", 10);
        // "the quick" (9) | "brown fox" (9) | "jumps" (5)
        assert_eq!(out, vec!["the quick", "brown fox", "jumps"]);
        for w in widths(&out) {
            assert!(w <= 10);
        }
    }

    #[test]
    fn long_word_hard_breaks() {
        let out = wrap_text("abcdefghijklmnop", 5);
        // 16 chars, 5 cols each → "abcde", "fghij", "klmno", "p"
        assert_eq!(out, vec!["abcde", "fghij", "klmno", "p"]);
        for w in widths(&out) {
            assert!(w <= 5);
        }
    }

    #[test]
    fn long_word_after_short_word_flushes_first() {
        let out = wrap_text("hi supercalifragilistic world", 10);
        // "hi" stays its own line; long word breaks; "world" tail-packs.
        assert_eq!(out, vec!["hi", "supercalif", "ragilistic", "world"]);
    }

    #[test]
    fn cjk_counts_double_width() {
        // 4 CJK chars = 8 cols; max_cols = 4 → 2 chars per chunk.
        let out = wrap_text("脳波再生", 4);
        assert_eq!(out, vec!["脳波", "再生"]);
        for w in widths(&out) {
            assert!(w <= 4);
        }
    }

    #[test]
    fn cjk_token_in_word_stream() {
        // ASCII + CJK token wider than budget → hard-breaks the CJK.
        let out = wrap_text("hi 脳波再生デモ end", 6);
        // "hi" (2) | "脳波再" (6) | "生デモ" (6) | "end" (3)
        assert_eq!(out, vec!["hi", "脳波再", "生デモ", "end"]);
        for w in widths(&out) {
            assert!(w <= 6);
        }
    }

    #[test]
    fn combining_marks_do_not_consume_budget() {
        // "e\u{0301}" renders as 'é' (1 col); width-zero combining mark.
        let out = wrap_text("e\u{0301}xample fits", 8);
        // Width: 7 ("éxample" 7 + " " 1 + "fits" 4 = 12 > 8) — wait, that's
        // "éxample" = 7 cols, " fits" needs 5 more → 12 total > 8, so wraps.
        assert_eq!(out, vec!["e\u{0301}xample", "fits"]);
    }

    #[test]
    fn single_char_wider_than_max_is_best_effort() {
        // max_cols = 1, but '脳' is 2 cols. Documented best-effort: char
        // gets its own line, width exceeds budget, no panic.
        let out = wrap_text("脳波", 1);
        assert_eq!(out, vec!["脳", "波"]);
    }

    #[test]
    fn zero_max_returns_single_empty_chunk() {
        // Defensive — caller saturates to 1, but we don't panic.
        assert_eq!(wrap_text("anything", 0), vec![String::new()]);
    }

    #[test]
    fn never_returns_empty_vec() {
        for max in [1usize, 2, 5, 10, 80] {
            for input in ["", " ", "a", "abc def", "脳波 hello"] {
                let out = wrap_text(input, max);
                assert!(!out.is_empty(), "empty for input={input:?} max={max}");
            }
        }
    }

    #[test]
    fn long_help_string_wraps_under_budget() {
        // The actual problem case from the help screen — :openrouter desc.
        let desc = "Select OpenRouter chat model (alias :openrouter-models)";
        let out = wrap_text(desc, 46); // 60 panel width - 2 borders - DESC_COL(14)
        for w in widths(&out) {
            assert!(w <= 46, "chunk too wide: {w} > 46");
        }
        // Reconstructing with single spaces should give us back the
        // whitespace-collapsed original.
        assert_eq!(out.join(" "), desc);
    }
}
