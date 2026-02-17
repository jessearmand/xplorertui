pub mod api;
pub mod app;
pub mod auth;
pub mod cli;
pub mod command;
pub mod config;
pub mod event;
pub mod ui;

use app::App;
use auth::credentials::CredentialSet;
use clap::Parser;
use cli::{Cli, CliCommand};
use config::load_config;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Initialize tracing (logs to stderr if RUST_LOG is set).
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        // No subcommand or explicit `tui` → launch the interactive TUI.
        None | Some(CliCommand::Tui) => run_tui().await,
        // `auth` → standalone PKCE flow.
        Some(CliCommand::Auth) => run_auth_command().await,
        // All other subcommands → non-interactive JSONL output.
        Some(cmd) => cli::run_command(cmd).await,
    }
}

/// Launch the interactive TUI.
async fn run_tui() -> color_eyre::Result<()> {
    let config = load_config();

    // Load credentials, tolerating missing creds (TUI can still show help etc.).
    let (creds, api_client) = match cli::build_api_client() {
        Ok((client, creds)) => {
            tracing::info!(method = ?client.auth_method(), "auth initialized");
            (creds, Some(client))
        }
        Err(e) => {
            tracing::warn!("no credentials / auth setup failed: {e}");
            eprintln!("Warning: {e}. Running without API access.");
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
    let config = load_config();

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

    match auth::oauth2_pkce::start_pkce_flow(&oauth2_creds, config.oauth_callback_port).await {
        Ok(_) => {
            println!("Authentication successful! Tokens saved to ~/.config/xplorertui/tokens.json");
            Ok(())
        }
        Err(e) => Err(color_eyre::eyre::eyre!("Authentication failed: {e}")),
    }
}
