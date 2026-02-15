//! OAuth 2.0 Authorization Code flow with PKCE for X API v2.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

use crate::auth::credentials::OAuth2Credentials;

const AUTH_URL: &str = "https://twitter.com/i/oauth2/authorize";
const TOKEN_URL: &str = "https://api.x.com/2/oauth2/token";

const DEFAULT_SCOPES: &[&str] = &["tweet.read", "users.read", "offline.access"];

#[derive(Debug, Error)]
pub enum OAuth2Error {
    #[error("oauth2 request error: {0}")]
    Request(String),
    #[error("CSRF state mismatch")]
    CsrfMismatch,
    #[error("callback missing authorization code")]
    MissingCode,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no refresh token available")]
    NoRefreshToken,
}

/// Persisted token data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

fn tokens_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/xplorertui/tokens.json")
}

pub fn save_tokens(data: &TokenData) -> Result<(), OAuth2Error> {
    let path = tokens_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)?;
    std::fs::write(&path, json)?;
    Ok(())
}

pub fn load_tokens() -> Result<Option<TokenData>, OAuth2Error> {
    let path = tokens_path();
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path)?;
    let data: TokenData = serde_json::from_str(&json)?;
    Ok(Some(data))
}

fn token_response_to_data<T: TokenResponse>(
    token_result: &T,
    existing_refresh: Option<&str>,
) -> TokenData {
    let expires_at = token_result
        .expires_in()
        .map(|d| Utc::now() + chrono::Duration::seconds(d.as_secs() as i64));

    let refresh_token = token_result
        .refresh_token()
        .map(|t| t.secret().clone())
        .or_else(|| existing_refresh.map(|s| s.to_string()));

    TokenData {
        access_token: token_result.access_token().secret().clone(),
        refresh_token,
        expires_at,
    }
}

/// Run the full OAuth 2.0 PKCE authorization flow.
///
/// 1. Bind a local TCP listener on a random port.
/// 2. Open the user's browser to the X authorization page.
/// 3. Wait for the redirect callback.
/// 4. Exchange the authorization code for tokens.
/// 5. Persist tokens to disk.
pub async fn start_pkce_flow(creds: &OAuth2Credentials) -> Result<TokenData, OAuth2Error> {
    // Bind listener on random port.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    let redirect_url = format!("http://127.0.0.1:{}", local_addr.port());

    let mut client = BasicClient::new(ClientId::new(creds.client_id.clone()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.to_string()).expect("valid auth URL"))
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_string()).expect("valid token URL"))
        .set_redirect_uri(RedirectUrl::new(redirect_url).expect("valid redirect URL"));

    if let Some(ref secret) = creds.client_secret {
        client = client.set_client_secret(ClientSecret::new(secret.clone()));
    }

    // Generate PKCE challenge.
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    // Build auth URL.
    let mut auth_request = client.authorize_url(CsrfToken::new_random);

    for scope in DEFAULT_SCOPES {
        auth_request = auth_request.add_scope(Scope::new(scope.to_string()));
    }

    let (auth_url, csrf_state) = auth_request.set_pkce_challenge(pkce_challenge).url();

    tracing::info!("opening browser for authorization");
    let auth_url_str = auth_url.to_string();
    if let Err(e) = open::that(&auth_url_str) {
        tracing::warn!("failed to open browser: {e}");
        eprintln!("Open this URL in your browser:\n{auth_url_str}");
    }

    // Wait for callback.
    let (mut stream, _addr) = listener.accept().await?;

    let mut buf = vec![0u8; 4096];
    let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
    let request_str = String::from_utf8_lossy(&buf[..n]);

    // Parse the GET request line for code and state.
    let request_line = request_str.lines().next().unwrap_or("");
    let path = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = path.split('?').nth(1).unwrap_or("");

    let mut code: Option<String> = None;
    let mut state: Option<String> = None;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        match (kv.next(), kv.next()) {
            (Some("code"), Some(v)) => code = Some(v.to_string()),
            (Some("state"), Some(v)) => state = Some(v.to_string()),
            _ => {}
        }
    }

    // Send response to browser.
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
        <html><body><h2>Authorization successful!</h2>\
        <p>You can close this tab.</p></body></html>";
    stream.write_all(response.as_bytes()).await?;

    // Validate state.
    let state = state.ok_or(OAuth2Error::CsrfMismatch)?;
    if state != *csrf_state.secret() {
        return Err(OAuth2Error::CsrfMismatch);
    }

    let code = code.ok_or(OAuth2Error::MissingCode)?;

    // Exchange code for tokens.
    let http_client = reqwest::Client::new();
    let token_result = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(&http_client)
        .await
        .map_err(|e| OAuth2Error::Request(e.to_string()))?;

    let data = token_response_to_data(&token_result, None);
    save_tokens(&data)?;
    Ok(data)
}

/// Refresh an expired access token using a stored refresh token.
pub async fn refresh_token(
    creds: &OAuth2Credentials,
    refresh: &str,
) -> Result<TokenData, OAuth2Error> {
    let mut client = BasicClient::new(ClientId::new(creds.client_id.clone()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.to_string()).expect("valid auth URL"))
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_string()).expect("valid token URL"))
        .set_redirect_uri(
            RedirectUrl::new("http://127.0.0.1".to_string()).expect("valid redirect URL"),
        );

    if let Some(ref secret) = creds.client_secret {
        client = client.set_client_secret(ClientSecret::new(secret.clone()));
    }

    let http_client = reqwest::Client::new();
    let token_result = client
        .exchange_refresh_token(&RefreshToken::new(refresh.to_string()))
        .request_async(&http_client)
        .await
        .map_err(|e| OAuth2Error::Request(e.to_string()))?;

    let data = token_response_to_data(&token_result, Some(refresh));
    save_tokens(&data)?;
    Ok(data)
}
