//! OAuth 1.0a HMAC-SHA1 request signing.
//!
//! Ported from x-cli's auth.py.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha1::Sha1;
use url::Url;

use crate::auth::credentials::OAuth1Credentials;

/// The percent-encoding set that matches Python's `quote(s, safe="")`.
/// RFC 3986 unreserved characters (ALPHA / DIGIT / "-" / "." / "_" / "~")
/// must NOT be encoded, everything else must be.
const ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn percent_encode(s: &str) -> String {
    utf8_percent_encode(s, ENCODE_SET).to_string()
}

fn generate_nonce() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::rng().fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn generate_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
        .to_string()
}

/// Generate an OAuth 1.0a `Authorization` header value.
///
/// `method` – HTTP method (GET, POST, …).
/// `url` – full request URL (query params are extracted automatically).
/// `creds` – OAuth 1.0a credentials.
/// `params` – additional body/query params to include in the signature base.
pub fn generate_oauth_header(
    method: &str,
    url: &str,
    creds: &OAuth1Credentials,
    params: Option<&[(&str, &str)]>,
) -> String {
    // -- 1. Core oauth params (without signature) --
    let nonce = generate_nonce();
    let timestamp = generate_timestamp();

    let mut oauth_params: Vec<(String, String)> = vec![
        ("oauth_consumer_key".into(), creds.api_key.clone()),
        ("oauth_nonce".into(), nonce),
        ("oauth_signature_method".into(), "HMAC-SHA1".into()),
        ("oauth_timestamp".into(), timestamp),
        ("oauth_token".into(), creds.access_token.clone()),
        ("oauth_version".into(), "1.0".into()),
    ];

    // -- 2. Collect all params for signature base string --
    let mut all_params: Vec<(String, String)> = oauth_params.clone();

    if let Some(extra) = params {
        for (k, v) in extra {
            all_params.push(((*k).to_string(), (*v).to_string()));
        }
    }

    // Extract query string params from the URL.
    if let Ok(parsed) = Url::parse(url) {
        for (k, v) in parsed.query_pairs() {
            all_params.push((k.into_owned(), v.into_owned()));
        }
    }

    // -- 3. Sort and build parameter string --
    all_params.sort();
    let param_string: String = all_params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // -- 4. Base URL (strip query string) --
    let parsed = Url::parse(url).expect("invalid URL passed to generate_oauth_header");
    let base_url = format!(
        "{}://{}{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or(""),
        parsed.path()
    );

    // -- 5. Signature base string --
    let base_string = format!(
        "{}&{}&{}",
        method.to_uppercase(),
        percent_encode(&base_url),
        percent_encode(&param_string),
    );

    // -- 6. Signing key --
    let signing_key = format!(
        "{}&{}",
        percent_encode(&creds.api_secret),
        percent_encode(&creds.access_token_secret),
    );

    // -- 7. HMAC-SHA1 --
    let mut mac =
        Hmac::<Sha1>::new_from_slice(signing_key.as_bytes()).expect("HMAC accepts any key size");
    mac.update(base_string.as_bytes());
    let signature = BASE64.encode(mac.finalize().into_bytes());

    // -- 8. Append signature to oauth params --
    oauth_params.push(("oauth_signature".into(), signature));
    oauth_params.sort();

    // -- 9. Build header value --
    let header_parts: String = oauth_params
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join(", ");

    format!("OAuth {header_parts}")
}
