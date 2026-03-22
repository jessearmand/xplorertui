use crate::api::types::{Includes, Media, Tweet};

/// Extract media URLs for a set of tweets, preferring `preview_image_url`
/// (smaller, faster to download) over the full `url`.
///
/// Returns `(tweet_index, media_url)` pairs.
pub fn extract_media_urls(tweets: &[Tweet], includes: Option<&Includes>) -> Vec<(usize, String)> {
    let all_media: Vec<&Media> = includes
        .and_then(|inc| inc.media.as_ref())
        .map(|m| m.iter().collect())
        .unwrap_or_default();

    let mut results = Vec::new();

    for (idx, tweet) in tweets.iter().enumerate() {
        let Some(ref att) = tweet.attachments else {
            continue;
        };
        let Some(ref keys) = att.media_keys else {
            continue;
        };

        for key in keys {
            if let Some(media) = all_media.iter().find(|m| m.media_key == *key) {
                // Prefer preview_image_url (smaller) for embedding purposes.
                let url = media.preview_image_url.as_deref().or(media.url.as_deref());
                if let Some(url) = url {
                    results.push((idx, url.to_string()));
                    break; // One image per tweet is enough for embedding.
                }
            }
        }
    }

    results
}

/// Download an image from a URL, returning the raw bytes.
pub async fn download_image(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, reqwest::Error> {
    let resp = http.get(url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    Ok(bytes.to_vec())
}

/// Download an image and return it as a base64-encoded data URI.
pub async fn download_image_as_base64(
    http: &reqwest::Client,
    url: &str,
) -> Result<String, reqwest::Error> {
    use base64::Engine;

    let bytes = download_image(http, url).await?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);

    // Infer MIME type from URL extension, default to jpeg.
    let mime = if url.contains(".png") {
        "image/png"
    } else if url.contains(".gif") {
        "image/gif"
    } else if url.contains(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    };

    Ok(format!("data:{mime};base64,{encoded}"))
}
