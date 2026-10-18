# Jev (TypeSafe) on OpenRouter: faster topic labels, better search ranking

Status: A-i (`:topics` via Jev) and B (`xplorertui rank`) implemented, plus a
per-tweet `xplorertui classify`, in `src/openrouter/decisions.rs`. See §8 for
live measurements, which changed parts of this design. §10 adds photos (in
the thread view, in Jev cluster requests and in embeddings), larger clusters,
and the move to Jev 1.14. Sources checked 2026-09-27: OpenRouter Jev hub, Jev
tutorial, Decisions API reference, the "Classify and Tag Text at Scale"
cookbook, and the TypeSafe `Choice` and `State` docs; for §10, 2026-10-18:
TypeSafe's Images page, OpenRouter's Multimodal Decisions guide and
Embeddings reference, and the `ratatui-image` README.

## 1. What Jev is (and isn't)

`typesafe/jev-1.14` (the default since §10; 1.13 before) is a *decision*
model, not a generator. You send a `state` (string, JSON object or array; 262k
tokens on OpenRouter, 32k when this was written) and a map of typed
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
  accurate. On 134 real posts in six languages (§10), 1.14 was as accurate as
  1.13 and better calibrated, but Korean and Chinese still trail German and
  Spanish.
- **It reads images** (both 1.13 and 1.14): PNG or JPEG parts in the `state`.
  See §10 for the limits.

## 2. What xplorertui does today

```
:cluster
  tweets ──embed (MLX Qwen3-Embedding / OpenRouter)──► vectors
         ──k-means (k = 5, fixed; §10: √(n/2) in 4..10)──► labels
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
    "topic":    { "type": "choice", "instructions": "Which topic do most of these posts share?", "criteria": { /* taxonomy */ } } } }
```

That's k requests in parallel, one round trip, and no output parsing. If
the choice is `other` or `topic.confidence < 0.5`, show "Mixed". (An earlier
draft also asked a `coherent` noul. It turned out to be badly calibrated, see §8,
so it was dropped.) The second-best option in `probabilities` is a
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
  collapse toward 0, so use `score` per post for a full ordering. (Untested; §8
  suggests per-item addressing inside one large state is unreliable.)

## 5. When embeddings are still needed

| Need | Embeddings | Jev | Why |
|---|---|---|---|
| Discovering topics nobody named | ✅ | ❌ | Jev only picks from given options |
| 2D map / scatter plot | ✅ | ~ | needs a vector space (see idea below) |
| Retrieval over large or cached corpora | ✅ | ❌ | a query is O(1) against cached vectors; Jev is O(N) per query and judgments are query-specific |
| Near-duplicate / "more like this" | ✅ | ❌ | pairwise; Jev would be O(N²) |
| Non-English / CJK tweets | ✅ (Qwen3 multilingual) | ⚠️ | Jev is English-first; 1.14 narrows the gap (§10); route on the `lang` field we already fetch |
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

## 7. Open questions (answered in §8)

1. Is `state` billed once per request regardless of question count? The
   cookbook numbers (621 tokens with 7 questions) suggest yes.
2. How reliably can multiple questions address different elements of one
   array `state` (for example `posts[3]`)? If this works, it would allow
   batching many tweets per request.
3. Latency distribution per request from the TUI's region, to decide K and
   concurrency.
4. Accuracy on the user's actual timelines, measured with a 100–150 item
   hand-labelled sample and the cookbook's threshold sweep.

## 8. Live measurements (2026-09-27, `typesafe/jev-1.13-20260917`)

Probe scripts ran against the live API through OpenRouter. The numbers are
small samples: treat them as direction, not benchmarks.

**Q1. Is `state` billed once per request?** Yes. The same post with 1 question
used 398 input tokens; with 7 questions it used 471 (+73) and cost $0.0000167 vs
$0.0000198. Output tokens are reported (67 → 174) but aren't what drives the
cost. Extra questions are close to free, so asking tags alongside the topic
is the right call.

**Q2. Can one array `state` hold many posts, with one question per
`posts[i]`?** Only for very small batches. With a 7-topic `choice` per post:

| posts in one state | accuracy | same text labelled differently | input tokens |
|---|---|---|---|
| 8 | 8/8 | 0 | 1,492 |
| 22 | 19/22 | 0 | 3,645 |
| 44 | 28/44 | 16 of 22 texts | 7,036 |
| 88 | 33/88 | 21 of 22 texts | 13,818 |
| 1 per request (22 requests) | 22/22 | – | ~400 each |

Addressing breaks down past roughly 10–20 items: questions start answering
about the wrong element. Batching 8 saves about half the tokens, but the
cost is already negligible, so **the implementation sends one post per
request**. A cluster request (several posts, one question about all of them)
is a different shape and works well.

**Q3. Latency.** 32 single-post requests at concurrency 8: 2.8 s wall time,
p50 0.51 s, p90 0.90 s, max 0.96 s, no errors or 429s. One request is 0.3–0.7
s. Three cluster requests in parallel took 0.9 s end to end, including the
TLS handshake, from the Rust client.

**Q4. Accuracy on real timelines.** Still open. It needs X data and a
hand-labelled sample.

**Other findings**

- The `score` response also includes a `legend` (index → criteria text), and
  its `probabilities` are keyed by index (`"0"`…`"3"`), not by criteria text.
- The `coherent` noul is poorly calibrated for cluster labelling. A clean
  4-post sports cluster scored 0.26, an AI cluster 0.71, and a deliberately
  mixed one 0.02. The `choice` itself was more useful: the mixed cluster
  picked `other` at 0.84, and both clean clusters had confidence 1.0.
- Ranking sanity check ("rust async runtimes", 6 posts): a runtime comparison
  scored 2.54, a question about async-std 2.06, and a Tokio release note 2.05.
  A "BUY NOW Rust course" post scored 0.14 after weighting by
  `substantive` = 0.03. Off-topic posts scored 0.0.
- Cost: `classify` came to about $0.00003 per tweet (topic + 3 tags), and `rank`
  to about $0.000016 per tweet.

**Implemented surface**

- `OpenRouterClient::decide` retries 429/5xx and the in-flight-budget 402
  (honouring `Retry-After`). `decide_all` keeps results in input order and
  runs at bounded concurrency.
- `[jev]` in `config.toml` (`model`, `concurrency`, `topics`,
  `propose_labels`). When it's present and OpenRouter is authenticated,
  `:topics` (and the auto-label after `:cluster`) uses Jev. With a chat model
  available it routes by a per-cluster tech `noul`: A3 on mostly tech
  timelines, chat-written labels otherwise (see §9). Without one, it picks
  from the taxonomy.
- `xplorertui classify` / `xplorertui rank <query>` read tweet JSONL on stdin, so
  they compose with `home` / `search` and need only OpenRouter credentials.
- Not yet done: rank in the TUI search view (`:rank jev|embed`), `:classify`
  view (A-ii), `context_annotations` (A2, needs X API), and cost in the status
  bar.

## 9. Real-timeline evaluation (2026-10-10)

This answers Q4 and explains why taxonomy labels came out generic and
duplicated. Scripts and summary results are in `tools/jev-eval/`.

**Setup.** 224 recent English posts from X search: 144 "tech-heavy timeline"
posts from 9 query buckets (local inference, model releases, chips and
datacenters, AI policy, Rust, terminal tools, coding agents, startup funding,
crypto), and 80 "broad" posts from 8 non-tech buckets. Each corpus was
shuffled to stand in for a timeline, embedded locally with
`Qwen/Qwen3-Embedding-0.6B` and `google/embeddinggemma-2`, and clustered with
k-means (k = 5 and 8, 3 seeds each, so 6 runs per cell). Clusters were only
about 60% one query bucket: real posts mix.

**Metrics.** *dup* = how many labels repeat within a run, not counting
"Mixed". *mixed* = clusters labelled "Mixed" per run. *dist* = for each
cluster with posts the labeller didn't see (labellers get a cluster's first
8 posts, so only clusters with more than 8 count) and a label other than
"Mixed", whether a matcher (Jev and Luna, averaged) shown those held-out
posts picks the cluster's own label out of all k labels. Each cell scores
about 18–28 clusters (tech k=8: about 40), so differences under about 0.1
are noise.

| Labeller | Tech k=5 dup / mixed / dist | Tech k=8 | Broad k=5 | Broad k=8 |
|---|---|---|---|---|
| Jev, default taxonomy (old behaviour) | 1.0 / 0.5 / 0.42 | 3.5 / 0.2 / 0.48 | 0.2 / 1.0 / 0.28 | 0.7 / 1.7 / 0.35 |
| Luna, default taxonomy | 0.8 / 0.2 / 0.56 | 3.3 / 0 / 0.49 | 0.2 / 1.3 / 0.31 | 0.6 / 1.4 / 0.34 |
| Qwen3.5-9B writes labels (reasoning off) | 0 / 0 / 0.92 | 0 / 0 / 0.77 | 0 / 0 / 0.63 | 0 / 0 / 0.67 |
| Jev, 30-topic taxonomy | 0 / 0.2 / 0.83 | 0 / 0.3 / 0.66 | 0.2 / 1.5 / 0.55 | 0.3 / 1.8 / 0.46 |
| Jev, two-level taxonomy (re-ask colliding parents) | 0 / 0 / 0.78 | 0.5 / 0 / 0.75 | 0.2 / 0 / 0.59 | 1.5 / 0 / 0.55 |
| Qwen3.5-9B proposes, Jev picks (A3) | 0 / 0 / 0.93 | 0 / 0 / 0.81 | 0 / 0.8 / 0.50 | 0 / 0.8 / 0.52 |
| **Routed: Jev tech check, then A3 or chat labels (as shipped)** | 0 / 0 / 0.93 | 0 / 0 / 0.81 | 0 / 0 / 0.63 | 0 / 0 / 0.67 |
| Qwen3.5-9B proposes, Luna picks | 0 / 0 / 0.93 | 0 / 0 / 0.77 | 0 / 0.8 / 0.54 | 0 / 0.3 / 0.58 |

An earlier version of this table scored clusters of 8 or fewer posts on the
same posts the labeller saw, which inflated every *dist*, most on the broad
corpus (where clusters are smaller).

**Why the taxonomy collides.** Jev can only choose from the options it's
given. A tech-heavy timeline has many real subtopics, but they fall into
about four of the 20 default topics (AI, programming, business, crypto), so
with k = 8 some clusters must share a name. Asking for "the most specific
topic" doesn't change that, and forcing unique labels on the coarse list
gives unique but wrong labels. A finer taxonomy fixes tech timelines but not
broad ones: real broad posts split into, for example, three politics
clusters, and no fixed list anticipates that. Labels proposed from the
clusters' own posts never collide, so A3 is now the default whenever a chat
model is available. Proposing from the whole corpus instead of cluster
samples gave labels that were too niche.

**Where A3 stands.** On tech-heavy timelines (this app's main use), A3
matches or slightly beats labels the chat model writes itself (0.93 / 0.81
vs 0.92 / 0.77) and is far ahead of the taxonomy. On broad timelines it
still beats the taxonomy, but letting the chat model write the labels
directly scores higher (0.63–0.67 vs 0.50–0.52). Part of the gap is that
A3 marks about one junk cluster per run "Mixed", while the chat model names
every cluster, junk included.

**Routing.** So `:topics` now asks Jev one `noul` per cluster first: "Are
most of these posts about technology: software, AI, computing hardware,
crypto, science or the tech industry?" If at least half the posts sit in
clusters with P ≥ 0.5, it runs A3; otherwise the chat model writes the
labels. On the eval corpora the split is clean: the tech share was 1.0 in
every tech-corpus run and 0–0.16 (mean 0.06) on the broad corpus, so the
routed setup took the expected branch in all 24 runs and matches the
better of the two in every cell. It costs one extra round of k concurrent
Jev requests (about 0.3 s). If the tech check fails entirely, the chat
model writes the labels.

In 14 of the 156 propose-then-pick assignments in the original sweep, Jev
put at least 0.5 on "None of these". All 14 were junk or mixed clusters
(ticket resale, commission spam, adult content, unrelated posts), and
forcing a candidate on them gave labels like "Election Strategy & Voting".
Those clusters now show "Mixed" and take no candidate, the same 0.5 cut-off
the taxonomy path uses. A proposal is accepted only if every cluster got at
least one candidate and the pool has one candidate per cluster; otherwise
the taxonomy is used.

The proposal prompt asks for the last of the three labels to fit every
tweet in the cluster. Without that, Qwen sometimes proposed only narrow
labels ("Neovim dotfiles" for a cluster about Neovim, Ghostty and zsh) and
Jev rejected them all, turning a clean cluster "Mixed": 4 of 45 clusters in
a repeated live test, against 0 of 45 with the requirement. The table above
uses the final prompt.

**Per-post topic accuracy** (30-topic taxonomy, one post per request,
reference labels from Claude Sonnet 5.5):

| | Jev 1.13 | Luna (`openai/gpt-6-luna-decisions`) |
|---|---|---|
| Accuracy | 83.5% | 80.4% |
| Accuracy at confidence ≥ 0.8 | 92.9% (169 posts) | 90.9% (164 posts) |
| Same top-level topic as reference | 92% | 89% |
| p50 / p90 latency | 0.27 / 0.34 s | 0.26 / 0.31 s |
| Cost per 1k posts | $0.045 | $0 as billed during the test |

Most misses are between neighbouring subtopics (coding agents vs terminal
tools, AI products vs model releases). Luna is served by the same
`/api/alpha/decisions` endpoint and request format, so it can be used by
setting `[jev] model`. It refused 3 questions during the sweep (spam or adult
posts), returned as 502 "refused to answer"; those are no longer retried.

**Fixes made alongside.**

- Chat labelling used `reasoning: { exclude: true }` with a 32–256 token
  budget. Hidden reasoning still counts against `max_tokens`, so reasoning
  models such as Qwen3.5-9B used the whole budget and never produced labels.
  Chat requests now send `reasoning: { enabled: false }`; models that reject
  that (Sonnet 5.5 returns 400 "Reasoning is mandatory") are retried with
  low-effort hidden reasoning and 2,048 extra tokens.
- The MLX server takes no reasoning setting; it passes
  `enable_thinking=False` to the chat template on every request. That is a
  template variable, so a model whose template does not read it thinks
  anyway and used to spend the label budget on stripped thinking. On the
  server, `max_tokens` now caps the answer only: thinking gets up to 2,048
  extra tokens in the same run, so no second request is needed. The server
  also reports the real finish reason and
  `usage.completion_tokens_details.reasoning_tokens`.
- `cluster_label` still shortens descriptions at the first comma or
  parenthesis, so custom `topics` with long descriptions get clipped labels.
  A3 sidesteps this; a display-name field would fix it for taxonomies.
- Representative posts are still the first 8 in timeline order, not the
  ones closest to the centroid.

## 10. Photos, larger clusters, and Jev 1.14 (2026-10-18)

Scripts and results: `tools/jev-eval/exp_images.py` and
`results_images.json`. The posts came from `xplorertui search` with keyword
queries per language, so their topics are easier than a real timeline's.
Treat small differences as noise.

### What the APIs accept

- **Jev reads images through OpenRouter without a header.** TypeSafe's own
  API needs `TypeSafe-Danger-Zone-Images: true`; OpenRouter answered the
  same with and without it, for 1.13 and 1.14.
- **Image part:** `{"type": "image_url", "image_url": {"url":
  "data:image/jpeg;base64,…"}}`. Remote URLs are not fetched. Jev reads
  image parts at any depth of the `state`, so a post can be
  `{"text": …, "image": <part>}` inside the existing `posts` array.
- **Limits:** PNG or JPEG only (no WebP, no GIF, no animated PNG);
  280–1568 px a side (OpenRouter says 256; TypeSafe says 280, so we use 280);
  78,400–2,458,624 px in total; 16 MiB a file. Images outside the limits are
  rejected, never resized. EXIF orientation is not applied.
- **X photo sizes:** `?format=jpg&name=small` gives 680 px on the long side,
  inside the range for every photo we saw. `name=thumb` (150 px) is under
  the minimum. The app always asks for `small` and pads a short side under
  280 px with black instead of upscaling.

### Cost and time

| Photo size sent | Input tokens (photo only) | Jev latency |
|---|---|---|
| `small`, 680 px | 1,383–1,692 | 0.44–0.56 s |
| `medium`, 1200 px | 2,844–3,788 | 0.47–0.80 s |
| 1568 px | 4,274–4,835 | 0.48–0.83 s |

A request without a photo used about 600 tokens, so a `small` photo adds
about 800, at the normal input price. One cluster request with 8 posts took
0.48 s with no photos, 0.60 s with 2, 0.81 s with 4 and 1.06 s with 8 (1,357
→ 7,727 input tokens). Downloads were the slow part: 45 of 53 X photos
arrived in under 0.1 s, but 8 took 10–12 s.

### Accuracy

Per-post topic over the 20-topic taxonomy plus `adult`, against Claude
Sonnet 5.5 reference labels (the reference saw the photo too):

| | Jev 1.13 | Jev 1.14 |
|---|---|---|
| All 134 posts | 0.87 | 0.87 |
| At confidence ≥ 0.8 | 0.92 (105 posts) | 0.96 (102 posts) |
| German / Spanish / Japanese | 0.95 / 0.95 / 0.94 | 0.95 / 0.95 / 0.94 |
| Korean / Chinese | 0.75 / 0.75 | 0.80 / 0.75 |
| Korean / Chinese at confidence ≥ 0.8 | 0.81 / 0.87 | 1.00 / 0.92 |
| p50 / p90 latency | 0.43 / 0.93 s | 0.42 / 0.48 s |

On the 53 posts with a photo:

| | Jev 1.13 | Jev 1.14 |
|---|---|---|
| Text only | 0.89 | 0.87 |
| Text and photo | 0.89 (0.95 at ≥ 0.8) | 0.85 (1.00 at ≥ 0.8) |
| Photo only | 0.72 | 0.72 |

The photo did not make the topic more accurate, even on the 15 posts with
under 60 characters of text: the text alone got 14 of them right. It did
make high-confidence answers more reliable. A photo alone is a weak but
usable signal (0.72), which matters for posts with almost no text.

### What the app does with this

- **Default model:** `typesafe/jev-1.14`, pinned. Same accuracy, better
  calibration, shorter tail latency. `[jev] model` in `config.toml`
  overrides it.
- **Posts:** `:cluster` loads more pages until it has `cluster_posts` (100)
  posts; one X API read returns 100 at most. A search is asked again for
  100 results. Each page's includes are merged, not replaced, and media go
  into an app-wide cache keyed by media key.
- **Clusters:** k = √(n/2), from 4 to 10 (20 posts → 4, 100 → 7, 200 → 10),
  or `cluster_k`. Ten is the number of cluster colours. The 8 posts a
  cluster request carries are now the 8 nearest the centroid, not the first
  8 in timeline order.
- **Photos to Jev:** only in the pick round of propose-then-pick, and only
  for posts with under 60 characters of text once links, mentions and
  hashtags are removed. At most 2 a cluster and 24 a run; clusters take
  turns. Downloads start with the tech check, 8 at a time, and a photo that
  takes over 3 s is skipped. At k = 10 that is 20 photos, about 16k extra
  input tokens (under $0.001) and about 0.1 s more per request.
- **Photo embeddings:** a cluster where more than 80% of the posts are
  photos with thin text grouped because the posts have no text, not
  because they show the same thing. Those posts get one vector of text and
  photo, and k-means runs again on all posts with the same k. Text and
  photo vectors must come from the same model to be comparable, so the
  photo vector comes from the model that embedded the text:
  `embeddinggemma-2` on the MLX server, `google/gemini-embedding-2` on
  OpenRouter. A text-only model (Qwen3-Embedding, or an OpenRouter model
  whose `input_modalities` lack `image`) skips the step, and the status
  line says why.
  - The MLX server's `/v1/embeddings/multimodal` returns the text and the
    image as separate vectors, so a new `/v1/embeddings/documents` puts the
    image tokens inside the text sequence and returns one vector per post.
    Checked on `mlx-community/embeddinggemma-2-mxfp8`: a text-only document
    gives the same vector as `/v1/embeddings` (cosine 1.0); a post with its
    photo stays near its own text (0.90) and away from an unrelated post
    (0.71). It loads a second copy of the model with its vision tower on
    first use; about 1 s for 3 documents on an M-series Mac.
  - OpenRouter: one input item `{"content": [text part, image part]}` gives
    one 3,072-dimension vector; a 680 px photo was 258 image tokens
    ($0.00012). Cosine to its own text 0.72, to an unrelated post 0.31.
- **Thread view:** the focused post's photo (the selected reply's if it has
  one, else the root's) in a 48 × 14-cell box, `[` / `]` to page through
  photos. `ratatui-image` 11.1 draws it with the terminal's protocol: Kitty
  (Kitty, Ghostty), iTerm2 (iTerm2, WezTerm, Rio) or Sixel (Xterm with
  `-ti 340`, Foot, mlterm). Terminal.app, Alacritty, Konsole and Warp get
  half-block characters. The protocol query runs before `App::new`, whose
  event handler would otherwise read the terminal's reply. Decoding and
  encoding run on a blocking task; the UI draws the cached result.

### Not done

- Box questions (bounding boxes, 1.14 only).
- Photos in the tech check, in chat-written labels, or in `classify` /
  `rank`.
- Video and GIF frames.

