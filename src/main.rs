pub mod api;
pub mod app;
pub mod auth;
pub mod command;
pub mod config;
pub mod event;
pub mod ui;

use app::App;
use auth::credentials::{CredentialSet, load_credentials};
use auth::{AuthMethod, AuthProvider};
use config::load_config;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Initialize tracing (logs to stderr if RUST_LOG is set).
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Handle `xplorertui auth` subcommand before launching TUI.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("auth") {
        return run_auth_command().await;
    }

    let config = load_config();

    // Load credentials and build API client.
    let (creds, api_client) = match load_credentials() {
        Ok(creds) => {
            let client = match AuthProvider::new(creds.clone()) {
                Ok(auth) => {
                    // Hint if OAuth2 PKCE is detected but no tokens exist.
                    if auth.method == AuthMethod::OAuth2Pkce && !auth::has_stored_tokens() {
                        eprintln!(
                            "Hint: Run `xplorertui auth` to authenticate with OAuth 2.0 PKCE, \
                             or use `:auth` inside the TUI."
                        );
                    }
                    tracing::info!(method = ?auth.method, "auth initialized");
                    Some(api::XApiClient::new(auth))
                }
                Err(e) => {
                    tracing::warn!("auth setup failed: {e}");
                    eprintln!("Warning: auth setup failed ({e}). Running without API access.");
                    None
                }
            };
            (creds, client)
        }
        Err(e) => {
            tracing::warn!("no credentials found: {e}");
            eprintln!("Warning: no credentials found ({e}). Running without API access.");
            (CredentialSet::default(), None)
        }
    };

    let terminal = ratatui::init();
    let result = App::new(config, api_client, creds).run(terminal).await;
    ratatui::restore();
    result
}

/// Standalone `xplorertui auth` command — runs the PKCE flow outside the TUI.
async fn run_auth_command() -> color_eyre::Result<()> {
    // Load .env files so X_CLIENT_ID is available, but don't require a full
    // credential set — the user may only have OAuth2 vars configured.
    auth::credentials::load_env_files();

    let get = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());

    let client_id = get("X_CLIENT_ID").ok_or_else(|| {
        color_eyre::eyre::eyre!(
            "X_CLIENT_ID is not set.\n\
             OAuth 2.0 PKCE requires X_CLIENT_ID (and optionally X_CLIENT_SECRET).\n\
             Add them to ~/.config/xplorertui/.env or your environment."
        )
    })?;

    let oauth2_creds = auth::credentials::OAuth2Credentials {
        client_id,
        client_secret: get("X_CLIENT_SECRET"),
    };

    // Check for existing tokens.
    if auth::has_stored_tokens() {
        eprint!("Tokens already exist. Re-authenticate? [y/N] ");
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    println!("Starting OAuth 2.0 PKCE authorization flow...");
    println!("Your browser should open for authorization.");
    println!();

    match auth::oauth2_pkce::start_pkce_flow(&oauth2_creds).await {
        Ok(_) => {
            println!("Authentication successful! Tokens saved to ~/.config/xplorertui/tokens.json");
            Ok(())
        }
        Err(e) => Err(color_eyre::eyre::eyre!("Authentication failed: {e}")),
    }
}
