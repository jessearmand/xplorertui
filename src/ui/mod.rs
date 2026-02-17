pub mod bookmarks;
pub mod command_bar;
pub mod help;
pub mod input;
pub mod search;
pub mod status_bar;
pub mod thread;
pub mod timeline;
pub mod tweet;
pub mod user;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};

use crate::app::{App, AppMode};
use crate::event::ViewKind;

use bookmarks::BookmarksView;
use command_bar::CommandBar;
use help::HelpView;
use search::SearchView;
use status_bar::StatusBar;
use thread::ThreadView;
use timeline::TimelineView;
use user::UserProfileView;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Layout: main content + status bar + optional command bar
    let bottom_height = if app.mode != AppMode::Normal { 2 } else { 1 };

    let [main_area, bottom_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(bottom_height)]).areas(area);

    // Split bottom into status bar and optional command bar
    if app.mode != AppMode::Normal {
        let [status_area, cmd_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(bottom_area);
        frame.render_widget(StatusBar::new(app), status_area);
        frame.render_widget(CommandBar::new(app), cmd_area);
    } else {
        frame.render_widget(StatusBar::new(app), bottom_area);
    }

    // Render the current view
    match app.current_view() {
        Some(ViewKind::Home) => {
            frame.render_widget(
                TimelineView::new("Home", &app.home_timeline.tweets, app)
                    .loading(app.home_timeline.loading),
                main_area,
            );
        }
        Some(ViewKind::Mentions) => {
            frame.render_widget(
                TimelineView::new("Mentions", &app.mentions.tweets, app)
                    .loading(app.mentions.loading),
                main_area,
            );
        }
        Some(ViewKind::Bookmarks) => {
            frame.render_widget(BookmarksView::new(app), main_area);
        }
        Some(ViewKind::Search) => {
            frame.render_widget(SearchView::new(app), main_area);
        }
        Some(ViewKind::UserTimeline(user_id)) => {
            let title = format!("Timeline: {user_id}");
            frame.render_widget(
                TimelineView::new(&title, &app.viewed_user_timeline.tweets, app)
                    .loading(app.viewed_user_timeline.loading),
                main_area,
            );
        }
        Some(ViewKind::Thread(conv_id)) => {
            let _ = conv_id; // conv_id is part of the ViewKind, thread data is in app state
            frame.render_widget(
                ThreadView::new(app.thread_root.as_ref(), &app.thread_tweets, app),
                main_area,
            );
        }
        Some(ViewKind::UserProfile(_)) => {
            if let Some(ref user) = app.viewed_user {
                frame.render_widget(UserProfileView::new(user), main_area);
            } else {
                frame.render_widget(
                    TimelineView::new("User Profile", &[], app).loading(true),
                    main_area,
                );
            }
        }
        Some(ViewKind::Help) => {
            // Render the view underneath first, then overlay help.
            render_previous_view(frame, app, main_area);
            frame.render_widget(HelpView::new(), main_area);
        }
        None => {
            frame.render_widget(TimelineView::new("xplorertui", &[], app), main_area);
        }
    }
}

/// Render the view underneath the current one (for overlay views like Help).
fn render_previous_view(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    if app.view_stack.len() < 2 {
        return;
    }

    let prev_view = &app.view_stack[app.view_stack.len() - 2];
    match &prev_view.kind {
        ViewKind::Home => {
            frame.render_widget(
                TimelineView::new("Home", &app.home_timeline.tweets, app),
                area,
            );
        }
        ViewKind::Mentions => {
            frame.render_widget(
                TimelineView::new("Mentions", &app.mentions.tweets, app),
                area,
            );
        }
        ViewKind::Bookmarks => {
            frame.render_widget(BookmarksView::new(app), area);
        }
        ViewKind::Search => {
            frame.render_widget(SearchView::new(app), area);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::draw;
    use crate::api::types::Tweet;
    use crate::app::App;
    use crate::auth::credentials::CredentialSet;
    use crate::config::AppConfig;

    fn make_tweet(id: &str) -> Tweet {
        Tweet {
            id: id.to_string(),
            text: format!("tweet {id}"),
            author_id: None,
            created_at: None,
            conversation_id: None,
            in_reply_to_user_id: None,
            lang: None,
            edit_history_tweet_ids: None,
            public_metrics: None,
            entities: None,
            referenced_tweets: None,
            attachments: None,
            note_tweet: None,
        }
    }

    fn buffer_lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        let area = *buffer.area();
        let mut lines = Vec::with_capacity(area.height as usize);

        for y in 0..area.height {
            let mut line = String::with_capacity(area.width as usize);
            for x in 0..area.width {
                if let Some(cell) = buffer.cell((x, y)) {
                    line.push_str(cell.symbol());
                }
            }
            lines.push(line);
        }

        lines
    }

    #[tokio::test]
    async fn draw_home_timeline_keeps_bottom_selection_visible_without_hanging() {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.home_timeline.tweets = vec![make_tweet("0"), make_tweet("1"), make_tweet("2")];

        // Height 11 -> main area 10 -> timeline inner height 8 after borders.
        // With 1-line tweet text, each card+separator is height 4, so items 0+1
        // exactly fill the viewport and selecting item 2 exercises the edge case.
        let backend = TestBackend::new(80, 11);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");

        for selected in 0..app.home_timeline.tweets.len() {
            let root_view = app
                .view_stack
                .last_mut()
                .expect("app always has at least one view");
            root_view.selected_index = selected;
            terminal
                .draw(|frame| draw(frame, &app))
                .expect("draw should complete");
        }

        let lines = buffer_lines(&terminal).join("\n");
        assert!(
            lines.contains("tweet 2"),
            "expected bottom-selected tweet text in rendered buffer, got:\n{lines}"
        );
    }
}
