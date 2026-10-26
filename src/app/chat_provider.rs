//! Saving the `:provider` choice as `chat_provider` in config.toml.
//!
//! `:provider <name>` remembers an offer. Once that provider answers (a
//! probe for the local servers, a chosen model for OpenRouter), a prompt
//! offers to save it, so it is used from startup. `:provider auto` offers
//! to remove a saved choice.

use super::{App, Popup, PromptAction};
use crate::config::{self, ChatProviderKind};

impl App {
    /// `:provider <kind>` was run: offer to save `kind` once it answers.
    /// Nothing to offer when it is already the saved choice.
    pub(super) fn expect_chat_provider(&mut self, kind: ChatProviderKind) {
        self.provider_save_offer = (self.config.chat_provider != Some(kind)).then_some(kind);
    }

    /// A probe or model choice for `kind` finished. If `:provider` asked
    /// for `kind` and it answers, offer to save it.
    pub(super) fn chat_provider_checked(&mut self, kind: ChatProviderKind, answers: bool) {
        if self.provider_save_offer != Some(kind) {
            return;
        }
        self.provider_save_offer = None;
        if answers {
            self.offer_to_save_chat_provider(kind);
        }
    }

    fn offer_to_save_chat_provider(&mut self, kind: ChatProviderKind) {
        let model = self
            .resolved_chat_model()
            .map(|m| format!(" with {m}"))
            .unwrap_or_default();
        let note = match kind {
            ChatProviderKind::OpenRouter => {
                "\n\nThe OpenRouter chat model is not saved: pick one with \
                 :openrouter-models in each session."
            }
            _ => "",
        };
        let text = format!(
            "{} answers{model}.\n\nSave chat_provider = \"{}\" to {} so it is \
             used from startup? It then never falls back to another provider.{note}",
            kind.label(),
            kind.key(),
            self.config_file_name(),
        );
        self.ask(Popup::Confirm {
            title: "Save Chat Provider",
            text,
            accept_label: "Save",
            action: PromptAction::SaveChatProvider(Some(kind)),
        });
    }

    /// `:provider auto` was run: offer to remove a saved choice.
    pub(super) fn offer_to_clear_chat_provider(&mut self) {
        self.provider_save_offer = None;
        let Some(saved) = self.config.chat_provider else {
            return;
        };
        let text = format!(
            "chat_provider = \"{}\" is saved in {}.\n\nRemove it so auto \
             (MLX, then llama.cpp, then OpenRouter) applies from startup?",
            saved.key(),
            self.config_file_name(),
        );
        self.ask(Popup::Confirm {
            title: "Remove Chat Provider",
            text,
            accept_label: "Remove",
            action: PromptAction::SaveChatProvider(None),
        });
    }

    /// Open `prompt` unless another popup is open: replacing that one would
    /// drop what it is waiting for.
    fn ask(&mut self, prompt: Popup) {
        if self.popup.is_none() {
            self.open_popup(prompt);
        }
    }

    fn config_file_name(&self) -> String {
        self.config_file
            .as_ref()
            .map_or_else(|| "config.toml".into(), |p| p.display().to_string())
    }

    /// Write `chat_provider` (or remove it, for `None`) to config.toml.
    pub(super) fn save_chat_provider(&mut self, provider: Option<ChatProviderKind>) {
        let Some(path) = self.config_file.clone() else {
            self.set_error("Cannot save chat_provider: no home directory for config.toml.".into());
            return;
        };
        match config::save_chat_provider(&path, provider) {
            Ok(()) => {
                self.config.chat_provider = provider;
                self.status_message = Some(match provider {
                    Some(kind) => format!(
                        "Saved chat_provider = \"{}\" to {}",
                        kind.key(),
                        path.display()
                    ),
                    None => format!("Removed chat_provider from {}", path.display()),
                });
            }
            Err(e) => self.set_error(format!("Cannot save chat_provider: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::*;
    use crate::app::LlamaCppProbe;
    use crate::auth::credentials::CredentialSet;
    use crate::config::AppConfig;
    use crate::event::AppEvent;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// An app whose config file is a fresh path in the temp directory.
    fn app_with_config_file(name: &str) -> (App, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("xplorertui-provider-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        let mut app = App::new(AppConfig::default(), None, CredentialSet::default());
        app.config_file = Some(path.clone());
        (app, path)
    }

    fn llama_probe(chat: Option<&str>) -> AppEvent {
        AppEvent::LlamaCppProbed(LlamaCppProbe {
            chat_model: chat.map(String::from),
            embed_model: None,
        })
    }

    #[tokio::test]
    async fn reachable_provider_is_offered_and_saved() {
        let (mut app, path) = app_with_config_file("save");
        app.llama_cpp.chat = Some(std::sync::Arc::new(
            crate::llamacpp::client::LlamaCppClient::new("http://127.0.0.1:9".into()),
        ));
        app.preferred_chat_provider = Some(ChatProviderKind::LlamaCpp);
        app.expect_chat_provider(ChatProviderKind::LlamaCpp);

        app.handle_app_event(llama_probe(Some("gemma-4-12b")));
        let Some(Popup::Confirm { action, text, .. }) = &app.popup else {
            panic!("expected a save prompt, got {:?}", app.popup);
        };
        assert_eq!(
            *action,
            PromptAction::SaveChatProvider(Some(ChatProviderKind::LlamaCpp))
        );
        assert!(
            text.contains("llama.cpp answers with gemma-4-12b"),
            "{text}"
        );

        app.handle_popup_key(key(KeyCode::Enter));
        assert_eq!(app.config.chat_provider, Some(ChatProviderKind::LlamaCpp));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "chat_provider = \"llama\"\n"
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn unreachable_or_unasked_provider_is_not_offered() {
        let (mut app, _) = app_with_config_file("unreachable");
        app.expect_chat_provider(ChatProviderKind::LlamaCpp);
        app.handle_app_event(llama_probe(None));
        assert!(app.popup.is_none());
        assert_eq!(app.provider_save_offer, None);

        // A startup probe, with no :provider before it, offers nothing.
        app.handle_app_event(llama_probe(Some("gemma-4-12b")));
        assert!(app.popup.is_none());
    }

    #[tokio::test]
    async fn saved_provider_is_not_offered_again() {
        let (mut app, _) = app_with_config_file("again");
        app.config.chat_provider = Some(ChatProviderKind::Mlx);
        app.expect_chat_provider(ChatProviderKind::Mlx);
        assert_eq!(app.provider_save_offer, None);
    }

    #[tokio::test]
    async fn declining_keeps_the_config_unchanged() {
        let (mut app, path) = app_with_config_file("decline");
        app.expect_chat_provider(ChatProviderKind::Mlx);
        app.chat_provider_checked(ChatProviderKind::Mlx, true);
        assert!(app.popup.is_some());
        app.handle_popup_key(key(KeyCode::Esc));
        assert!(app.popup.is_none());
        assert_eq!(app.config.chat_provider, None);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn auto_offers_to_remove_a_saved_provider() {
        let (mut app, path) = app_with_config_file("auto");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "# keep me\nchat_provider = \"mlx\"\n").unwrap();
        app.config.chat_provider = Some(ChatProviderKind::Mlx);

        app.offer_to_clear_chat_provider();
        let Some(Popup::Confirm { action, .. }) = &app.popup else {
            panic!("expected a remove prompt, got {:?}", app.popup);
        };
        assert_eq!(*action, PromptAction::SaveChatProvider(None));
        app.handle_popup_key(key(KeyCode::Char('y')));
        assert_eq!(app.config.chat_provider, None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# keep me\n");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn an_open_popup_is_not_replaced() {
        let (mut app, _) = app_with_config_file("busy");
        app.open_popup(Popup::info("something else"));
        app.expect_chat_provider(ChatProviderKind::Mlx);
        app.chat_provider_checked(ChatProviderKind::Mlx, true);
        assert_eq!(app.popup, Some(Popup::info("something else")));
    }
}
