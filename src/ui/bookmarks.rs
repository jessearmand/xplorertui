use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::app::App;
use crate::ui::timeline::TimelineView;

/// Bookmarks view: displays bookmarked tweets as a timeline.
pub struct BookmarksView<'a> {
    pub app: &'a App,
}

impl<'a> BookmarksView<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl Widget for BookmarksView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        TimelineView::new("Bookmarks", &self.app.bookmarks.tweets, self.app)
            .loading(self.app.bookmarks.loading)
            .render(area, buf);
    }
}
