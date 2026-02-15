use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CredentialError {
    #[error("failed to load .env file: {0}")]
    EnvFile(#[from] dotenvy::Error),
    #[error("no credentials found; set X_API_KEY/X_CLIENT_ID/X_BEARER_TOKEN in env or .env")]
    NoCredentials,
}

/// OAuth 1.0a credentials (user-context with full signing).
#[derive(Debug, Clone)]
pub struct OAuth1Credentials {
    pub api_key: String,
    pub api_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
    pub bearer_token: Option<String>,
}

/// OAuth 2.0 PKCE credentials (confidential or public client).
#[derive(Debug, Clone)]
pub struct OAuth2Credentials {
    pub client_id: String,
    pub client_secret: Option<String>,
}

/// App-only bearer token.
#[derive(Debug, Clone)]
pub struct BearerCredentials {
    pub bearer_token: String,
}

/// All detected credentials bundled together.
#[derive(Debug, Clone, Default)]
pub struct CredentialSet {
    pub oauth1: Option<OAuth1Credentials>,
    pub oauth2: Option<OAuth2Credentials>,
    pub bearer: Option<BearerCredentials>,
}

/// Return candidate .env paths in priority order.
fn env_file_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".config/xplorertui/.env"));
        paths.push(home.join(".config/x-cli/.env"));
    }
    paths.push(PathBuf::from(".env"));
    paths
}

/// Load credentials from environment variables, trying .env files first.
///
/// Priority: ~/.config/xplorertui/.env > ~/.config/x-cli/.env > cwd .env
/// Variables already set in the environment take precedence.
pub fn load_credentials() -> Result<CredentialSet, CredentialError> {
    // Load .env files (earlier files have higher priority because dotenvy
    // does NOT overwrite existing env vars).
    for path in env_file_paths() {
        if path.exists() {
            let _ = dotenvy::from_path(&path);
        }
    }

    let get = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());

    let oauth1 = match (
        get("X_API_KEY"),
        get("X_API_SECRET"),
        get("X_ACCESS_TOKEN"),
        get("X_ACCESS_TOKEN_SECRET"),
    ) {
        (Some(api_key), Some(api_secret), Some(access_token), Some(access_token_secret)) => {
            Some(OAuth1Credentials {
                api_key,
                api_secret,
                access_token,
                access_token_secret,
                bearer_token: get("X_BEARER_TOKEN"),
            })
        }
        _ => None,
    };

    let oauth2 = get("X_CLIENT_ID").map(|client_id| OAuth2Credentials {
        client_id,
        client_secret: get("X_CLIENT_SECRET"),
    });

    let bearer = get("X_BEARER_TOKEN").map(|bearer_token| BearerCredentials { bearer_token });

    if oauth1.is_none() && oauth2.is_none() && bearer.is_none() {
        return Err(CredentialError::NoCredentials);
    }

    Ok(CredentialSet {
        oauth1,
        oauth2,
        bearer,
    })
}
