use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::app::App;
use crate::ui::timeline::TimelineView;

/// Search view: displays search results as a timeline.
pub struct SearchView<'a> {
    pub app: &'a App,
}

impl<'a> SearchView<'a> {
    pub fn new(app: &'a App) -> Self {
        Self { app }
    }
}

impl Widget for SearchView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = if self.app.search_query.is_empty() {
            "Search (press / to search)".to_string()
        } else {
            format!("Search: {}", self.app.search_query)
        };

        TimelineView::new(&title, &self.app.search_results.tweets, self.app)
            .loading(self.app.search_results.loading)
            .render(area, buf);
    }
}
