use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, AppMode};
use crate::event::{AppEvent, ViewKind};

impl App {
    // -- Key event routing --------------------------------------------------

    pub(super) fn handle_key_event(&mut self, key: KeyEvent) {
        // Ctrl-C always quits.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            self.events.send(AppEvent::Quit);
            return;
        }

        // Dismiss error popup if open (swallow all other keys).
        if self.error_detail.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                    self.error_detail = None;
                }
                _ => {}
            }
            return;
        }

        // Handle model filter popup if open (swallow all keys).
        if self.model_filter_open {
            self.handle_filter_popup_key(key);
            return;
        }

        match self.mode {
            AppMode::Normal => self.handle_normal_key(key),
            AppMode::Command => self.handle_command_key(key),
            AppMode::Search => self.handle_search_key(key),
        }
    }

    fn handle_filter_popup_key(&mut self, key: KeyEvent) {
        let providers = self.model_providers();
        // Popup items: "All" + each provider
        let item_count = providers.len() + 1;

        match key.code {
            KeyCode::Esc | KeyCode::Char('f') => {
                self.model_filter_open = false;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.model_filter_index + 1 < item_count {
                    self.model_filter_index += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.model_filter_index = self.model_filter_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                if self.model_filter_index == 0 {
                    self.model_filter = None;
                } else if let Some(provider) = providers.get(self.model_filter_index - 1) {
                    self.model_filter = Some(provider.clone());
                }
                self.model_filter_open = false;
                // Reset model selection since the filtered list changed
                if let Some(vs) = self.view_stack.last_mut() {
                    vs.selected_index = 0;
                }
            }
            _ => {}
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                // In cluster tweet list mode, go back to cluster list first
                if self.current_view() == Some(&ViewKind::Cluster)
                    && self.selected_cluster.is_some()
                {
                    let cluster_idx = self.selected_cluster.unwrap();
                    self.selected_cluster = None;
                    if let Some(vs) = self.view_stack.last_mut() {
                        vs.selected_index = cluster_idx;
                    }
                } else if self.view_stack.len() > 1 {
                    self.events.send(AppEvent::PopView);
                } else {
                    self.events.send(AppEvent::Quit);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection_up();
            }
            KeyCode::Enter => {
                self.open_selected();
            }
            KeyCode::Char('/') => {
                self.mode = AppMode::Search;
                self.search_input.clear();
            }
            KeyCode::Char(':') => {
                self.mode = AppMode::Command;
                self.command_input.clear();
            }
            KeyCode::Char('?') => {
                self.events.send(AppEvent::PushView(ViewKind::Help));
            }
            KeyCode::Char('1') => {
                self.events.send(AppEvent::SwitchView(ViewKind::Home));
            }
            KeyCode::Char('2') => {
                self.events.send(AppEvent::SwitchView(ViewKind::Mentions));
            }
            KeyCode::Char('3') => {
                self.events.send(AppEvent::SwitchView(ViewKind::Bookmarks));
            }
            KeyCode::Char('4') => {
                self.events.send(AppEvent::SwitchView(ViewKind::Search));
            }
            KeyCode::Char('@') => {
                self.mode = AppMode::Command;
                self.command_input = "user ".to_string();
            }
            KeyCode::Char('n') => {
                self.load_next_page();
            }
            KeyCode::Char('y') => {
                self.copy_tweet_url();
            }
            KeyCode::Char('o') => {
                self.open_tweet_url();
            }
            KeyCode::Char('r') => {
                self.events.send(AppEvent::RefreshView);
            }
            KeyCode::Char('f') => {
                if matches!(
                    self.current_view(),
                    Some(ViewKind::OpenRouterModels | ViewKind::TextModels)
                ) {
                    self.model_filter_open = true;
                    self.model_filter_index = 0;
                }
            }
            _ => {}
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.command_input.clear();
            }
            KeyCode::Enter => {
                self.execute_command();
                self.mode = AppMode::Normal;
            }
            KeyCode::Backspace => {
                self.command_input.pop();
            }
            KeyCode::Char(c) => {
                self.command_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = AppMode::Normal;
                self.search_input.clear();
            }
            KeyCode::Enter => {
                let query = self.search_input.clone();
                if !query.is_empty() {
                    self.search_query = query.clone();
                    self.events.send(AppEvent::FetchSearch {
                        query,
                        pagination_token: None,
                    });
                    self.events.send(AppEvent::SwitchView(ViewKind::Search));
                }
                self.mode = AppMode::Normal;
            }
            KeyCode::Backspace => {
                self.search_input.pop();
            }
            KeyCode::Char(c) => {
                self.search_input.push(c);
            }
            _ => {}
        }
    }
}
