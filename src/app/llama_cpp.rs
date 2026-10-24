//! llama.cpp (`llama-server`) backends: which servers are configured, what
//! their probes found, and the probe itself.

use std::future::Future;
use std::sync::Arc;

use super::App;
use crate::config::{LlamaCppConfig, TaskPrompts};
use crate::event::{AppEvent, Event};
use crate::llamacpp::client::LlamaCppClient;

/// The llama.cpp servers from `[llama_cpp]` and what their probes found.
#[derive(Default)]
pub struct LlamaCppState {
    pub chat: Option<Arc<LlamaCppClient>>,
    pub embed: Option<Arc<LlamaCppClient>>,
    /// Model name to send for chat; `Some` once the chat probe succeeded.
    pub chat_model: Option<String>,
    /// Model name to send for embeddings; `Some` once the embedding probe
    /// succeeded.
    pub embed_model: Option<String>,
    pub task_prompts: Arc<TaskPrompts>,
    configured_chat_model: Option<String>,
    configured_embed_model: Option<String>,
}

/// What a probe found: the model name for each server that answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LlamaCppProbe {
    pub chat_model: Option<String>,
    pub embed_model: Option<String>,
}

impl LlamaCppState {
    pub fn from_config(config: Option<&LlamaCppConfig>) -> Self {
        let Some(config) = config else {
            return Self::default();
        };
        let client = |url: &Option<String>| {
            url.as_ref()
                .map(|url| Arc::new(LlamaCppClient::new(url.clone())))
        };
        Self {
            chat: client(&config.chat_url),
            embed: client(&config.embedding_url),
            chat_model: None,
            embed_model: None,
            task_prompts: Arc::new(config.task_prompts.clone()),
            configured_chat_model: config.chat_model.clone(),
            configured_embed_model: config.embedding_model.clone(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.chat.is_some() || self.embed.is_some()
    }

    /// Probe both servers at once. The future owns what it needs, so it can
    /// run on a spawned task.
    pub fn probe(&self) -> impl Future<Output = LlamaCppProbe> + Send + 'static {
        let chat = self.chat.clone();
        let embed = self.embed.clone();
        let chat_name = self.configured_chat_model.clone();
        let embed_name = self.configured_embed_model.clone();
        async move {
            let chat = async {
                match &chat {
                    Some(client) => client.probe_chat(chat_name.as_deref()).await,
                    None => None,
                }
            };
            let embed = async {
                match &embed {
                    Some(client) => client.probe_embeddings(embed_name.as_deref()).await,
                    None => None,
                }
            };
            let (chat_model, embed_model) = tokio::join!(chat, embed);
            LlamaCppProbe {
                chat_model,
                embed_model,
            }
        }
    }

    pub fn apply(&mut self, probe: LlamaCppProbe) {
        self.chat_model = probe.chat_model;
        self.embed_model = probe.embed_model;
    }

    /// One line for the status bar about what the probe found.
    pub fn status(&self) -> String {
        let mut found = Vec::new();
        if let Some(model) = &self.chat_model {
            found.push(format!("chat ({model})"));
        }
        if let Some(model) = &self.embed_model {
            found.push(format!("embeddings ({model})"));
        }
        if found.is_empty() {
            "llama.cpp server not reachable.".to_string()
        } else {
            format!("llama.cpp server detected: {}", found.join(", "))
        }
    }
}

impl App {
    pub(super) fn dispatch_probe_llama_cpp(&self) {
        if !self.llama_cpp.is_configured() {
            return;
        }
        let probe = self.llama_cpp.probe();
        let sender = self.events.sender();
        tokio::spawn(async move {
            let found = probe.await;
            let _ = sender.send(Event::App(Box::new(AppEvent::LlamaCppProbed(found))));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_configured_without_the_section() {
        let state = LlamaCppState::from_config(None);
        assert!(!state.is_configured());
        assert!(state.chat.is_none() && state.embed.is_none());
    }

    #[test]
    fn status_names_the_models_that_answered() {
        let mut state = LlamaCppState::default();
        assert_eq!(state.status(), "llama.cpp server not reachable.");
        state.apply(LlamaCppProbe {
            chat_model: Some("gemma-4-12b".into()),
            embed_model: None,
        });
        assert_eq!(
            state.status(),
            "llama.cpp server detected: chat (gemma-4-12b)"
        );
    }
}
