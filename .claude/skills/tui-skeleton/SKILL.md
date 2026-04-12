---
name: tui-skeleton
description: >
  Guide for using the tui-skeleton crate to add animated skeleton loading
  placeholders to Ratatui terminal UIs. Use this skill whenever adding loading
  animations, skeleton placeholders, shimmer effects, or progress indicators in
  a Ratatui application. Also trigger when the user mentions tui-skeleton,
  skeleton loading in Ratatui, animated loading widgets, or asks how to show
  placeholders while async data loads in a Ratatui TUI. Covers widget selection,
  animation modes (Noise/Breathe/Sweep/Plasma), color math pitfalls, debounce
  patterns, and adaptive tick rates.
---

# tui-skeleton — Animated Skeleton Loading for Ratatui

`tui-skeleton` (v0.3+) provides stateless skeleton loading widgets for Ratatui apps. All animation state is derived from a single `elapsed_ms: u64` timestamp — no mutable state, no tick counters, no animation structs. Pass the time, get the frame.

## Dependency

```toml
[dependencies]
tui-skeleton = "0.3"
```

Requires `ratatui-core 0.1` and `ratatui-widgets 0.3` (same versions used by `ratatui 0.29+`). The crate re-exports `Color`, `Constraint`, and `Block` so you can avoid adding direct ratatui-core dependencies.

## Core Concepts

### Stateless, Timestamp-Driven

Store a single `Instant` at app creation. Each frame, compute `elapsed_ms`:

```rust
use std::time::Instant;

struct App {
    epoch: Instant,
    // ...
}

impl App {
    fn elapsed_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}
```

Pass this to any skeleton widget — the animation frame is computed deterministically from the timestamp.

### Two Axes of Appearance

Every widget has two orthogonal controls:

1. **Animation mode** (`AnimationMode`) — how brightness changes over time
2. **Fill variant** (`.braille(bool)`) — character used: solid `█` or braille `⣿`

Noise mode ignores the braille flag and always uses random braille glyphs.

## Animation Modes

| Mode | Cycle | Behavior | Intensity |
|------|-------|----------|-----------|
| `Breathe` (default) | 5s | Uniform sine pulse, all cells same brightness | 0.0–1.0 |
| `Sweep` | 2.8s (0.8s active + 2s rest) | Cosine highlight window travels left→right | 0.0–1.0 positional |
| `Plasma` | 4s | Dual sine waves, organic shifting patterns | 0.0–0.6 |
| `Noise` | None | Random braille glyph per cell per frame (TV static) | **Constant 0.3** |

### Critical: Noise Mode Color Math

Noise uses a **fixed intensity of 0.3** — the animation comes from changing *glyphs*, not changing *brightness*. The rendered color is always:

```
rendered = base + 0.3 * (highlight - base)
```

This means your color choice matters far more for Noise than for other modes. With dark colors, the result can be invisible:

```rust
// BAD — invisible on dark terminals:
// Rgb(40,40,40) + 0.3*(Rgb(70,70,80) - Rgb(40,40,40)) = Rgb(49, 49, 52)
.base(Color::Rgb(40, 40, 40))
.highlight(Color::Rgb(70, 70, 80))

// GOOD — use the crate defaults (DarkGray/Gray):
// Rgb(128,128,128) + 0.3*(Rgb(169,169,169) - Rgb(128,128,128)) = Rgb(140, 140, 140)
// (omit .base()/.highlight() to use defaults, or set explicitly:)
.base(Color::DarkGray)
.highlight(Color::Gray)
```

The crate's default colors (`DarkGray`/`Gray`) are chosen to be visible at the 0.3 intensity. If you customize colors, always compute the rendered result first:
- Named colors: `DarkGray` = `Rgb(128,128,128)`, `Gray` = `Rgb(169,169,169)`, `White` = `Rgb(255,255,255)`
- Other named colors default to `Rgb(128,128,128)` in the interpolation

## Available Widgets

| Widget | Shape | Good for |
|--------|-------|----------|
| `SkeletonBlock` | Solid filled rectangle | Popups, cards, generic areas |
| `SkeletonList` | Spaced rows with ragged right edges | Menus, sidebars, item lists |
| `SkeletonText` | Paragraph with varying line widths | Content areas, descriptions |
| `SkeletonStreamingText` | Typewriter fill left→right, top→bottom | Chat messages, streaming responses |
| `SkeletonTable` | Rows with column separators, zebra striping | Data tables, grids |
| `SkeletonBarChart` | Vertical bars rising from bottom | Analytics, dashboards |
| `SkeletonHBarChart` | Horizontal bars from left | Horizontal metrics |
| `SkeletonBrailleBar` | Braille progress bars with peak marker | Progress indicators |
| `SkeletonKvTable` | Key-value pairs layout | Detail panels, properties |
| `SkeletonLineChart` | Braille line chart with wave traces | Time series, graphs |

## Builder Pattern

All widgets follow the same builder API:

```rust
use tui_skeleton::{SkeletonList, SkeletonBlock, SkeletonText, AnimationMode};
use ratatui::widgets::Widget;

let elapsed_ms = app.elapsed_ms();

// List (for menus, timelines)
let list = SkeletonList::new(elapsed_ms)
    .mode(AnimationMode::Noise)
    .items(8);  // number of list items (default: 5)

// Block (for popups, cards)
let block = SkeletonBlock::new(elapsed_ms)
    .mode(AnimationMode::Sweep);

// Text (for content areas)
let text = SkeletonText::new(elapsed_ms)
    .mode(AnimationMode::Breathe);

// Streaming text (for chat/LLM responses)
let streaming = SkeletonStreamingText::new(elapsed_ms)
    .lines(5)
    .duration_ms(3000)  // typewriter fill duration
    .repeat(true);      // loop the animation

// All render via standard Widget trait
list.render(area, buf);
```

### Common Builder Methods (all widgets)

- `.mode(AnimationMode)` — animation style (default: `Breathe`)
- `.braille(bool)` — use braille fill `⣿` instead of solid `█` (default: `false`)
- `.base(impl Into<Color>)` — dim resting color (default: `DarkGray`)
- `.highlight(impl Into<Color>)` — peak brightness color (default: `Gray`)
- `.block(Block)` — optional border/title container

## Integration Pattern

### Conditional Rendering

Show skeleton while loading, real content when ready:

```rust
impl Widget for MyView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.loading {
            SkeletonList::new(self.app.elapsed_ms())
                .mode(AnimationMode::Noise)
                .items(8)
                .render(area, buf);
        } else {
            // render real content
        }
    }
}
```

### Debounce Pattern

Avoid skeleton flicker on fast responses by only showing skeletons after a threshold:

```rust
struct App {
    epoch: Instant,
    loading_started_at: Option<Instant>,
}

impl App {
    /// Returns elapsed_ms only after 200ms of loading (debounce).
    /// Use for network fetches where fast responses shouldn't flicker.
    fn skeleton_elapsed_ms(&self) -> Option<u64> {
        let started = self.loading_started_at?;
        if started.elapsed().as_millis() < 200 {
            return None;
        }
        Some(self.epoch.elapsed().as_millis() as u64)
    }

    /// Returns elapsed_ms immediately (no debounce).
    /// Use for known-slow operations (ML computation, large API calls).
    fn skeleton_elapsed_ms_immediate(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn mark_loading_started(&mut self) {
        if self.loading_started_at.is_none() {
            self.loading_started_at = Some(Instant::now());
        }
    }
}
```

Then in the UI, provide a text fallback during the debounce window:

```rust
if self.loading && self.data.is_empty() {
    if let Some(elapsed_ms) = self.app.skeleton_elapsed_ms() {
        // Past debounce threshold — show animated skeleton
        SkeletonList::new(elapsed_ms)
            .mode(AnimationMode::Noise)
            .render(inner, buf);
    } else {
        // Within debounce — show simple text fallback
        buf.set_string(x, y, "Loading...", Style::default().fg(Color::DarkGray));
    }
    return;
}
```

### Popup Skeleton

For centered loading popups (e.g., computing clusters):

```rust
fn render_loading_popup(elapsed_ms: u64, area: Rect, buf: &mut Buffer) {
    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 7u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    Clear.render(popup, buf);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Processing... ")
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(popup);
    block.render(popup, buf);

    SkeletonBlock::new(elapsed_ms)
        .mode(AnimationMode::Noise)
        .render(inner, buf);
}
```

## Frame Rate and Adaptive Tick Rate

### Determining your app's frame rate

A Ratatui app's frame rate is determined by its **event loop tick interval**, not by the terminal or display. Before making any assumptions about FPS, **search the app's event loop code** for the actual tick configuration. Do not assume a value — find it.

Look for these patterns in the event handler / main loop:
- A `TICK_FPS`, `FPS`, or `TICK_RATE` constant
- A `Duration::from_millis(...)` used as a tick interval or poll timeout
- A `crossterm::event::poll(duration)` call
- A `tokio::time::interval(duration)` in a background event task

The tick interval drives how often `draw()` is called. tui-skeleton needs at least 20 FPS (50ms tick interval) for smooth animation. If the app's tick rate is slower, you'll need to use the adaptive tick rate pattern below.

### Adaptive tick rate

The crate exports two duration constants for apps that want to save CPU when idle:

```rust
use tui_skeleton::{TICK_ANIMATED, TICK_IDLE};
// TICK_ANIMATED = 50ms (20 FPS) — use when skeletons are visible
// TICK_IDLE = 200ms (5 FPS) — use when all data has loaded
```

If your app already ticks at 20+ FPS, you don't need to change anything for smooth animations. For apps with slower tick rates (common in simple TUIs), switch your event poll timeout to `TICK_ANIMATED` while any skeleton is visible, then back to `TICK_IDLE` when loading finishes. This keeps CPU usage low during idle while delivering smooth animations during loading.

## Pitfalls

1. **Noise mode colors too dark** — The #1 mistake. Always verify your rendered color at 0.3 intensity. When in doubt, use the defaults (omit `.base()`/`.highlight()`).

2. **Skeleton renders but looks blank** — Check that the area has sufficient dimensions. Widgets return early if `inner.is_empty()`. For `SkeletonList`, each item needs 2 rows (1 content + 1 gap), so `items * 2` must be <= area height.

3. **Animation not updating** — Make sure `elapsed_ms` actually changes between frames. It must come from a monotonic clock (e.g., `Instant::now()`), not a cached value.

4. **Loading flag asymmetry** — If you set `timeline.loading = false` on response but never set it to `true` on dispatch, the skeleton will never appear. Always pair: set `true` when dispatching, set `false` when receiving.
