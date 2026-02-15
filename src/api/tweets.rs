use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::api::types::{ListResponse, SingleResponse, Tweet};
use crate::api::{
    ApiClientError, XApiClient, media_fields, tweet_expansions, tweet_fields, user_fields,
};

/// Percent-encoding set for URL query values (encode everything except unreserved chars).
const QUERY_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn encode_query(s: &str) -> String {
    utf8_percent_encode(s, QUERY_ENCODE_SET).to_string()
}

impl XApiClient {
    /// Fetch a single tweet by ID.
    pub async fn get_tweet(&self, tweet_id: &str) -> Result<SingleResponse<Tweet>, ApiClientError> {
        let url = Self::url(&format!(
            "/tweets/{tweet_id}?tweet.fields={}&expansions={}&user.fields={}&media.fields={}",
            tweet_fields(),
            tweet_expansions(),
            user_fields(),
            media_fields(),
        ));
        self.bearer_get(&url).await
    }

    /// Search recent tweets matching a query.
    pub async fn search_tweets(
        &self,
        query: &str,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<Tweet>, ApiClientError> {
        let max_results = max_results.clamp(10, 100);
        let encoded_query = encode_query(query);

        let mut url = format!(
            "{}/tweets/search/recent?query={}&max_results={}&tweet.fields={}&expansions={}&user.fields={}&media.fields={}",
            Self::url(""),
            encoded_query,
            max_results,
            tweet_fields(),
            tweet_expansions(),
            user_fields(),
            media_fields(),
        );

        if let Some(token) = pagination_token {
            url.push_str(&format!("&pagination_token={token}"));
        }

        self.bearer_get(&url).await
    }

    /// Get tweets in a conversation thread.
    pub async fn get_conversation_thread(
        &self,
        conversation_id: &str,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<Tweet>, ApiClientError> {
        let query = format!("conversation_id:{conversation_id}");

        let max_results = max_results.clamp(10, 100);
        let encoded_query = encode_query(&query);

        let mut url = format!(
            "{}/tweets/search/recent?query={}&max_results={}&sort_order=recency&tweet.fields={}&expansions={}&user.fields={}&media.fields={}",
            Self::url(""),
            encoded_query,
            max_results,
            tweet_fields(),
            tweet_expansions(),
            user_fields(),
            media_fields(),
        );

        if let Some(token) = pagination_token {
            url.push_str(&format!("&pagination_token={token}"));
        }

        self.bearer_get(&url).await
    }
}
