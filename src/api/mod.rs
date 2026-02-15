pub mod engagement;
pub mod tweets;
pub mod types;
pub mod users;

use chrono::{DateTime, Utc};
use reqwest::Response;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::auth::oauth2_pkce;
use crate::auth::{AuthError, AuthMethod, AuthProvider};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ApiClientError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("rate limited until {reset_at}")]
    RateLimited { reset_at: DateTime<Utc> },
    #[error("API error (status {status}): {detail}")]
    ApiError { status: u16, detail: String },
    #[error("auth error: {0}")]
    Auth(#[from] AuthError),
    #[error("deserialization error: {0}")]
    Deserialize(String),
}

// ---------------------------------------------------------------------------
// Rate limit tracking
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct RateLimitInfo {
    pub remaining: Option<u32>,
    pub reset_at: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
}

// ---------------------------------------------------------------------------
// Query parameter helpers
// ---------------------------------------------------------------------------

pub(crate) fn tweet_fields() -> &'static str {
    "created_at,public_metrics,author_id,conversation_id,in_reply_to_user_id,\
     referenced_tweets,attachments,entities,lang,note_tweet"
}

pub(crate) fn tweet_expansions() -> &'static str {
    "author_id,referenced_tweets.id,attachments.media_keys"
}

pub(crate) fn user_fields() -> &'static str {
    "name,username,verified,profile_image_url,public_metrics,created_at,\
     description,url,location,pinned_tweet_id"
}

pub(crate) fn media_fields() -> &'static str {
    "url,preview_image_url,type,width,height,alt_text"
}

// ---------------------------------------------------------------------------
// API client
// ---------------------------------------------------------------------------

const BASE_URL: &str = "https://api.x.com/2";

pub struct XApiClient {
    http_client: reqwest::Client,
    auth: AuthProvider,
    user_id: Option<String>,
    #[allow(dead_code)]
    rate_limit: RateLimitInfo,
}

impl XApiClient {
    pub fn new(auth: AuthProvider) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            auth,
            user_id: None,
            rate_limit: RateLimitInfo::default(),
        }
    }

    /// Return the authenticated user's ID, caching after first call.
    pub async fn get_my_user_id(&mut self) -> Result<String, ApiClientError> {
        if let Some(ref id) = self.user_id {
            return Ok(id.clone());
        }
        let id = self
            .auth
            .get_authenticated_user_id(&self.http_client)
            .await?;
        self.user_id = Some(id.clone());
        Ok(id)
    }

    /// Issue a GET request with bearer-token authorization.
    pub(crate) async fn bearer_get<T: DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, ApiClientError> {
        let auth_header = match self.auth.method {
            AuthMethod::OAuth2Pkce => {
                if let Some(tokens) = oauth2_pkce::load_tokens().map_err(AuthError::OAuth2)? {
                    format!("Bearer {}", tokens.access_token)
                } else {
                    return Err(ApiClientError::Auth(AuthError::NoAuthMethod));
                }
            }
            _ => self.auth.get_bearer_header()?,
        };

        let resp = self
            .http_client
            .get(url)
            .header("Authorization", &auth_header)
            .send()
            .await?;

        self.handle_response(resp).await
    }

    /// Issue a GET request with user-context authorization.
    ///
    /// OAuth2 PKCE  -> stored access token
    /// OAuth 1.0a   -> signed OAuth header
    /// Bearer-only  -> fall back to bearer token
    pub(crate) async fn oauth_get<T: DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, ApiClientError> {
        let auth_header = match self.auth.method {
            AuthMethod::OAuth2Pkce => {
                if let Some(tokens) = oauth2_pkce::load_tokens().map_err(AuthError::OAuth2)? {
                    format!("Bearer {}", tokens.access_token)
                } else {
                    return Err(ApiClientError::Auth(AuthError::NoAuthMethod));
                }
            }
            AuthMethod::OAuth1 => self.auth.get_oauth_header("GET", url, None)?,
            AuthMethod::BearerOnly => self.auth.get_bearer_header()?,
        };

        let resp = self
            .http_client
            .get(url)
            .header("Authorization", &auth_header)
            .send()
            .await?;

        self.handle_response(resp).await
    }

    /// Parse rate-limit headers, check status, and deserialize the body.
    async fn handle_response<T: DeserializeOwned>(
        &self,
        resp: Response,
    ) -> Result<T, ApiClientError> {
        // Parse rate-limit headers (best effort).
        let remaining = resp
            .headers()
            .get("x-rate-limit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok());

        let reset_at = resp
            .headers()
            .get("x-rate-limit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<i64>().ok())
            .and_then(|ts| DateTime::from_timestamp(ts, 0));

        let _limit = resp
            .headers()
            .get("x-rate-limit-limit")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok());

        let status = resp.status();

        if status.as_u16() == 429 {
            let reset = reset_at.unwrap_or_else(Utc::now);
            return Err(ApiClientError::RateLimited { reset_at: reset });
        }

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ApiClientError::ApiError {
                status: status.as_u16(),
                detail: body,
            });
        }

        // Store rate-limit info (interior mutability is not required since
        // the fields are purely informational; we skip the update here and
        // keep the struct simple).
        let _ = remaining;

        let body = resp.text().await?;
        serde_json::from_str::<T>(&body)
            .map_err(|e| ApiClientError::Deserialize(format!("{e}: {body}")))
    }

    /// Build a full API URL from a path (e.g. "/tweets/123").
    pub(crate) fn url(path: &str) -> String {
        format!("{BASE_URL}{path}")
    }
}
