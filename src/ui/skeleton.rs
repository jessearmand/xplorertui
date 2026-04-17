use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Clear, Widget};
use tui_skeleton::{AnimationMode, SkeletonBlock, SkeletonList};

/// Base and highlight colors for skeleton animations.
///
/// Noise mode uses a constant 0.3 intensity, so the rendered color is
/// `base + 0.3 * (highlight - base)`. DarkGray→Gray gives ~Rgb(140,140,140)
/// which is clearly visible as animated braille static on dark terminals.
const SKELETON_BASE: tui_skeleton::Color = tui_skeleton::Color::DarkGray;
const SKELETON_HIGHLIGHT: tui_skeleton::Color = tui_skeleton::Color::Gray;

/// Render a skeleton list resembling tweet cards in the given area.
///
/// Used by `TimelineView` when loading and the debounce threshold has passed.
pub fn render_timeline_skeleton(elapsed_ms: u64, title: &str, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title))
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width < 4 || inner.height < 2 {
        return;
    }

    let skeleton = SkeletonList::new(elapsed_ms)
        .mode(AnimationMode::Noise)
        .base(SKELETON_BASE)
        .highlight(SKELETON_HIGHLIGHT)
        .items((inner.height / 3).max(1));

    skeleton.render(inner, buf);
}

/// Render a skeleton list for model loading views (OpenRouter, HuggingFace).
///
/// Shows skeleton rows inside the given block area. Used immediately (no
/// debounce) since model fetches are always network-bound.
pub fn render_models_skeleton(elapsed_ms: u64, title: &str, area: Rect, buf: &mut Buffer) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title))
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    block.render(area, buf);

    if inner.width < 4 || inner.height < 2 {
        return;
    }

    let skeleton = SkeletonList::new(elapsed_ms)
        .mode(AnimationMode::Noise)
        .base(SKELETON_BASE)
        .highlight(SKELETON_HIGHLIGHT)
        .items((inner.height / 2).max(1));

    skeleton.render(inner, buf);
}

/// Render a skeleton block inside a centered popup for cluster loading.
///
/// Replaces the static "Computing Clusters..." popup with animated noise.
pub fn render_cluster_skeleton(elapsed_ms: u64, area: Rect, buf: &mut Buffer) {
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 7u16.min(area.height.saturating_sub(2));
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

    let skeleton = SkeletonBlock::new(elapsed_ms)
        .mode(AnimationMode::Noise)
        .base(SKELETON_BASE)
        .highlight(SKELETON_HIGHLIGHT);

    skeleton.render(inner, buf);
}
