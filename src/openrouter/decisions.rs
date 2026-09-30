//! Jev (TypeSafe) decision primitives via OpenRouter's Decisions API.
//!
//! Jev is a decision model, not a generator: it takes a `state` and a map of
//! typed questions (`choice`, `noul`, `score`) and returns probabilities.
//! See `docs/jev-design.md` for how xplorertui uses it.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

use super::OpenRouterError;
use super::client::OpenRouterClient;

/// The Decisions API lives under `/api/alpha`, not `/api/v1`.
pub const DECISIONS_URL: &str = "https://openrouter.ai/api/alpha/decisions";

/// Pinned default. Thresholds are version-specific, so don't track `-latest`.
pub const DEFAULT_JEV_MODEL: &str = "typesafe/jev-1.13";

/// Default number of in-flight Decisions requests.
pub const DEFAULT_CONCURRENCY: usize = 8;

/// Retries for 429 / 5xx / in-flight-budget 402 responses.
const MAX_RETRIES: u32 = 3;

/// Below this `choice` confidence a cluster label is shown as mixed.
pub const MIN_TOPIC_CONFIDENCE: f64 = 0.5;

/// A runner-up topic is shown only if it has at least this probability.
const SECONDARY_TOPIC_MIN_PROB: f64 = 0.25;

/// Maximum tweets sent per cluster-labelling request.
const MAX_POSTS_PER_CLUSTER: usize = 8;

/// Key of the catch-all option in every topic taxonomy.
pub const OTHER_TOPIC: &str = "other";

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DecisionRequest {
    pub model: String,
    pub state: serde_json::Value,
    pub questions: BTreeMap<String, Question>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Pick one of N options (key → description).
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
    },
    /// Probability that a statement holds.
    Noul { instructions: String },
    /// Position on an ordered scale (lowest first).
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice {
        choice: String,
        #[serde(default)]
        confidence: Option<f64>,
        #[serde(default)]
        probabilities: HashMap<String, f64>,
    },
    Noul {
        noul: f64,
    },
    Score {
        /// Probability-weighted index into the criteria (0-based).
        score: f64,
        #[serde(default)]
        confidence: Option<f64>,
        /// Keyed by criteria index as a string ("0", "1", ...).
        #[serde(default)]
        probabilities: HashMap<String, f64>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecisionUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub model: String,
    pub answers: HashMap<String, Answer>,
    #[serde(default)]
    pub usage: DecisionUsage,
}

impl DecisionResponse {
    pub fn choice(&self, key: &str) -> Option<(&str, f64, &HashMap<String, f64>)> {
        match self.answers.get(key)? {
            Answer::Choice {
                choice,
                confidence,
                probabilities,
            } => {
                let conf = confidence
                    .or_else(|| probabilities.get(choice).copied())
                    .unwrap_or(0.0);
                Some((choice.as_str(), conf, probabilities))
            }
            _ => None,
        }
    }

    pub fn noul(&self, key: &str) -> Option<f64> {
        match self.answers.get(key)? {
            Answer::Noul { noul } => Some(*noul),
            _ => None,
        }
    }

    pub fn score(&self, key: &str) -> Option<f64> {
        match self.answers.get(key)? {
            Answer::Score { score, .. } => Some(*score),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

impl OpenRouterClient {
    /// Send one Decisions request, retrying on rate limits and transient
    /// server errors (honouring `Retry-After` when present).
    pub async fn decide(
        &self,
        request: &DecisionRequest,
    ) -> Result<DecisionResponse, OpenRouterError> {
        let mut attempt = 0;
        loop {
            let resp = self.http().post(DECISIONS_URL).json(request).send().await?;
            let status = resp.status();
            if status.is_success() {
                let body = resp.text().await?;
                tracing::debug!("jev response: {body}");
                return Ok(serde_json::from_str(&body)?);
            }

            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let body = resp.text().await.unwrap_or_default();

            if attempt < MAX_RETRIES && is_retryable(status.as_u16(), &body) {
                let delay = retry_after
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| Duration::from_millis(500 << attempt));
                tracing::debug!("jev {status}, retrying in {delay:?}");
                tokio::time::sleep(delay).await;
                attempt += 1;
                continue;
            }

            return Err(OpenRouterError::ApiError {
                status: status.as_u16(),
                detail: body,
            });
        }
    }

    /// Run many Decisions requests with bounded concurrency. Results are
    /// returned in input order; individual failures don't abort the batch.
    pub async fn decide_all(
        &self,
        requests: Vec<DecisionRequest>,
        concurrency: usize,
    ) -> Vec<Result<DecisionResponse, OpenRouterError>> {
        stream::iter(requests)
            .map(|req| async move { self.decide(&req).await })
            .buffered(concurrency.max(1))
            .collect()
            .await
    }
}

fn is_retryable(status: u16, body: &str) -> bool {
    match status {
        // Any 5xx, including 529 (provider overloaded), is transient.
        429 | 500..=599 => true,
        // OpenRouter uses 402 for its in-flight budget, which clears on its own.
        402 => body.contains("openrouter_in_flight_budget"),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Topic taxonomy
// ---------------------------------------------------------------------------

/// Default topics: the TweetTopic taxonomy (cardiffnlp/tweet_topic_multi),
/// with science & technology split into finer tech topics, plus `other`.
pub fn default_topics() -> BTreeMap<String, String> {
    [
        ("ai_ml", "AI, machine learning, LLMs, model releases"),
        (
            "programming",
            "Software engineering, programming languages, developer tools",
        ),
        (
            "science_tech",
            "Science and technology (other than AI or programming)",
        ),
        ("crypto", "Cryptocurrency, blockchain, web3"),
        ("business", "Business, startups, entrepreneurs, products"),
        ("finance", "Finance, markets, investing, the economy"),
        ("news_politics", "News, politics, government, social issues"),
        ("arts_culture", "Arts and culture, books, design"),
        ("pop_culture", "Celebrities and pop culture"),
        ("film_tv", "Film, TV, and video"),
        ("music", "Music"),
        ("gaming", "Gaming"),
        ("sports", "Sports"),
        ("fitness_health", "Fitness and health"),
        ("food_dining", "Food and dining"),
        ("travel", "Travel and adventure"),
        ("fashion", "Fashion and style"),
        ("education", "Learning and education"),
        (
            "daily_life",
            "Daily life, family, relationships, personal updates",
        ),
        (OTHER_TOPIC, "None of the above"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Make sure a user-supplied taxonomy has a catch-all option.
pub fn with_other(mut topics: BTreeMap<String, String>) -> BTreeMap<String, String> {
    topics
        .entry(OTHER_TOPIC.to_string())
        .or_insert_with(|| "None of the above".to_string());
    topics
}

/// Human-readable label for a topic key: the first clause of its description
/// ("AI, machine learning, ..." → "AI").
pub fn topic_display(key: &str, topics: &BTreeMap<String, String>) -> String {
    match topics.get(key) {
        Some(desc) if key != OTHER_TOPIC => desc
            .split([',', '('])
            .next()
            .unwrap_or(desc)
            .trim()
            .to_string(),
        _ => key.replace('_', " "),
    }
}

// ---------------------------------------------------------------------------
// Request builders
// ---------------------------------------------------------------------------

fn clean_text(text: &str, max_chars: usize) -> String {
    text.chars()
        .take(max_chars)
        .collect::<String>()
        .replace('\n', " ")
}

/// Label one cluster: a single `choice` over the taxonomy, with up to
/// `MAX_POSTS_PER_CLUSTER` representative posts as state.
pub fn cluster_topic_request(
    model: &str,
    posts: &[&str],
    topics: &BTreeMap<String, String>,
) -> DecisionRequest {
    let posts: Vec<String> = posts
        .iter()
        .take(MAX_POSTS_PER_CLUSTER)
        .map(|p| clean_text(p, 280))
        .collect();
    DecisionRequest {
        model: model.to_string(),
        state: serde_json::json!({ "posts": posts }),
        questions: BTreeMap::from([(
            "topic".to_string(),
            Question::Choice {
                instructions: "Which topic do most of these posts share?".into(),
                criteria: topics.clone(),
            },
        )]),
    }
}

/// Classify one post: topic `choice` plus a few `noul` tags. One post per
/// request: batching posts into one state loses accuracy fast (see the
/// design note), while extra questions are nearly free.
pub fn classify_post_request(
    model: &str,
    post: &str,
    topics: &BTreeMap<String, String>,
) -> DecisionRequest {
    let mut questions = BTreeMap::from([(
        "topic".to_string(),
        Question::Choice {
            instructions: "Which topic is this post about?".into(),
            criteria: topics.clone(),
        },
    )]);
    for (key, instructions) in POST_TAGS {
        questions.insert(
            key.to_string(),
            Question::Noul {
                instructions: instructions.to_string(),
            },
        );
    }
    DecisionRequest {
        model: model.to_string(),
        state: serde_json::json!({ "post": clean_text(post, 1000) }),
        questions,
    }
}

/// `noul` tags asked alongside the topic in `classify_post_request`.
pub const POST_TAGS: &[(&str, &str)] = &[
    ("is_question", "Is this post asking a question?"),
    ("is_news", "Is this post announcing or reporting news?"),
    (
        "is_promo",
        "Is this post promotional, an advertisement, or spam?",
    ),
];

/// Relevance scale for search reranking, lowest first.
pub const RELEVANCE_CRITERIA: &[&str] = &[
    "Unrelated",
    "Mentions the topic",
    "About the topic",
    "Directly answers / highly informative",
];

/// Score one search candidate against the query.
pub fn relevance_request(model: &str, query: &str, post: &str) -> DecisionRequest {
    DecisionRequest {
        model: model.to_string(),
        state: serde_json::json!({ "query": query, "post": clean_text(post, 1000) }),
        questions: BTreeMap::from([
            (
                "relevance".to_string(),
                Question::Score {
                    instructions: "How well does the post address the search query?".into(),
                    criteria: RELEVANCE_CRITERIA.iter().map(|s| s.to_string()).collect(),
                },
            ),
            (
                "substantive".to_string(),
                Question::Noul {
                    instructions: "Does the post contain substantive information rather \
                                   than promotion, spam, or a bare link?"
                        .into(),
                },
            ),
        ]),
    }
}

// ---------------------------------------------------------------------------
// Interpreting answers
// ---------------------------------------------------------------------------

/// Turn a cluster-labelling response into a display label, e.g. "AI",
/// "AI · Programming", or "Mixed" when Jev isn't confident.
pub fn cluster_label(resp: &DecisionResponse, topics: &BTreeMap<String, String>) -> Option<String> {
    let (choice, confidence, probs) = resp.choice("topic")?;
    if choice == OTHER_TOPIC || confidence < MIN_TOPIC_CONFIDENCE {
        return Some("Mixed".to_string());
    }
    let primary = topic_display(choice, topics);
    let runner_up = probs
        .iter()
        .filter(|(k, p)| {
            k.as_str() != choice && k.as_str() != OTHER_TOPIC && **p >= SECONDARY_TOPIC_MIN_PROB
        })
        .max_by(|a, b| a.1.total_cmp(b.1));
    Some(match runner_up {
        Some((k, _)) => format!("{primary} · {}", topic_display(k, topics)),
        None => primary,
    })
}

/// Rank key for a relevance response: expected relevance (0–3) weighted by
/// the probability the post is substantive.
pub fn relevance_key(resp: &DecisionResponse) -> Option<f64> {
    let score = resp.score("relevance")?;
    let substantive = resp.noul("substantive").unwrap_or(1.0);
    Some(score * substantive)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from a live call to typesafe/jev-1.13 (2026-09-27).
    const LIVE_RESPONSE: &str = r#"{"model":"typesafe/jev-1.13-20260917","answers":{"topic":{"type":"choice","choice":"rust","probabilities":{"sports":0,"other":0,"ai_ml":0,"rust":1},"confidence":1},"is_news":{"type":"noul","noul":0.95},"relevance":{"type":"score","score":1.99,"legend":{"0":"Unrelated","1":"Mentions the topic","2":"About the topic","3":"Directly answers / highly informative"},"probabilities":{"0":0,"1":0.1,"2":0.81,"3":0.09},"confidence":0.81}},"usage":{"input_tokens":446,"output_tokens":79,"cost":0.000018732},"id":"gen-dec-1790528946-TL2NZBiAx8prx4a7gxJF","provider":"TypeSafe"}"#;

    fn resp(json: &str) -> DecisionResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parses_live_response() {
        let r = resp(LIVE_RESPONSE);
        let (choice, conf, probs) = r.choice("topic").unwrap();
        assert_eq!(choice, "rust");
        assert_eq!(conf, 1.0);
        assert_eq!(probs["rust"], 1.0);
        assert_eq!(r.noul("is_news"), Some(0.95));
        assert_eq!(r.score("relevance"), Some(1.99));
        assert_eq!(r.usage.input_tokens, 446);
        // Wrong-typed lookups return None.
        assert_eq!(r.noul("topic"), None);
        assert!(r.choice("missing").is_none());
    }

    #[test]
    fn serializes_questions_with_type_tag() {
        let req = relevance_request("m", "q", "p");
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["questions"]["relevance"]["type"], "score");
        assert_eq!(v["questions"]["relevance"]["criteria"][0], "Unrelated");
        assert_eq!(v["questions"]["substantive"]["type"], "noul");
        assert!(v["questions"]["substantive"].get("criteria").is_none());
        assert_eq!(v["state"]["query"], "q");

        let req = cluster_topic_request("m", &["a\nb"; 20], &default_topics());
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["questions"]["topic"]["type"], "choice");
        assert_eq!(v["state"]["posts"].as_array().unwrap().len(), 8);
        assert_eq!(v["state"]["posts"][0], "a b");
    }

    #[test]
    fn classify_request_has_topic_and_tags() {
        let req = classify_post_request("m", "hello", &default_topics());
        assert!(req.questions.contains_key("topic"));
        for (k, _) in POST_TAGS {
            assert!(req.questions.contains_key(*k));
        }
    }

    #[test]
    fn default_topics_include_other() {
        assert!(default_topics().contains_key(OTHER_TOPIC));
        let t = with_other(BTreeMap::from([("x".into(), "X".into())]));
        assert!(t.contains_key(OTHER_TOPIC));
    }

    #[test]
    fn topic_display_uses_first_clause() {
        let t = default_topics();
        assert_eq!(topic_display("ai_ml", &t), "AI");
        assert_eq!(topic_display("sports", &t), "Sports");
        assert_eq!(topic_display("science_tech", &t), "Science and technology");
        assert_eq!(topic_display("unknown_key", &t), "unknown key");
    }

    fn topic_resp(choice: &str, conf: f64, probs: &[(&str, f64)]) -> DecisionResponse {
        let probs: HashMap<String, f64> = probs.iter().map(|(k, p)| (k.to_string(), *p)).collect();
        DecisionResponse {
            model: "m".into(),
            answers: HashMap::from([(
                "topic".into(),
                Answer::Choice {
                    choice: choice.into(),
                    confidence: Some(conf),
                    probabilities: probs,
                },
            )]),
            usage: DecisionUsage::default(),
        }
    }

    #[test]
    fn cluster_label_variants() {
        let t = default_topics();
        let r = topic_resp("sports", 1.0, &[("sports", 1.0)]);
        assert_eq!(cluster_label(&r, &t).unwrap(), "Sports");

        let r = topic_resp(
            "ai_ml",
            0.6,
            &[("ai_ml", 0.6), ("programming", 0.35), ("other", 0.05)],
        );
        assert_eq!(cluster_label(&r, &t).unwrap(), "AI · Software engineering");

        let r = topic_resp("other", 0.84, &[("other", 0.86), ("programming", 0.13)]);
        assert_eq!(cluster_label(&r, &t).unwrap(), "Mixed");

        let r = topic_resp("finance", 0.4, &[("finance", 0.4), ("business", 0.3)]);
        assert_eq!(cluster_label(&r, &t).unwrap(), "Mixed");
    }

    #[test]
    fn relevance_key_weights_by_substantive() {
        let mut r = resp(LIVE_RESPONSE);
        r.answers
            .insert("substantive".into(), Answer::Noul { noul: 0.5 });
        assert!((relevance_key(&r).unwrap() - 0.995).abs() < 1e-9);
    }

    #[test]
    fn retryable_statuses() {
        assert!(is_retryable(429, ""));
        assert!(is_retryable(503, ""));
        assert!(is_retryable(529, ""));
        assert!(!is_retryable(600, ""));
        assert!(is_retryable(
            402,
            r#"{"limit_source":"openrouter_in_flight_budget"}"#
        ));
        assert!(!is_retryable(402, "insufficient credits"));
        assert!(!is_retryable(400, ""));
    }

    /// Live check against OpenRouter. Run with:
    /// `OPENROUTER_API_KEY=... cargo test jev_live -- --ignored`
    #[tokio::test]
    #[ignore = "calls the live OpenRouter Decisions API"]
    async fn jev_live_cluster_labels() {
        let key = std::env::var("OPENROUTER_API_KEY").expect("OPENROUTER_API_KEY");
        let client = OpenRouterClient::new(key);
        let topics = default_topics();
        let clusters: [&[&str]; 3] = [
            &[
                "Lakers won in OT, LeBron 40",
                "Marathon PR today, 3:12",
                "Champions League draw is out",
            ],
            &[
                "New open-weights LLM tops coding leaderboards",
                "Fine-tuned a 0.8B model on my notes",
            ],
            &[
                "Tokio shipped io_uring",
                "Best ramen in Shibuya",
                "Fed holds rates",
                "My cat spilled my coffee",
            ],
        ];
        let requests = clusters
            .iter()
            .map(|posts| cluster_topic_request(DEFAULT_JEV_MODEL, posts, &topics))
            .collect();
        let labels: Vec<String> = client
            .decide_all(requests, DEFAULT_CONCURRENCY)
            .await
            .into_iter()
            .map(|r| cluster_label(&r.unwrap(), &topics).unwrap())
            .collect();
        assert_eq!(labels[0], "Sports");
        assert!(labels[1].starts_with("AI"), "{labels:?}");
        assert_eq!(labels[2], "Mixed");
    }
}
