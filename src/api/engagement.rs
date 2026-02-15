use crate::api::types::{ListResponse, Tweet};
use crate::api::{
    ApiClientError, XApiClient, media_fields, tweet_expansions, tweet_fields, user_fields,
};

impl XApiClient {
    /// Get the authenticated user's bookmarks.
    pub async fn get_bookmarks(
        &mut self,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<Tweet>, ApiClientError> {
        let my_id = self.get_my_user_id().await?;
        let max_results = max_results.clamp(10, 100);

        let mut url = Self::url(&format!(
            "/users/{my_id}/bookmarks?max_results={max_results}\
             &tweet.fields={}&expansions={}&user.fields={}&media.fields={}",
            tweet_fields(),
            tweet_expansions(),
            user_fields(),
            media_fields(),
        ));

        if let Some(token) = pagination_token {
            url.push_str(&format!("&pagination_token={token}"));
        }

        self.oauth_get(&url).await
    }

    /// Get tweets liked by a user.
    pub async fn get_liked_posts(
        &self,
        user_id: &str,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<Tweet>, ApiClientError> {
        let max_results = max_results.clamp(10, 100);

        let mut url = Self::url(&format!(
            "/users/{user_id}/liked_tweets?max_results={max_results}\
             &tweet.fields={}&expansions={}&user.fields={}&media.fields={}",
            tweet_fields(),
            tweet_expansions(),
            user_fields(),
            media_fields(),
        ));

        if let Some(token) = pagination_token {
            url.push_str(&format!("&pagination_token={token}"));
        }

        self.bearer_get(&url).await
    }
}
