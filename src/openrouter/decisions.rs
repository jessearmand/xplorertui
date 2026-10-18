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
pub const DEFAULT_JEV_MODEL: &str = "typesafe/jev-1.14";

/// Default number of in-flight Decisions requests.
pub const DEFAULT_CONCURRENCY: usize = 8;

/// Retries for 429 / 5xx / in-flight-budget 402 responses.
const MAX_RETRIES: u32 = 3;

/// Below this `choice` confidence a cluster label is shown as mixed.
pub const MIN_TOPIC_CONFIDENCE: f64 = 0.5;

/// A runner-up topic is shown only if it has at least this probability.
const SECONDARY_TOPIC_MIN_PROB: f64 = 0.25;

/// Maximum tweets sent per cluster-labelling request.
pub const MAX_POSTS_PER_CLUSTER: usize = 8;

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
    /// Send one Decisions request, retrying on rate limits, transient
    /// server errors (honouring `Retry-After` when present) and dropped
    /// connections.
    pub async fn decide(
        &self,
        request: &DecisionRequest,
    ) -> Result<DecisionResponse, OpenRouterError> {
        let mut attempt = 0;
        loop {
            // Send and read the whole body, so a connection dropped after
            // the headers is handled like one dropped before them.
            let exchange = async {
                let resp = self.http().post(DECISIONS_URL).json(request).send().await?;
                let status = resp.status();
                let retry_after = resp
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok());
                let body = resp.text().await?;
                Ok::<_, reqwest::Error>((status, retry_after, body))
            };
            let (status, retry_after, body) = match exchange.await {
                Ok(parts) => parts,
                // Dropped or reset connections are transient; a decision
                // request is safe to resend.
                Err(e)
                    if attempt < MAX_RETRIES
                        && (e.is_connect() || e.is_request() || e.is_body() || e.is_decode()) =>
                {
                    tracing::debug!("jev request failed ({e}), retrying");
                    tokio::time::sleep(Duration::from_millis(500 << attempt)).await;
                    attempt += 1;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            if status.is_success() {
                tracing::debug!("jev response: {body}");
                return Ok(serde_json::from_str(&body)?);
            }

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
        // A model refusing a question (e.g. on spam or adult posts) comes
        // back as a 502 and will refuse again.
        502 if body.contains("refused to answer") => false,
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

/// State for a cluster question: up to `MAX_POSTS_PER_CLUSTER` posts.
fn cluster_state(posts: &[&str]) -> serde_json::Value {
    let posts: Vec<String> = posts
        .iter()
        .take(MAX_POSTS_PER_CLUSTER)
        .map(|p| clean_text(p, 280))
        .collect();
    serde_json::json!({ "posts": posts })
}

/// Label one cluster: a single `choice` over the taxonomy, with up to
/// `MAX_POSTS_PER_CLUSTER` representative posts as state.
pub fn cluster_topic_request(
    model: &str,
    posts: &[&str],
    topics: &BTreeMap<String, String>,
) -> DecisionRequest {
    DecisionRequest {
        model: model.to_string(),
        state: cluster_state(posts),
        questions: BTreeMap::from([(
            "topic".to_string(),
            Question::Choice {
                instructions: "Which topic do most of these posts share?".into(),
                criteria: topics.clone(),
            },
        )]),
    }
}

/// Asked of each cluster before labelling, to route `:topics`.
pub const TECH_QUESTION: &str = "Are most of these posts about technology: software, AI, \
     computing hardware, crypto, science or the tech industry?";

/// Propose-then-pick runs when at least this share of posts sits in
/// clusters Jev calls tech. Below it, the chat model writes the labels:
/// on non-tech timelines its own labels score higher (design note §9).
pub const MIN_TECH_SHARE: f64 = 0.5;

/// Does this cluster's content count as tech? One `noul` over up to
/// `MAX_POSTS_PER_CLUSTER` posts.
pub fn tech_request(model: &str, posts: &[&str]) -> DecisionRequest {
    DecisionRequest {
        model: model.to_string(),
        state: cluster_state(posts),
        questions: BTreeMap::from([(
            "tech".to_string(),
            Question::Noul {
                instructions: TECH_QUESTION.into(),
            },
        )]),
    }
}

/// Share of posts in clusters Jev calls tech (P >= 0.5), weighted by
/// cluster size. Failed requests are left out; `None` if all failed.
pub fn tech_share(responses: &[Option<&DecisionResponse>], sizes: &[usize]) -> Option<f64> {
    let (mut total, mut tech) = (0usize, 0usize);
    for (resp, &size) in responses.iter().zip(sizes) {
        let Some(p) = resp.and_then(|r| r.noul("tech")) else {
            continue;
        };
        total += size;
        if p >= 0.5 {
            tech += size;
        }
    }
    (total > 0).then(|| tech as f64 / total as f64)
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
// Proposed labels: a small chat model proposes, Jev picks
// ---------------------------------------------------------------------------

/// System prompt asking a chat model for candidate labels per cluster.
/// Proposing from the clusters' own posts keeps candidates at the
/// clusters' granularity (corpus-wide candidates came out too niche).
/// The last label must fit the whole cluster: with only narrow candidates,
/// Jev often answers "None of these" for a clean cluster.
pub const PROPOSE_LABELS_PROMPT: &str = "For each cluster of tweets, propose 3 distinct candidate \
     topic labels (2-5 words each), ordered from most specific to most general. The last label \
     must fit every tweet in the cluster. Reply with one line per cluster in the format \
     `Cluster <index>: label A | label B | label C`. Include every cluster 0 through N-1. No \
     preamble, no explanations.";

/// Longest candidate label kept from a proposal.
const MAX_CANDIDATE_CHARS: usize = 60;

/// Most candidates offered to Jev in one `choice`.
pub const MAX_CANDIDATES: usize = 64;

/// Output budget for a proposal of three short labels per cluster.
pub fn propose_max_tokens(num_clusters: usize) -> u32 {
    (num_clusters as u32).saturating_mul(48).clamp(64, 1024)
}

/// Parse `Cluster <n>: a | b | c` lines into one candidate pool,
/// deduplicated case-insensitively. Lines without the prefix (preamble,
/// explanations) are ignored. Returns `None` unless every cluster
/// `0..num_clusters` has a usable candidate and the pool has at least one
/// candidate per cluster: a truncated or partial proposal would otherwise
/// leave clusters unlabelled instead of falling back to the taxonomy.
pub fn parse_label_candidates(content: &str, num_clusters: usize) -> Option<Vec<String>> {
    let mut pool: Vec<String> = Vec::new();
    let mut covered = vec![false; num_clusters];
    for line in content.lines() {
        let line = line.trim().trim_start_matches(['-', '*', ' ']);
        let Some((head, rest)) = line.split_once(':') else {
            continue;
        };
        let head = head.trim().trim_end_matches('*').to_ascii_lowercase();
        let Some(cluster) = head
            .strip_prefix("cluster")
            .and_then(|n| n.trim().parse::<usize>().ok())
        else {
            continue;
        };
        for candidate in rest.split('|') {
            let c = candidate
                .trim()
                .trim_matches(|ch| matches!(ch, '*' | '`' | '"'))
                .trim();
            if c.is_empty() || c.chars().count() > MAX_CANDIDATE_CHARS {
                continue;
            }
            if let Some(slot) = covered.get_mut(cluster) {
                *slot = true;
            }
            if pool.len() < MAX_CANDIDATES && !pool.iter().any(|p| p.eq_ignore_ascii_case(c)) {
                pool.push(c.to_string());
            }
        }
    }
    (covered.iter().all(|&c| c) && pool.len() >= num_clusters).then_some(pool)
}

/// Option key for candidate `i` in `pick_label_request`.
fn candidate_key(i: usize) -> String {
    format!("c{i}")
}

/// Ask which proposed label fits one cluster. Every cluster gets the same
/// candidate pool, so the answers can be compared across clusters.
pub fn pick_label_request(model: &str, posts: &[&str], candidates: &[String]) -> DecisionRequest {
    let mut criteria: BTreeMap<String, String> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (candidate_key(i), c.clone()))
        .collect();
    criteria.insert(OTHER_TOPIC.to_string(), "None of these".to_string());
    let mut req = cluster_topic_request(model, posts, &criteria);
    req.questions.insert(
        "topic".to_string(),
        Question::Choice {
            instructions: "Which label best describes the topic most of these posts share?".into(),
            criteria,
        },
    );
    req
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

/// Outcome of `assign_unique_labels` for one cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPick {
    /// Index into the candidate pool.
    Candidate(usize),
    /// Jev judged none of the candidates to fit (shown as "Mixed").
    Mixed,
    /// The request failed, or every remaining candidate went to a
    /// cluster with a stronger claim.
    Unlabelled,
}

/// Pick one distinct candidate per cluster, maximising the joint
/// probability Jev gave each cluster's label (an optimal assignment on
/// `-ln p`, so two clusters never share a label). Clusters where Jev puts
/// at least `MIN_TOPIC_CONFIDENCE` on "None of these" are `Mixed` and take
/// no candidate: forcing one there gives a confidently wrong label.
pub fn assign_unique_labels(
    responses: &[Option<&DecisionResponse>],
    num_candidates: usize,
) -> Vec<LabelPick> {
    let mut out = vec![LabelPick::Unlabelled; responses.len()];
    let mut rows: Vec<(usize, &HashMap<String, f64>)> = Vec::new();
    for (c, r) in responses.iter().enumerate() {
        let Some((_, _, probs)) = r.and_then(|r| r.choice("topic")) else {
            continue;
        };
        if probs.get(OTHER_TOPIC).copied().unwrap_or(0.0) >= MIN_TOPIC_CONFIDENCE {
            out[c] = LabelPick::Mixed;
        } else {
            rows.push((c, probs));
        }
    }
    if rows.is_empty() || num_candidates == 0 {
        return out;
    }
    // Surplus clusters land on dummy columns that cost more than any
    // real label (-ln 1e-6 ≈ 13.8).
    const DUMMY_COST: f64 = 20.0;
    let width = num_candidates.max(rows.len());
    let cost: Vec<Vec<f64>> = rows
        .iter()
        .map(|(_, probs)| {
            (0..width)
                .map(|i| {
                    if i < num_candidates {
                        let p = probs.get(&candidate_key(i)).copied().unwrap_or(0.0);
                        -p.max(1e-6).ln()
                    } else {
                        DUMMY_COST
                    }
                })
                .collect()
        })
        .collect();
    for ((cluster, _), col) in rows.iter().zip(min_cost_assignment(&cost)) {
        if col < num_candidates {
            out[*cluster] = LabelPick::Candidate(col);
        }
    }
    out
}

/// Hungarian algorithm: the column for each row that minimises total cost.
/// Requires `rows <= columns` and finite costs. O(rows² · columns).
fn min_cost_assignment(cost: &[Vec<f64>]) -> Vec<usize> {
    let n = cost.len();
    let Some(m) = cost.first().map(Vec::len) else {
        return Vec::new();
    };
    debug_assert!(n <= m);
    // 1-based potentials and matching; column 0 is a sentinel.
    let mut u = vec![0.0; n + 1];
    let mut v = vec![0.0; m + 1];
    let mut row_of = vec![0usize; m + 1];
    let mut way = vec![0usize; m + 1];
    for i in 1..=n {
        row_of[0] = i;
        let mut j0 = 0;
        let mut min_v = vec![f64::INFINITY; m + 1];
        let mut used = vec![false; m + 1];
        loop {
            used[j0] = true;
            let i0 = row_of[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0;
            for j in 1..=m {
                if used[j] {
                    continue;
                }
                let reduced = cost[i0 - 1][j - 1] - u[i0] - v[j];
                if reduced < min_v[j] {
                    min_v[j] = reduced;
                    way[j] = j0;
                }
                if min_v[j] < delta {
                    delta = min_v[j];
                    j1 = j;
                }
            }
            for j in 0..=m {
                if used[j] {
                    u[row_of[j]] += delta;
                    v[j] -= delta;
                } else {
                    min_v[j] -= delta;
                }
            }
            j0 = j1;
            if row_of[j0] == 0 {
                break;
            }
        }
        while j0 != 0 {
            let j1 = way[j0];
            row_of[j0] = row_of[j1];
            j0 = j1;
        }
    }
    let mut col_of = vec![0; n];
    for (j, &i) in row_of.iter().enumerate().skip(1) {
        if i != 0 {
            col_of[i - 1] = j - 1;
        }
    }
    col_of
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
        assert!(!is_retryable(
            502,
            r#"{"error":{"message":"OpenAI refused to answer question \"topic\"","code":502}}"#
        ));
    }

    #[test]
    fn parses_label_candidates() {
        let content = "Here are the labels:\n\
            Cluster 0: Rust TUI tools | Terminal apps | Programming\n\
            **Cluster 1**: \"AI coding agents\" | Developer tools | programming\n\
            - Cluster 2: Crypto markets | | Finance\n\
            Note: these are short labels.\n\
            Cluster 3: This candidate is far too long to be a short topic label for any cluster at all";
        assert_eq!(
            parse_label_candidates(content, 3).unwrap(),
            [
                "Rust TUI tools",
                "Terminal apps",
                "Programming",
                "AI coding agents",
                "Developer tools",
                "Crypto markets",
                "Finance",
            ]
        );
        // Cluster 3 has no usable candidate.
        assert_eq!(parse_label_candidates(content, 4), None);
    }

    #[test]
    fn rejects_partial_proposals() {
        // Truncated reply: cluster 1 never appears.
        let truncated = "Cluster 0: Rust | Terminal apps | Programming\nCluster 2: Crypto";
        assert_eq!(parse_label_candidates(truncated, 3), None);
        // Every cluster present, but only two distinct labels for three clusters.
        let thin = "Cluster 0: Rust\nCluster 1: rust\nCluster 2: Crypto";
        assert_eq!(parse_label_candidates(thin, 3), None);
        // A cluster whose labels all duplicate another's still counts as covered.
        let dup = "Cluster 0: Rust | Crypto\nCluster 1: rust\n";
        assert_eq!(parse_label_candidates(dup, 2).unwrap(), ["Rust", "Crypto"]);
    }

    fn noul_resp(key: &str, p: f64) -> DecisionResponse {
        DecisionResponse {
            model: "m".into(),
            answers: HashMap::from([(key.into(), Answer::Noul { noul: p })]),
            usage: DecisionUsage::default(),
        }
    }

    #[test]
    fn tech_request_asks_one_noul() {
        let req = tech_request("m", &["post"]);
        assert_eq!(req.questions.len(), 1);
        assert!(matches!(req.questions["tech"], Question::Noul { .. }));
        assert_eq!(req.state["posts"][0], "post");
    }

    #[test]
    fn tech_share_weights_by_cluster_size() {
        let tech = noul_resp("tech", 0.9);
        let other = noul_resp("tech", 0.1);
        // 30 tech posts out of 40.
        let share = tech_share(&[Some(&tech), Some(&other)], &[30, 10]);
        assert_eq!(share, Some(0.75));
        // Failed requests drop out of both sides.
        assert_eq!(tech_share(&[None, Some(&other)], &[30, 10]), Some(0.0));
        assert_eq!(tech_share(&[None, None], &[30, 10]), None);
    }

    #[test]
    fn pick_request_lists_candidates_and_other() {
        let req = pick_label_request("m", &["post"], &["Rust".into(), "Crypto".into()]);
        let Question::Choice { criteria, .. } = &req.questions["topic"] else {
            panic!("expected choice");
        };
        assert_eq!(criteria["c0"], "Rust");
        assert_eq!(criteria["c1"], "Crypto");
        assert!(criteria.contains_key(OTHER_TOPIC));
        assert_eq!(req.state["posts"][0], "post");
    }

    #[test]
    fn assign_unique_labels_resolves_collisions() {
        use LabelPick::*;
        // Both clusters prefer c0; cluster 1 cares less, so it moves to c1.
        let a = topic_resp("c0", 0.9, &[("c0", 0.9), ("c1", 0.05), ("c2", 0.05)]);
        let b = topic_resp("c0", 0.6, &[("c0", 0.6), ("c1", 0.35), ("c2", 0.05)]);
        assert_eq!(
            assign_unique_labels(&[Some(&a), Some(&b)], 3),
            [Candidate(0), Candidate(1)]
        );
        // A failed request gets no label and doesn't block the others.
        assert_eq!(
            assign_unique_labels(&[None, Some(&b)], 3),
            [Unlabelled, Candidate(0)]
        );
        // More clusters than candidates: the weakest claim goes unlabelled.
        assert_eq!(
            assign_unique_labels(&[Some(&a), Some(&b)], 1),
            [Candidate(0), Unlabelled]
        );
    }

    #[test]
    fn assign_unique_labels_keeps_none_of_these() {
        use LabelPick::*;
        // Jev says none of the candidates fit cluster 0: it stays Mixed and
        // leaves c0 free for cluster 1.
        let junk = topic_resp(OTHER_TOPIC, 0.85, &[("c0", 0.1), (OTHER_TOPIC, 0.85)]);
        let b = topic_resp("c0", 0.6, &[("c0", 0.6), ("c1", 0.35), (OTHER_TOPIC, 0.05)]);
        assert_eq!(
            assign_unique_labels(&[Some(&junk), Some(&b)], 2),
            [Mixed, Candidate(0)]
        );
        // Below the threshold, "other" doesn't block a label.
        let unsure = topic_resp("c1", 0.4, &[("c1", 0.4), (OTHER_TOPIC, 0.35)]);
        assert_eq!(assign_unique_labels(&[Some(&unsure)], 2), [Candidate(1)]);
    }

    #[test]
    fn min_cost_assignment_is_optimal() {
        // Brute force every injective row -> column mapping.
        fn best(cost: &[Vec<f64>], row: usize, used: &mut Vec<bool>) -> f64 {
            if row == cost.len() {
                return 0.0;
            }
            let mut min = f64::INFINITY;
            for j in 0..used.len() {
                if !used[j] {
                    used[j] = true;
                    min = min.min(cost[row][j] + best(cost, row + 1, used));
                    used[j] = false;
                }
            }
            min
        }
        let mut seed = 7u64;
        let mut rand = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as f64 / (1u64 << 31) as f64
        };
        for (n, m) in [(1, 1), (3, 3), (4, 6), (5, 5), (2, 7)] {
            let cost: Vec<Vec<f64>> = (0..n).map(|_| (0..m).map(|_| rand()).collect()).collect();
            let cols = min_cost_assignment(&cost);
            let mut seen = cols.clone();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), n, "columns must be distinct");
            let total: f64 = cols.iter().enumerate().map(|(i, &j)| cost[i][j]).sum();
            assert!((total - best(&cost, 0, &mut vec![false; m])).abs() < 1e-9);
        }
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
