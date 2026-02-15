//! Authentication module for X API v2.
//!
//! Supports OAuth 2.0 PKCE, OAuth 1.0a, and app-only bearer token.

pub mod credentials;
pub mod oauth1;
pub mod oauth2_pkce;

use thiserror::Error;

use credentials::CredentialSet;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("credential error: {0}")]
    Credential(#[from] credentials::CredentialError),
    #[error("oauth2 error: {0}")]
    OAuth2(#[from] oauth2_pkce::OAuth2Error),
    #[error("no suitable auth method available")]
    NoAuthMethod,
    #[error("oauth1 credentials required for this endpoint")]
    OAuth1Required,
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("failed to parse /2/users/me response: {0}")]
    UserParse(String),
}

/// Which authentication strategy to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// OAuth 2.0 Authorization Code with PKCE.
    OAuth2Pkce,
    /// OAuth 1.0a HMAC-SHA1 (user-context).
    OAuth1,
    /// App-only bearer token (read-only).
    BearerOnly,
}

/// Central auth provider that wraps the active strategy.
#[derive(Debug, Clone)]
pub struct AuthProvider {
    pub method: AuthMethod,
    pub credentials: CredentialSet,
}

/// Pick the best available auth method from a credential set.
///
/// Preference: OAuth2 PKCE > OAuth 1.0a > Bearer-only.
pub fn detect_auth_method(creds: &CredentialSet) -> Result<AuthMethod, AuthError> {
    if creds.oauth2.is_some() {
        Ok(AuthMethod::OAuth2Pkce)
    } else if creds.oauth1.is_some() {
        Ok(AuthMethod::OAuth1)
    } else if creds.bearer.is_some() {
        Ok(AuthMethod::BearerOnly)
    } else {
        Err(AuthError::NoAuthMethod)
    }
}

impl AuthProvider {
    /// Create a new provider by loading credentials and detecting the best method.
    pub fn new(credentials: CredentialSet) -> Result<Self, AuthError> {
        let method = detect_auth_method(&credentials)?;
        Ok(Self {
            method,
            credentials,
        })
    }

    /// Return a `Bearer <token>` header value for read-only endpoints.
    ///
    /// Uses the bearer token from OAuth1 credentials or standalone bearer creds.
    pub fn get_bearer_header(&self) -> Result<String, AuthError> {
        // Try OAuth1 credentials' bearer token first.
        if let Some(ref o1) = self.credentials.oauth1
            && let Some(ref bt) = o1.bearer_token
        {
            return Ok(format!("Bearer {bt}"));
        }
        // Fall back to standalone bearer credentials.
        if let Some(ref bc) = self.credentials.bearer {
            return Ok(format!("Bearer {}", bc.bearer_token));
        }
        Err(AuthError::NoAuthMethod)
    }

    /// Return an OAuth 1.0a `Authorization` header for user-context endpoints.
    pub fn get_oauth_header(
        &self,
        method: &str,
        url: &str,
        params: Option<&[(&str, &str)]>,
    ) -> Result<String, AuthError> {
        let creds = self
            .credentials
            .oauth1
            .as_ref()
            .ok_or(AuthError::OAuth1Required)?;
        Ok(oauth1::generate_oauth_header(method, url, creds, params))
    }

    /// Call `GET /2/users/me` and return the authenticated user's ID.
    pub async fn get_authenticated_user_id(
        &self,
        client: &reqwest::Client,
    ) -> Result<String, AuthError> {
        let url = "https://api.x.com/2/users/me";
        let auth_header = match self.method {
            AuthMethod::OAuth1 => self.get_oauth_header("GET", url, None)?,
            AuthMethod::BearerOnly => self.get_bearer_header()?,
            AuthMethod::OAuth2Pkce => {
                // Use stored OAuth2 token if available.
                if let Some(tokens) = oauth2_pkce::load_tokens()? {
                    format!("Bearer {}", tokens.access_token)
                } else {
                    return Err(AuthError::NoAuthMethod);
                }
            }
        };

        let resp = client
            .get(url)
            .header("Authorization", &auth_header)
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        body["data"]["id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| AuthError::UserParse(body.to_string()))
    }
}
