//! Modal popups: full-text message details and yes/no prompts.
//!
//! At most one popup is open at a time (`App::popup`). While it is open it
//! swallows all keys except Ctrl-C.

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, ClusterSource};
use crate::event::{AppEvent, ViewKind};

/// Visual tone of a popup (border and title color).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupTone {
    Error,
    Info,
    Prompt,
}

/// What happens when the user accepts a [`Popup::Confirm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptAction {
    /// Open the embedding model picker, then cluster `resume` once a model
    /// is selected.
    SelectEmbeddingModel { resume: ClusterSource },
    /// Run the OpenRouter auth flow, then continue as `SelectEmbeddingModel`.
    OpenRouterAuth { resume: ClusterSource },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    /// Read-only text, dismissed with Esc/Enter/q.
    Message { tone: PopupTone, text: String },
    /// A question: Enter/y runs `action`, Esc/n/q cancels.
    Confirm {
        title: &'static str,
        text: String,
        accept_label: &'static str,
        action: PromptAction,
    },
}

impl Popup {
    pub fn error(text: impl Into<String>) -> Self {
        Self::Message {
            tone: PopupTone::Error,
            text: text.into(),
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::Message {
            tone: PopupTone::Info,
            text: text.into(),
        }
    }

    pub fn tone(&self) -> PopupTone {
        match self {
            Self::Message { tone, .. } => *tone,
            Self::Confirm { .. } => PopupTone::Prompt,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Message {
                tone: PopupTone::Error,
                ..
            } => "Error Details",
            Self::Message { .. } => "Message",
            Self::Confirm { title, .. } => title,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Message { text, .. } | Self::Confirm { text, .. } => text,
        }
    }

    /// Key hint shown on the popup's last line.
    pub fn hint(&self) -> String {
        match self {
            Self::Message { .. } => "Press Esc or Enter to dismiss".into(),
            Self::Confirm { accept_label, .. } => {
                format!("Enter/y: {accept_label}   Esc/n: Cancel")
            }
        }
    }
}

/// Rows moved by PageUp/PageDown in a popup.
const POPUP_PAGE: u16 = 10;

impl App {
    /// Show `popup`, replacing any open one, scrolled to the top.
    pub(super) fn open_popup(&mut self, popup: Popup) {
        self.popup = Some(popup);
        self.popup_scroll = 0;
    }

    pub(super) fn handle_popup_key(&mut self, key: KeyEvent) {
        if self.popup.is_none() {
            return;
        }
        // Scrolling works in every popup. The renderer clamps the offset, so
        // it's only bounded here to keep it from growing without limit.
        let max = u16::MAX / 2;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.popup_scroll = (self.popup_scroll + 1).min(max);
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.popup_scroll = self.popup_scroll.saturating_sub(1);
                return;
            }
            KeyCode::PageDown => {
                self.popup_scroll = (self.popup_scroll + POPUP_PAGE).min(max);
                return;
            }
            KeyCode::PageUp => {
                self.popup_scroll = self.popup_scroll.saturating_sub(POPUP_PAGE);
                return;
            }
            _ => {}
        }
        let Some(popup) = self.popup.take() else {
            return;
        };
        self.popup_scroll = 0;
        match popup {
            Popup::Message { .. } => {
                if !matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                    self.popup = Some(popup);
                }
            }
            Popup::Confirm { action, .. } => match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => self.accept_prompt(action),
                KeyCode::Esc | KeyCode::Char('n' | 'N' | 'q') => self.cancel_prompt(),
                _ => self.popup = Some(popup),
            },
        }
    }

    /// Show the full status message in a popup (`m` key).
    pub(super) fn show_status_detail(&mut self) {
        if let Some(msg) = self.status_message.clone() {
            self.open_popup(Popup::info(msg));
        }
    }

    fn accept_prompt(&mut self, action: PromptAction) {
        match action {
            PromptAction::SelectEmbeddingModel { resume } => {
                self.resume_cluster_after_model = Some(resume);
                self.open_embedding_models();
            }
            PromptAction::OpenRouterAuth { resume } => {
                self.resume_cluster_after_model = Some(resume);
                self.events.send(AppEvent::StartOpenRouterAuth);
            }
        }
    }

    /// Both prompt actions resume clustering, so cancelling either one
    /// abandons the pending cluster run.
    fn cancel_prompt(&mut self) {
        self.resume_cluster_after_model = None;
        // An empty cluster view is a dead end; go back to the timeline.
        if self.current_view() == Some(&ViewKind::Cluster) && self.cluster_result.is_none() {
            self.pop_view();
        }
        self.status_message =
            Some("Clustering cancelled. Use :embeddings to select a model.".into());
    }

    /// Ask the user to pick an embedding model (or sign in to OpenRouter
    /// first) after clustering found no embedding provider.
    pub(super) fn prompt_for_embedding_model(&mut self, source: ClusterSource) {
        let mlx_line = match &self.config.mlx_server_url {
            Some(url) => {
                format!("The MLX server at {url} is not reachable or has no embedding model.")
            }
            None => "No MLX server is configured (mlx_server_url).".to_string(),
        };
        let (or_line, question, accept_label, action) = if self.openrouter_client.is_some() {
            (
                "No OpenRouter embedding model is selected.",
                format!(
                    "Select an OpenRouter embedding model now? Clustering of {source} starts after you pick one."
                ),
                "Select model",
                PromptAction::SelectEmbeddingModel { resume: source },
            )
        } else {
            (
                "OpenRouter is not authenticated.",
                format!(
                    "Authenticate with OpenRouter now? You can then pick an embedding model, and clustering of {source} starts after you pick one."
                ),
                "Authenticate",
                PromptAction::OpenRouterAuth { resume: source },
            )
        };
        self.open_popup(Popup::Confirm {
            title: "Embedding Model Needed",
            text: format!(
                "Clustering needs an embedding model, and none is available.\n\n\
                 {mlx_line}\n{or_line}\n\n{question}"
            ),
            accept_label,
            action,
        });
    }

    /// Open the embedding model picker with a clean filter/search state.
    pub(super) fn open_embedding_models(&mut self) {
        self.model_filter = None;
        self.model_filter_open = false;
        self.model_search.clear();
        self.model_search_active = false;
        self.model_filter_search.clear();
        self.model_filter_search_active = false;
        self.events.send(AppEvent::FetchOpenRouterModels);
        self.events
            .send(AppEvent::PushView(ViewKind::OpenRouterModels));
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::auth::credentials::CredentialSet;
    use crate::config::AppConfig;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// App sitting in an empty Cluster view pushed from Home, as it is when
    /// `start_cluster` finds no embedding provider.
    fn app_in_empty_cluster_view() -> App {
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.push_view(ViewKind::Cluster);
        app
    }

    #[tokio::test]
    async fn missing_provider_without_openrouter_offers_auth() {
        let mut app = app_in_empty_cluster_view();
        app.prompt_for_embedding_model(ClusterSource::Home);
        let Some(Popup::Confirm { action, text, .. }) = &app.popup else {
            panic!("expected a confirm prompt, got {:?}", app.popup);
        };
        assert_eq!(
            *action,
            PromptAction::OpenRouterAuth {
                resume: ClusterSource::Home
            }
        );
        assert!(text.contains("not authenticated"), "{text}");
    }

    #[tokio::test]
    async fn accepting_select_prompt_opens_picker_and_remembers_source() {
        let mut app = app_in_empty_cluster_view();
        app.popup = Some(Popup::Confirm {
            title: "t",
            text: String::new(),
            accept_label: "Select model",
            action: PromptAction::SelectEmbeddingModel {
                resume: ClusterSource::Mentions,
            },
        });
        app.handle_popup_key(key(KeyCode::Char('y')));
        assert!(app.popup.is_none());
        assert_eq!(
            app.resume_cluster_after_model,
            Some(ClusterSource::Mentions)
        );
    }

    #[tokio::test]
    async fn cancelling_prompt_leaves_empty_cluster_view() {
        let mut app = app_in_empty_cluster_view();
        app.prompt_for_embedding_model(ClusterSource::Home);
        app.handle_popup_key(key(KeyCode::Esc));
        assert!(app.popup.is_none());
        assert_eq!(app.resume_cluster_after_model, None);
        assert_eq!(app.current_view(), Some(&ViewKind::Home));
    }

    #[tokio::test]
    async fn other_keys_keep_popup_open() {
        let mut app = app_in_empty_cluster_view();
        app.prompt_for_embedding_model(ClusterSource::Home);
        app.handle_popup_key(key(KeyCode::Char('j')));
        assert!(matches!(app.popup, Some(Popup::Confirm { .. })));

        app.popup = Some(Popup::info("long message"));
        app.handle_popup_key(key(KeyCode::Char('j')));
        assert!(app.popup.is_some());
        app.handle_popup_key(key(KeyCode::Enter));
        assert!(app.popup.is_none());
    }

    #[tokio::test]
    async fn scroll_keys_move_offset_and_closing_resets_it() {
        let mut app = app_in_empty_cluster_view();
        app.open_popup(Popup::info("long message"));
        app.handle_popup_key(key(KeyCode::Char('j')));
        app.handle_popup_key(key(KeyCode::Down));
        app.handle_popup_key(key(KeyCode::PageDown));
        assert_eq!(app.popup_scroll, 2 + POPUP_PAGE);
        app.handle_popup_key(key(KeyCode::Char('k')));
        app.handle_popup_key(key(KeyCode::PageUp));
        assert_eq!(app.popup_scroll, 1);
        app.handle_popup_key(key(KeyCode::Up));
        app.handle_popup_key(key(KeyCode::Up));
        assert_eq!(app.popup_scroll, 0);
        assert!(app.popup.is_some());

        app.popup_scroll = 5;
        app.handle_popup_key(key(KeyCode::Esc));
        assert!(app.popup.is_none());
        assert_eq!(app.popup_scroll, 0);

        // Opening a popup over a scrolled one starts at the top.
        app.open_popup(Popup::info("a"));
        app.popup_scroll = 3;
        app.open_popup(Popup::info("b"));
        assert_eq!(app.popup_scroll, 0);
    }

    #[tokio::test]
    async fn m_shows_full_status_message() {
        let mut app = app_in_empty_cluster_view();
        app.status_message = Some("a very long status message".into());
        app.show_status_detail();
        assert_eq!(app.popup, Some(Popup::info("a very long status message")));
    }
}
