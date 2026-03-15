use std::sync::Arc;
use tokio::sync::Mutex;

use ratatui::DefaultTerminal;

use super::App;
use crate::api::XApiClient;
use crate::auth::AuthProvider;
use crate::event::EventHandler;

impl App {
    // -- Auth flow (suspends TUI) ------------------------------------------

    pub(super) async fn run_auth_flow(&mut self, terminal: &mut DefaultTerminal) {
        let Some(ref oauth2_creds) = self.credentials.oauth2 else {
            self.status_message =
                Some("OAuth 2.0 not configured. Set X_CLIENT_ID in your .env file.".into());
            return;
        };
        let oauth2_creds = oauth2_creds.clone();

        // Suspend the TUI so the user can interact with their browser.
        ratatui::restore();

        let result = crate::auth::oauth2_pkce::start_pkce_flow(
            &oauth2_creds,
            self.config.oauth_callback_port,
        )
        .await;

        match &result {
            Ok(_) => {
                println!();
                println!("Authentication successful! Tokens saved.");
            }
            Err(e) => {
                println!();
                println!("Authentication failed: {e}");
            }
        }

        println!();
        println!("Press Enter to return to the TUI...");
        let _ = std::io::stdin().read_line(&mut String::new());

        // Re-initialize the terminal and event handler.
        *terminal = ratatui::init();
        self.events = EventHandler::new();

        // On success, rebuild the API client with the new tokens.
        if result.is_ok() {
            match AuthProvider::new(self.credentials.clone()) {
                Ok(auth) => {
                    self.api_client = Some(Arc::new(Mutex::new(XApiClient::new(
                        auth,
                        self.config.oauth_callback_port,
                    ))));
                    self.status_message = Some("Authenticated successfully!".into());
                }
                Err(e) => {
                    self.status_message = Some(format!("Auth provider error: {e}"));
                }
            }
        } else if let Err(e) = result {
            self.status_message = Some(format!("Auth failed: {e}"));
        }
    }

    // -- OpenRouter auth flow (suspends TUI) ---------------------------------

    pub(super) async fn run_openrouter_auth_flow(&mut self, terminal: &mut DefaultTerminal) {
        ratatui::restore();

        let port = if self.config.openrouter_callback_port == 8478 {
            eprintln!(
                "Using OpenRouter callback port 3000 (legacy 8478 value detected in config)."
            );
            3000
        } else {
            self.config.openrouter_callback_port
        };

        let result = crate::openrouter::auth::start_openrouter_auth(port).await;

        match &result {
            Ok(_) => {
                println!();
                println!("OpenRouter authentication successful! API key saved.");
            }
            Err(e) => {
                println!();
                println!("OpenRouter authentication failed: {e}");
            }
        }

        println!();
        println!("Press Enter to return to the TUI...");
        let _ = std::io::stdin().read_line(&mut String::new());

        // Re-initialize the terminal and event handler.
        *terminal = ratatui::init();
        self.events = EventHandler::new();

        // On success, create the OpenRouter client.
        if result.is_ok() {
            match crate::cli::build_openrouter_client() {
                Ok(client) => {
                    self.openrouter_client = Some(Arc::new(client));
                    self.status_message = Some("OpenRouter authenticated successfully!".into());
                }
                Err(e) => {
                    self.status_message = Some(format!("OpenRouter client error: {e}"));
                }
            }
        } else if let Err(e) = result {
            self.status_message = Some(format!("OpenRouter auth failed: {e}"));
        }
    }

    /// Try to initialize the OpenRouter client from stored credentials.
    pub fn init_openrouter_client(&mut self) {
        if self.openrouter_client.is_some() {
            return;
        }
        if let Ok(client) = crate::cli::build_openrouter_client() {
            self.openrouter_client = Some(Arc::new(client));
        }
    }
}
