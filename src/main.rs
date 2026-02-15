pub mod api;
pub mod app;
pub mod auth;
pub mod command;
pub mod config;
pub mod event;
pub mod ui;

use app::App;
use auth::AuthProvider;
use auth::credentials::load_credentials;
use config::load_config;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Initialize tracing (logs to stderr if RUST_LOG is set).
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let config = load_config();

    // Load credentials and build API client.
    let api_client = match load_credentials() {
        Ok(creds) => match AuthProvider::new(creds) {
            Ok(auth) => {
                tracing::info!(method = ?auth.method, "auth initialized");
                Some(api::XApiClient::new(auth))
            }
            Err(e) => {
                tracing::warn!("auth setup failed: {e}");
                eprintln!("Warning: auth setup failed ({e}). Running without API access.");
                None
            }
        },
        Err(e) => {
            tracing::warn!("no credentials found: {e}");
            eprintln!("Warning: no credentials found ({e}). Running without API access.");
            None
        }
    };

    let terminal = ratatui::init();
    let result = App::new(config, api_client).run(terminal).await;
    ratatui::restore();
    result
}
