# Jev (TypeSafe) on OpenRouter: faster topic labels, better search ranking

Status: design note / not yet implemented. Sources checked 2026-09-27:
OpenRouter Jev hub, Jev tutorial, Decisions API reference, the
"Classify and Tag Text at Scale" cookbook, and the TypeSafe `Choice` and
`State` docs.

## 1. What Jev is (and isn't)

`typesafe/jev-1.13` is a *decision* model, not a generator. You send a `state`
(string, JSON object or array; 32k-token context) and a map of typed
`questions`, and you get back probabilities:

| Primitive | Answers | Returns |
|---|---|---|
| `choice` | which one of N options (up to 255) | `choice`, `probabilities{}`, `confidence` |
| `noul`   | does this hold? | `noul` = P(yes) |
| `score`  | where on an ordered scale? | `score` (probability-weighted index), `probabilities{}`, `confidence` |

- Endpoint: `POST https://openrouter.ai/api/alpha/decisions` (note: `/api/alpha`,
  not `/api/v1`, so it doesn't fit `OpenRouterClient`'s `BASE_URL` as is).
- Pricing: input tokens only, $0.042 / M at time of writing; output is free.
- Published throughput: 400 requests of ~610 tokens in 8.6 s at concurrency 8,
  so about 170 ms per request per worker. Cost was $0.0256 per 1,000 tweets.
- All questions in one request run in parallel and can't see each other, so
  adding questions barely changes latency.
- **It cannot write a label.** It only picks among options you supply.
- **It's English-first.** The docs say other languages, CJK especially, are less
  accurate.

## 2. What xplorertui does today

```
:cluster
  tweets ──embed (MLX Qwen3-Embedding / OpenRouter)──► vectors
         ──k-means (k = 5, fixed)──► labels
         ──PCA 2D──► scatter plot (ui/cluster.rs)
         ──closest-to-centroid tweet──► placeholder topic
  :topics (optional)
         ──chat LLM, "Cluster <i>: <label>" free text──► parse_cluster_topic_labels

search rerank
  X search results ──embed(query + tweets)──► cosine ──► sorted list
```

Where the time goes:

1. **The chat-LLM labelling step** (`dispatch_generate_cluster_topics`) is the
   slow, fragile part. It generates output tokens, can burn time on reasoning,
   needs `max_tokens` tuning, and relies on a line parser that has failure
   modes (the `labels.iter().all(|l| l.is_empty())` guard).
2. Embedding is cheap, especially locally on MLX, and k-means/PCA over about
   100 vectors takes milliseconds. Jev will **not** beat local embedding plus
   k-means on raw speed. Its gains come from replacing the generative step, and
   from better *judgment* quality in ranking.
3. Cosine over bi-encoder embeddings is a weak relevance signal. The query and
   the tweet are encoded separately, so there's no interaction between them.

## 3. Proposal A: topic labelling with Jev

You're right that Jev needs a label set. There are three ways to supply one,
and they can be layered:

### A1. Static taxonomy (no generator at all)

A curated `choice` over about 20–60 topics, configurable in `config.toml`:

```toml
[jev]
model = "typesafe/jev-1.13"      # pin; thresholds are version-specific
topics = { ai_ml = "AI, ML, LLMs, model releases", rust = "Rust language and ecosystem", ... , other = "None of the above" }
```

A good seed is the 19-topic TweetTopic taxonomy (cardiffnlp/tweet_topic_multi).
It's the dataset OpenRouter's own cookbook benchmarked Jev on (127/150 category
matches, ≥ 0.8 confidence → 114/122 correct). Always include an `other` option.

### A2. Taxonomy from the data we already fetch (free labels)

The X API v2 tweet field `context_annotations` returns X's own domain/entity
annotations, such as "Technology / OpenAI" or "Sports / NBA". We don't request
it today (`tweet_fields()` in `src/api/mod.rs`). Adding it gives a per-timeline
candidate label set at zero model cost. Hashtags from `entities` are a second
source. Take the top-N entity names, add the static taxonomy plus `other`, and
use that as the `choice` criteria.

### A3. Small generator proposes, Jev disposes

For open-ended discovery, call a small chat model **once per corpus**, not once
per cluster, to propose 15–30 candidate labels from a sample. Jev then does
every assignment. The generator's output shrinks to a short list, and all
per-item work moves to the cheap, typed, parse-free path. The existing MLX
Qwen3.5-0.8B is fine for this.

### Two pipeline shapes

**A-i. Cluster-then-label (drop-in replacement for `:topics`).**
Keep embeddings and k-means. Per cluster, send one Jev request:

```jsonc
{ "model": "typesafe/jev-1.13",
  "state": { "posts": ["…up to 8 representative tweets…"] },
  "questions": {
    "topic":    { "type": "choice", "instructions": "Which topic do most of these posts share?", "criteria": { /* taxonomy */ } },
    "coherent": { "type": "noul",   "instructions": "Do these posts share one clear topic?" } } }
```

That's k requests in parallel, one round trip, and no output parsing. If
`coherent < 0.5` or `topic.confidence` is low, show "mixed" or fall back to the
chat LLM for that cluster only. The second-best option in `probabilities` is a
free secondary label (for example "AI · Business").

**A-ii. Classify-then-group (replaces k-means for the topic view).**
Send one Jev `choice` per tweet. The groups *are* the topics, so clustering and
labelling become one step, and the number of groups follows from the data
instead of a fixed `k = 5`. For 100 tweets that's about $0.0026 and ~2 s at
concurrency 8 (less with more workers). Add `noul` tags alongside, such as
`is_question`, `is_promo`, `is_news`, and `score` sentiment, for no extra
latency.

Recommendation: ship **A-i** first. It's a surgical swap in `dispatch.rs`.
Then add **A-ii** as a `:classify` view once a taxonomy exists.

## 4. Proposal B: search ranking with Jev

Jev behaves like a cross-encoder reranker. It sees the query and the post
together.

```
X search (≤100) ──[optional embed prefilter → top K≈30]──► Jev score per post ──► rank
```

Per candidate:

```jsonc
{ "state": { "query": "rust async runtimes", "post": "…", "author": "@…" },
  "questions": {
    "relevance": { "type": "score", "instructions": "How well does the post address the search intent?",
                   "criteria": ["Unrelated", "Mentions the topic", "About the topic", "Directly answers / highly informative"] },
    "substantive": { "type": "noul", "instructions": "Does the post contain substantive information rather than promotion, spam, or a bare link?" } } }
```

Rank key: `relevance.score` (0–3, probability-weighted, so ties are rare),
multiplied by `substantive`. Break remaining ties with cosine when embeddings
are available. With K = 30 at concurrency 8–16 this is about 2–4 rounds, well
under a second or two, at about $0.001 per query.

This also opens up search features that embeddings can't do:

- **Facet filters** as `noul`s: "is a question", "contains a link to a paper",
  "first-hand experience", "announcement". Toggle them in the TUI and filter
  client-side on stored probabilities, with no re-query.
- **Intent routing**: one `choice` on the query itself (news / how-to / opinion
  / person lookup) picks the ranking recipe.
- **Top-1 "best answer"**: put ≤ 255 candidates in one `state` and ask a
  `choice` over their ids. This works for top-1, but the tail probabilities
  collapse toward 0, so use `score` per post for a full ordering.

## 5. When embeddings are still needed

| Need | Embeddings | Jev | Why |
|---|---|---|---|
| Discovering topics nobody named | ✅ | ❌ | Jev only picks from given options |
| 2D map / scatter plot | ✅ | ~ | needs a vector space (see idea below) |
| Retrieval over large or cached corpora | ✅ | ❌ | a query is O(1) against cached vectors; Jev is O(N) per query and judgments are query-specific |
| Near-duplicate / "more like this" | ✅ | ❌ | pairwise; Jev would be O(N²) |
| Non-English / CJK tweets | ✅ (Qwen3 multilingual) | ⚠️ | Jev is English-first; route on the `lang` field we already fetch |
| Offline / local / private | ✅ (MLX) | ❌ | Jev is cloud-only (though cheap) |
| Labelling clusters | ⚠️ (centroid tweet) | ✅ | typed, no parsing, ~one round |
| Precise relevance of top-K | ⚠️ (bi-encoder) | ✅ | cross-attention judgment + calibrated probs |
| Tags / filters / sentiment | ❌ | ✅ | many questions per request, same latency |

Rule of thumb: **embeddings for recall and geometry, Jev for judgment.**
Embeddings are needed when the operation is unsupervised, pairwise, spatial,
cached across queries, multilingual, or offline. Jev takes over as soon as
there's a named decision to make about a bounded set of items.

Speculative idea (untested): a tweet's `choice` probability vector over the L
taxonomy labels is itself an L-dimensional, *interpretable* embedding. PCA or
k-means over it would give a map whose axes have names. It's only as fine as
the taxonomy, so it's a complement to Qwen embeddings, not a replacement.

## 6. Implementation sketch (Rust)

New module `src/openrouter/decisions.rs`:

```rust
#[derive(Serialize)]
pub struct DecisionRequest<'a> {
    pub model: &'a str,
    pub state: serde_json::Value,
    pub questions: BTreeMap<String, Question>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice { instructions: String, criteria: IndexMap<String, Option<String>> },
    Noul   { instructions: String, #[serde(skip_serializing_if = "Option::is_none")] criteria: Option<NoulCriteria> },
    Score  { instructions: String, criteria: Vec<String> },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice { choice: String, #[serde(default)] confidence: Option<f64>, probabilities: HashMap<String, f64> },
    Noul   { noul: f64 },
    Score  { score: f64, #[serde(default)] confidence: Option<f64>, probabilities: HashMap<String, f64> },
}

#[derive(Deserialize)]
pub struct DecisionResponse { pub model: String, pub answers: HashMap<String, Answer>, pub usage: DecisionUsage }
```

- Add `OpenRouterClient::post_url` (or a `DECISIONS_URL` const) because the path
  is under `/api/alpha`, not `/api/v1`.
- Add a bounded-concurrency helper (`futures::stream::iter(..).buffer_unordered(n)`)
  with retry on 429/5xx and on 402 where `limit_source == "openrouter_in_flight_budget"`,
  honouring `Retry-After`.
- New events `JevTopicsLabeled` / `JevRanked`, following the existing
  `ClusterTopicsGenerated` generation-guard pattern so stale results are dropped.
- Commands: `:topics` prefers Jev when `[jev]` is configured and falls back to
  the chat LLM. `:rank jev|embed` switches the search reranker.
- Show `usage.cost` totals in the status bar.
- Pin the model version. Thresholds tuned on 1.13 may shift on `~typesafe/jev-latest`.

## 7. Open questions to verify with a live key

1. Is `state` billed once per request regardless of question count? The
   cookbook numbers (621 tokens with 7 questions) suggest yes.
2. How reliably can multiple questions address different elements of one
   array `state` (for example `posts[3]`)? If this works, it would allow
   batching many tweets per request.
3. Latency distribution per request from the TUI's region, to decide K and
   concurrency.
4. Accuracy on the user's actual timelines, measured with a 100–150 item
   hand-labelled sample and the cookbook's threshold sweep.
