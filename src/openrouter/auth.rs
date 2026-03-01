//! OpenRouter OAuth PKCE authorization flow.
//!
//! Unlike X's OAuth2 PKCE (which uses the `oauth2` crate), this is a direct
//! HTTP flow that exchanges a PKCE code for a persistent API key.

use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use url::form_urlencoded;

use super::OpenRouterError;
use super::types::{AuthKeysRequest, AuthKeysResponse};

// ---------------------------------------------------------------------------
// Key storage
// ---------------------------------------------------------------------------

/// Persisted OpenRouter API key data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenRouterKeyData {
    pub key: String,
    #[serde(default)]
    pub user_id: Option<String>,
}

fn key_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/xplorertui/openrouter_tokens.json")
}

pub fn save_key_data(data: &OpenRouterKeyData) -> Result<(), OpenRouterError> {
    let path = key_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)?;
    std::fs::write(&path, json)?;
    Ok(())
}

pub fn load_key_data() -> Result<Option<OpenRouterKeyData>, OpenRouterError> {
    let path = key_path();
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path)?;
    let data: OpenRouterKeyData = serde_json::from_str(&json)?;
    Ok(Some(data))
}

/// Load the OpenRouter API key.
///
/// Priority: `OPENROUTER_API_KEY` env var > stored file at
/// `~/.config/xplorertui/openrouter_tokens.json`.
pub fn load_api_key() -> Result<String, OpenRouterError> {
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY")
        && !key.is_empty()
    {
        return Ok(key);
    }

    if let Some(data) = load_key_data()? {
        return Ok(data.key);
    }

    Err(OpenRouterError::NoApiKey)
}

/// Check whether a stored OpenRouter API key exists on disk.
pub fn has_stored_key() -> bool {
    load_key_data().ok().flatten().is_some()
}

// ---------------------------------------------------------------------------
// PKCE helpers
// ---------------------------------------------------------------------------

/// Generate a random PKCE code verifier (43 URL-safe base64 characters).
fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Compute the PKCE code challenge: `base64url(SHA-256(verifier))`.
fn compute_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

fn build_auth_url(callback_url: &str, code_challenge: &str) -> String {
    format!(
        "https://openrouter.ai/auth\
         ?callback_url={}\
         &code_challenge={}\
         &code_challenge_method=S256",
        utf8_percent_encode(callback_url, NON_ALPHANUMERIC),
        code_challenge,
    )
}

fn build_start_page(auth_url: &str) -> String {
    // Use a local bootstrap page so browser navigation to OpenRouter carries
    // an explicit localhost referrer origin instead of a direct/no-referrer open.
    format!(
        "<!doctype html><html><head>\
         <meta charset=\"utf-8\"/>\
         <meta name=\"referrer\" content=\"origin\"/>\
         <title>OpenRouter Login</title>\
         </head><body>\
         <p>Redirecting to OpenRouter authorization...</p>\
         <script>window.location.replace({auth_url:?});</script>\
         </body></html>"
    )
}

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<(), OpenRouterError> {
    let response = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Auth flow
// ---------------------------------------------------------------------------

/// Run the OpenRouter OAuth PKCE flow.
///
/// 1. Generate PKCE code verifier + challenge.
/// 2. Open browser to OpenRouter auth page.
/// 3. Wait for callback with authorization code.
/// 4. Exchange code for API key via `POST /api/v1/auth/keys`.
/// 5. Save key to disk.
pub async fn start_openrouter_auth(port: u16) -> Result<OpenRouterKeyData, OpenRouterError> {
    let listener = TcpListener::bind(format!("localhost:{port}"))
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                OpenRouterError::Auth(format!(
                    "port {port} is already in use — \
                     check for conflicts or set openrouter_callback_port in config.toml"
                ))
            } else {
                OpenRouterError::Io(e)
            }
        })?;

    println!("Starting OpenRouter authorization flow...");
    println!("Your browser should open for authorization.");
    println!();

    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);

    // Keep callback URL exactly as documented for localhost apps.
    let callback_url = format!("http://localhost:{port}");
    let auth_url = build_auth_url(&callback_url, &code_challenge);
    let launch_url = format!("http://localhost:{port}/start");

    tracing::info!("opening browser for OpenRouter authorization");
    if let Err(e) = open::that(&launch_url) {
        tracing::warn!("failed to open browser: {e}");
        eprintln!("Open this URL in your browser:\n{launch_url}");
        eprintln!("\nIf localhost bootstrap fails, open this URL directly:\n{auth_url}");
    }

    // Wait for the redirect with the authorization code.
    // OpenRouter redirects to `http://localhost:{port}?code=...`.
    let code = loop {
        let (mut stream, _addr) = listener.accept().await?;

        let mut buf = vec![0u8; 4096];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
        let request_str = String::from_utf8_lossy(&buf[..n]);

        let request_line = request_str.lines().next().unwrap_or("");
        let path = request_line.split_whitespace().nth(1).unwrap_or("");
        let (request_path, query) = path.split_once('?').unwrap_or((path, ""));

        if request_path == "/start" {
            let body = build_start_page(&auth_url);
            write_response(&mut stream, "200 OK", "text/html; charset=utf-8", &body).await?;
            continue;
        }

        let params: std::collections::HashMap<String, String> =
            form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect();

        if let Some(error) = params.get("error") {
            let error_description = params
                .get("error_description")
                .cloned()
                .unwrap_or_else(|| "unknown authorization error".to_string());
            let message = format!("OpenRouter authorization failed: {error} ({error_description})");
            let body = "<html><body><h2>OpenRouter authorization failed.</h2>\
                        <p>You can close this tab and return to the terminal.</p></body></html>";
            write_response(
                &mut stream,
                "400 Bad Request",
                "text/html; charset=utf-8",
                body,
            )
            .await?;
            return Err(OpenRouterError::Auth(message));
        }

        let code = params.get("code").cloned();

        // Ignore requests that don't carry a code (e.g. /favicon.ico).
        if code.is_none() {
            write_response(
                &mut stream,
                "404 Not Found",
                "text/plain; charset=utf-8",
                "",
            )
            .await?;
            continue;
        }

        let response = "<html><body><h2>OpenRouter authorization successful!</h2>\
                        <p>You can close this tab.</p></body></html>";
        write_response(&mut stream, "200 OK", "text/html; charset=utf-8", response).await?;

        break code;
    };

    let code =
        code.ok_or_else(|| OpenRouterError::Auth("callback missing authorization code".into()))?;

    // Exchange the code for an API key.
    let http = reqwest::Client::new();
    let body = AuthKeysRequest {
        code,
        code_verifier,
        code_challenge_method: "S256".to_string(),
    };

    let resp = http
        .post("https://openrouter.ai/api/v1/auth/keys")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let detail = resp.text().await.unwrap_or_default();
        if status == 409 {
            let hint = "OpenRouter returned 409 while processing auth. \
                        This usually indicates app metadata/referrer/callback validation \
                        failed upstream. Try again, and if it persists, retry with \
                        callback URL http://localhost:3000 and a stable referrer URL.";
            return Err(OpenRouterError::ApiError {
                status,
                detail: format!("{detail}\n{hint}"),
            });
        }
        return Err(OpenRouterError::ApiError { status, detail });
    }

    let keys_resp: AuthKeysResponse = resp.json().await?;
    let key_data = OpenRouterKeyData {
        key: keys_resp.key,
        user_id: keys_resp.user_id,
    };

    save_key_data(&key_data)?;
    Ok(key_data)
}
