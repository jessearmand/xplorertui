use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Generic API response wrapper
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
#[serde(bound(serialize = "T: serde::Serialize"))]
pub struct ApiResponse<T> {
    #[serde(default)]
    pub data: Option<T>,
    #[serde(default)]
    pub includes: Option<Includes>,
    #[serde(default)]
    pub meta: Option<Meta>,
    #[serde(default)]
    pub errors: Option<Vec<ApiError>>,
}

/// Response containing a single object (e.g. GET /tweets/:id).
pub type SingleResponse<T> = ApiResponse<T>;

/// Response containing a list of objects (e.g. search, timeline).
pub type ListResponse<T> = ApiResponse<Vec<T>>;

// ---------------------------------------------------------------------------
// Tweet
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tweet {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub author_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub in_reply_to_user_id: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default)]
    pub edit_history_tweet_ids: Option<Vec<String>>,
    #[serde(default)]
    pub public_metrics: Option<PublicMetrics>,
    #[serde(default)]
    pub entities: Option<Entities>,
    #[serde(default)]
    pub referenced_tweets: Option<Vec<ReferencedTweet>>,
    #[serde(default)]
    pub attachments: Option<Attachments>,
    #[serde(default)]
    pub note_tweet: Option<NoteTweet>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicMetrics {
    pub like_count: u64,
    pub retweet_count: u64,
    pub reply_count: u64,
    pub quote_count: u64,
    #[serde(default)]
    pub bookmark_count: Option<u64>,
    #[serde(default)]
    pub impression_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferencedTweet {
    #[serde(rename = "type")]
    pub type_: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachments {
    #[serde(default)]
    pub media_keys: Option<Vec<String>>,
    #[serde(default)]
    pub poll_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteTweet {
    pub text: String,
    #[serde(default)]
    pub entities: Option<Entities>,
}

// ---------------------------------------------------------------------------
// User
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub verified: Option<bool>,
    #[serde(default)]
    pub profile_image_url: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub pinned_tweet_id: Option<String>,
    #[serde(default)]
    pub public_metrics: Option<UserPublicMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPublicMetrics {
    pub followers_count: u64,
    pub following_count: u64,
    pub tweet_count: u64,
    pub listed_count: u64,
}

// ---------------------------------------------------------------------------
// Media
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub media_key: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub preview_image_url: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub alt_text: Option<String>,
}

// ---------------------------------------------------------------------------
// Entities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entities {
    #[serde(default)]
    pub urls: Option<Vec<UrlEntity>>,
    #[serde(default)]
    pub hashtags: Option<Vec<HashtagEntity>>,
    #[serde(default)]
    pub mentions: Option<Vec<MentionEntity>>,
    #[serde(default)]
    pub cashtags: Option<Vec<CashtagEntity>>,
    #[serde(default)]
    pub annotations: Option<Vec<Annotation>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlEntity {
    pub start: i32,
    pub end: i32,
    pub url: String,
    #[serde(default)]
    pub expanded_url: Option<String>,
    #[serde(default)]
    pub display_url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashtagEntity {
    pub start: i32,
    pub end: i32,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MentionEntity {
    pub start: i32,
    pub end: i32,
    pub username: String,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashtagEntity {
    pub start: i32,
    pub end: i32,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub start: i32,
    pub end: i32,
    pub probability: f64,
    #[serde(rename = "type")]
    pub type_: String,
    pub normalized_text: String,
}

// ---------------------------------------------------------------------------
// Response metadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Includes {
    #[serde(default)]
    pub users: Option<Vec<User>>,
    #[serde(default)]
    pub tweets: Option<Vec<Tweet>>,
    #[serde(default)]
    pub media: Option<Vec<Media>>,
}

impl Includes {
    /// Add the includes of one more page to `slot`, so the users, tweets
    /// and media of earlier pages stay available.
    pub fn merge_page(slot: &mut Option<Includes>, page: Option<Includes>) {
        let Some(page) = page else {
            return;
        };
        let Some(current) = slot else {
            *slot = Some(page);
            return;
        };
        fn extend<T>(into: &mut Option<Vec<T>>, more: Option<Vec<T>>) {
            if let Some(more) = more {
                into.get_or_insert_with(Vec::new).extend(more);
            }
        }
        extend(&mut current.users, page.users);
        extend(&mut current.tweets, page.tweets);
        extend(&mut current.media, page.media);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub result_count: Option<u32>,
    #[serde(default)]
    pub next_token: Option<String>,
    #[serde(default)]
    pub previous_token: Option<String>,
    #[serde(default)]
    pub newest_id: Option<String>,
    #[serde(default)]
    pub oldest_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "type", default)]
    pub type_: Option<String>,
    #[serde(default)]
    pub status: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(key: &str) -> Media {
        serde_json::from_value(serde_json::json!({"media_key": key, "type": "photo"})).unwrap()
    }

    #[test]
    fn merging_a_page_keeps_earlier_media() {
        let mut slot = None;
        let page = |key: &str| Includes {
            users: None,
            tweets: None,
            media: Some(vec![media(key)]),
        };
        Includes::merge_page(&mut slot, Some(page("a")));
        Includes::merge_page(&mut slot, Some(page("b")));
        Includes::merge_page(&mut slot, None);
        let keys: Vec<_> = slot
            .unwrap()
            .media
            .unwrap()
            .into_iter()
            .map(|m| m.media_key)
            .collect();
        assert_eq!(keys, vec!["a", "b"]);
    }
}
