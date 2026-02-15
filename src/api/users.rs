use crate::api::types::{ListResponse, SingleResponse, Tweet, User};
use crate::api::{
    ApiClientError, XApiClient, media_fields, tweet_expansions, tweet_fields, user_fields,
};

impl XApiClient {
    /// Look up a user by username.
    pub async fn get_user(&self, username: &str) -> Result<SingleResponse<User>, ApiClientError> {
        let url = Self::url(&format!(
            "/users/by/username/{username}?user.fields={}",
            user_fields(),
        ));
        self.bearer_get(&url).await
    }

    /// Look up a user by numeric ID.
    pub async fn get_user_by_id(
        &self,
        user_id: &str,
    ) -> Result<SingleResponse<User>, ApiClientError> {
        let url = Self::url(&format!("/users/{user_id}?user.fields={}", user_fields(),));
        self.bearer_get(&url).await
    }

    /// Get a user's tweet timeline.
    pub async fn get_timeline(
        &self,
        user_id: &str,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<Tweet>, ApiClientError> {
        let max_results = max_results.clamp(10, 100);

        let mut url = Self::url(&format!(
            "/users/{user_id}/tweets?max_results={max_results}\
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

    /// Get the authenticated user's reverse-chronological home timeline.
    pub async fn get_home_timeline(
        &mut self,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<Tweet>, ApiClientError> {
        let my_id = self.get_my_user_id().await?;
        let max_results = max_results.clamp(10, 100);

        let mut url = Self::url(&format!(
            "/users/{my_id}/timelines/reverse_chronological?max_results={max_results}\
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

    /// Get a user's followers.
    pub async fn get_followers(
        &self,
        user_id: &str,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<User>, ApiClientError> {
        let max_results = max_results.clamp(1, 1000);

        let mut url = Self::url(&format!(
            "/users/{user_id}/followers?max_results={max_results}&user.fields={}",
            user_fields(),
        ));

        if let Some(token) = pagination_token {
            url.push_str(&format!("&pagination_token={token}"));
        }

        self.bearer_get(&url).await
    }

    /// Get users that a user is following.
    pub async fn get_following(
        &self,
        user_id: &str,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<User>, ApiClientError> {
        let max_results = max_results.clamp(1, 1000);

        let mut url = Self::url(&format!(
            "/users/{user_id}/following?max_results={max_results}&user.fields={}",
            user_fields(),
        ));

        if let Some(token) = pagination_token {
            url.push_str(&format!("&pagination_token={token}"));
        }

        self.bearer_get(&url).await
    }

    /// Get the authenticated user's mentions.
    pub async fn get_mentions(
        &mut self,
        max_results: u32,
        pagination_token: Option<&str>,
    ) -> Result<ListResponse<Tweet>, ApiClientError> {
        let my_id = self.get_my_user_id().await?;
        let max_results = max_results.clamp(10, 100);

        let mut url = Self::url(&format!(
            "/users/{my_id}/mentions?max_results={max_results}\
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
}
