//! Photos in Jev cluster requests: which posts send one, and how a photo
//! goes into the request state.
//!
//! Measured on jev-1.14 (docs/jev-design.md §10): a 680 px photo adds about
//! 800 input tokens and 0.05 s to a request, and 8 photos in one state took
//! 1.06 s against 0.48 s with none. Photos did not raise topic accuracy on
//! posts that have text, so only posts with thin text send theirs.

use super::decisions::DecisionRequest;

/// Most photos in one cluster request.
pub const MAX_PHOTOS_PER_CLUSTER: usize = 2;
/// Most photos in one `:topics` run, over all clusters.
pub const MAX_PHOTOS_PER_RUN: usize = 24;

/// Pick the posts that send a photo. `wants[c][j]` says whether post `j`
/// of cluster `c` (in the order the request lists them) wants its photo.
/// Clusters take turns, one photo each per round, so a big cluster cannot
/// use the whole run budget. Returns the same shape, `true` where chosen.
pub fn select_photos(wants: &[Vec<bool>], per_cluster: usize, per_run: usize) -> Vec<Vec<bool>> {
    let mut chosen: Vec<Vec<bool>> = wants.iter().map(|w| vec![false; w.len()]).collect();
    let mut candidates: Vec<std::vec::IntoIter<usize>> = wants
        .iter()
        .map(|w| {
            let idx: Vec<usize> = (0..w.len()).filter(|&j| w[j]).take(per_cluster).collect();
            idx.into_iter()
        })
        .collect();
    let mut left = per_run;
    while left > 0 {
        let mut any = false;
        for (c, next) in candidates.iter_mut().enumerate() {
            if left == 0 {
                break;
            }
            if let Some(j) = next.next() {
                chosen[c][j] = true;
                left -= 1;
                any = true;
            }
        }
        if !any {
            break;
        }
    }
    chosen
}

/// Put photos into a cluster request. `photos[j]` is a `data:` URL for
/// post `j` of the state's `posts` array, or `None`. A post with a photo
/// becomes `{"text": …, "image": <image_url part>}`; Jev reads image parts
/// at any depth of the state. Posts without one stay plain strings.
pub fn attach_photos(req: &mut DecisionRequest, photos: &[Option<&str>]) {
    let Some(posts) = req.state.get_mut("posts").and_then(|p| p.as_array_mut()) else {
        return;
    };
    for (post, photo) in posts.iter_mut().zip(photos) {
        let Some(url) = photo else {
            continue;
        };
        let text = post.as_str().unwrap_or_default().to_string();
        *post = serde_json::json!({
            "text": text,
            "image": { "type": "image_url", "image_url": { "url": url } },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openrouter::decisions::pick_label_request;

    #[test]
    fn clusters_take_turns_until_the_run_budget_ends() {
        let wants = vec![
            vec![true, true, true],
            vec![true, false, true],
            vec![false, true],
        ];
        let chosen = select_photos(&wants, 2, 4);
        assert_eq!(chosen[0], vec![true, true, false]);
        assert_eq!(chosen[1], vec![true, false, false]);
        assert_eq!(chosen[2], vec![false, true]);
        let total: usize = chosen.iter().flatten().filter(|&&b| b).count();
        assert_eq!(total, 4);
    }

    #[test]
    fn per_cluster_cap_holds_with_budget_to_spare() {
        let wants = vec![vec![true; 8]];
        let chosen = select_photos(&wants, 2, 24);
        assert_eq!(chosen[0].iter().filter(|&&b| b).count(), 2);
    }

    #[test]
    fn nothing_wanted_means_nothing_chosen() {
        let chosen = select_photos(&[vec![false, false]], 2, 24);
        assert_eq!(chosen, vec![vec![false, false]]);
    }

    #[test]
    fn attached_photo_becomes_an_image_part_next_to_the_text() {
        let mut req = pick_label_request("m", &["ラーメン", "long text"], &["Food".into()]);
        attach_photos(&mut req, &[Some("data:image/jpeg;base64,AAA"), None]);
        let posts = req.state["posts"].as_array().unwrap();
        assert_eq!(posts[0]["text"], "ラーメン");
        assert_eq!(posts[0]["image"]["type"], "image_url");
        assert_eq!(
            posts[0]["image"]["image_url"]["url"],
            "data:image/jpeg;base64,AAA"
        );
        assert_eq!(posts[1], "long text");
    }

    /// A real X photo (a bowl of microgreens) goes to Jev 1.14 and OpenRouter's
    /// gemini-embedding-2 through the same `PhotoCache`.
    const LIVE_PHOTO: &str = "https://pbs.twimg.com/media/HU502HFX0AAMWSH.jpg";

    #[tokio::test]
    #[ignore = "calls X's image CDN and the live OpenRouter APIs"]
    async fn live_photo_reaches_jev_and_the_embedding_model() {
        use crate::openrouter::client::OpenRouterClient;
        use crate::openrouter::decisions::DEFAULT_JEV_MODEL;

        let key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY");
        let client = OpenRouterClient::new(key);
        let cache = crate::media::PhotoCache::new();
        let photo = cache
            .get(LIVE_PHOTO, crate::media::cache::BATCH_TIMEOUT * 5)
            .await
            .expect("photo");

        // Text that says nothing about the subject: the photo has to decide.
        let candidates = vec![
            "Home gardening and food".to_string(),
            "Football".to_string(),
        ];
        let mut req = pick_label_request(DEFAULT_JEV_MODEL, &["look at this"], &candidates);
        attach_photos(&mut req, &[Some(&photo.data_url)]);
        let resp = client.decide(&req).await.expect("decision");
        let (choice, confidence, _) = resp.choice("topic").expect("topic answer");
        println!(
            "choice {choice} at {confidence}, {} input tokens",
            resp.usage.input_tokens
        );
        assert_eq!(choice, "c0");

        let docs = [(
            "look at this".to_string(),
            std::sync::Arc::clone(&photo.data_url),
        )];
        let embedded = client
            .embed_documents("google/gemini-embedding-2", &docs)
            .await
            .expect("embedding");
        assert_eq!(embedded.data.len(), 1);
        assert!(embedded.data[0].embedding.len() > 100);
    }
}
