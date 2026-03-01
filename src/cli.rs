use clap::{Parser, Subcommand};
use color_eyre::eyre::{self, eyre};

use crate::api::XApiClient;
use crate::api::types::{Includes, Tweet};
use crate::auth::credentials::load_credentials;
use crate::auth::{AuthMethod, AuthProvider};
use crate::config::load_config;
use crate::openrouter::client::OpenRouterClient;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "xplorertui", about = "TUI and CLI for the X platform")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Subcommand)]
pub enum CliCommand {
    /// Launch the interactive TUI (default)
    Tui,
    /// Run the OAuth 2.0 PKCE authentication flow
    Auth,
    /// Fetch your home timeline (JSONL)
    Home,
    /// Fetch your mentions (JSONL)
    Mentions,
    /// Fetch your bookmarks (JSONL)
    Bookmarks,
    /// Search recent tweets (JSONL)
    Search {
        /// Search query
        query: String,
    },
    /// Look up a user profile (JSONL)
    User {
        /// Username (without @)
        username: String,
    },
    /// Fetch a single tweet and its thread (JSONL)
    Open {
        /// Tweet ID or URL
        id_or_url: String,
    },
    /// Run the OpenRouter OAuth authorization flow
    #[command(name = "openrouter-auth")]
    OpenRouterAuth,
    /// List OpenRouter embedding models (JSONL)
    #[command(name = "openrouter-models")]
    OpenRouterModels,
}

// ---------------------------------------------------------------------------
// Denormalization helper
// ---------------------------------------------------------------------------

/// Build a self-contained JSON object for a tweet with its author and media
/// embedded. Returns a `serde_json::Value` ready for serialization.
fn denormalize_tweet(tweet: &Tweet, includes: &Option<Includes>) -> serde_json::Value {
    let author = includes
        .as_ref()
        .and_then(|inc| inc.users.as_ref())
        .and_then(|users| {
            tweet
                .author_id
                .as_ref()
                .and_then(|aid| users.iter().find(|u| &u.id == aid))
        });

    let media: Vec<&crate::api::types::Media> = includes
        .as_ref()
        .and_then(|inc| inc.media.as_ref())
        .map(|all_media| {
            tweet
                .attachments
                .as_ref()
                .and_then(|att| att.media_keys.as_ref())
                .map(|keys| {
                    keys.iter()
                        .filter_map(|k| all_media.iter().find(|m| &m.media_key == k))
                        .collect()
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();

    serde_json::json!({
        "tweet": tweet,
        "author": author,
        "media": media,
    })
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

/// Print a list of tweets as JSONL to stdout.
fn print_tweets(tweets: &[Tweet], includes: &Option<Includes>) -> eyre::Result<()> {
    for tweet in tweets {
        let line = serde_json::to_string(&denormalize_tweet(tweet, includes))?;
        println!("{line}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Client construction (shared with main.rs TUI path)
// ---------------------------------------------------------------------------

/// Build an `OpenRouterClient` from env var or stored API key.
pub fn build_openrouter_client() -> eyre::Result<OpenRouterClient> {
    crate::auth::credentials::load_env_files();
    let api_key = crate::openrouter::auth::load_api_key().map_err(|e| eyre!("{e}"))?;
    Ok(OpenRouterClient::new(api_key))
}

/// Build an authenticated `XApiClient` from env credentials + config.
/// Returns an error if no credentials are found or auth setup fails.
pub fn build_api_client() -> eyre::Result<(XApiClient, crate::auth::credentials::CredentialSet)> {
    let config = load_config();
    let creds = load_credentials()?;
    let auth = AuthProvider::new(creds.clone())?;

    if auth.method == AuthMethod::OAuth2Pkce && !crate::auth::has_stored_tokens() {
        eprintln!("Hint: Run `xplorertui auth` to authenticate with OAuth 2.0 PKCE.");
    }

    let client = XApiClient::new(auth, config.oauth_callback_port);
    Ok((client, creds))
}

// ---------------------------------------------------------------------------
// Command execution
// ---------------------------------------------------------------------------

/// Extract a tweet ID from either a raw ID or a tweet URL.
fn parse_tweet_id(id_or_url: &str) -> eyre::Result<String> {
    // If it looks like a URL, extract the status ID from the path.
    if id_or_url.starts_with("http://") || id_or_url.starts_with("https://") {
        let url = url::Url::parse(id_or_url).map_err(|e| eyre!("invalid URL: {e}"))?;
        // Expected path: /<user>/status/<id>
        let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
        if let Some(pos) = segments.iter().position(|&s| s == "status")
            && let Some(id) = segments.get(pos + 1)
        {
            return Ok(id.to_string());
        }
        Err(eyre!("could not extract tweet ID from URL: {id_or_url}"))
    } else {
        Ok(id_or_url.to_string())
    }
}

pub async fn run_command(cmd: CliCommand) -> eyre::Result<()> {
    let (mut client, _creds) = build_api_client()?;
    let config = load_config();
    let max = config.default_max_results;

    match cmd {
        CliCommand::Tui | CliCommand::Auth | CliCommand::OpenRouterAuth => {
            unreachable!("tui, auth, and openrouter-auth are handled in main")
        }

        CliCommand::Home => {
            let resp = client
                .get_home_timeline(max, None)
                .await
                .map_err(|e| eyre!("{e}"))?;
            if let Some(tweets) = &resp.data {
                print_tweets(tweets, &resp.includes)?;
            }
        }

        CliCommand::Mentions => {
            let resp = client
                .get_mentions(max, None)
                .await
                .map_err(|e| eyre!("{e}"))?;
            if let Some(tweets) = &resp.data {
                print_tweets(tweets, &resp.includes)?;
            }
        }

        CliCommand::Bookmarks => {
            let resp = client
                .get_bookmarks(max, None)
                .await
                .map_err(|e| eyre!("{e}"))?;
            if let Some(tweets) = &resp.data {
                print_tweets(tweets, &resp.includes)?;
            }
        }

        CliCommand::Search { query } => {
            let resp = client
                .search_tweets(&query, max, None)
                .await
                .map_err(|e| eyre!("{e}"))?;
            if let Some(tweets) = &resp.data {
                print_tweets(tweets, &resp.includes)?;
            }
        }

        CliCommand::User { username } => {
            let username = username.strip_prefix('@').unwrap_or(&username);
            let resp = client.get_user(username).await.map_err(|e| eyre!("{e}"))?;
            if let Some(user) = &resp.data {
                let line = serde_json::to_string(&serde_json::json!({ "user": user }))?;
                println!("{line}");
            } else {
                return Err(eyre!("user @{username} not found"));
            }
        }

        CliCommand::Open { id_or_url } => {
            let tweet_id = parse_tweet_id(&id_or_url)?;

            // Fetch the root tweet.
            let resp = client
                .get_tweet(&tweet_id)
                .await
                .map_err(|e| eyre!("{e}"))?;
            let root = resp
                .data
                .as_ref()
                .ok_or_else(|| eyre!("tweet {tweet_id} not found"))?;

            // Print the root tweet.
            let line = serde_json::to_string(&denormalize_tweet(root, &resp.includes))?;
            println!("{line}");

            // Fetch the conversation thread if there is a conversation_id.
            if let Some(conv_id) = &root.conversation_id {
                let thread = client
                    .get_conversation_thread(conv_id, max, None)
                    .await
                    .map_err(|e| eyre!("{e}"))?;
                if let Some(tweets) = &thread.data {
                    // Filter out the root tweet (already printed).
                    let replies: Vec<&Tweet> = tweets.iter().filter(|t| t.id != tweet_id).collect();
                    for tweet in replies {
                        let line =
                            serde_json::to_string(&denormalize_tweet(tweet, &thread.includes))?;
                        println!("{line}");
                    }
                }
            }
        }

        CliCommand::OpenRouterModels => {
            let or_client = build_openrouter_client()?;
            let resp: crate::openrouter::types::ModelsResponse = or_client
                .get("/embeddings/models")
                .await
                .map_err(|e| eyre!("{e}"))?;

            for model in &resp.data {
                let line = serde_json::to_string(&model)?;
                println!("{line}");
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tweet_id_plain() {
        assert_eq!(parse_tweet_id("1234567890").unwrap(), "1234567890");
    }

    #[test]
    fn parse_tweet_id_from_url() {
        let url = "https://x.com/user/status/1234567890";
        assert_eq!(parse_tweet_id(url).unwrap(), "1234567890");
    }

    #[test]
    fn parse_tweet_id_from_twitter_url() {
        let url = "https://twitter.com/user/status/9876543210?s=20";
        assert_eq!(parse_tweet_id(url).unwrap(), "9876543210");
    }

    #[test]
    fn parse_tweet_id_bad_url() {
        let url = "https://example.com/no-status-here";
        assert!(parse_tweet_id(url).is_err());
    }
}
