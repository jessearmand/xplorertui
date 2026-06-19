---
name: xplorertui-topic-research
description: Research any topic on X (Twitter) using the installed xplorertui CLI (`cargo install`), then answer the user's question with evidence from recent posts. Use when the user asks what is happening on X about a subject, wants public discourse on a news topic, needs sentiment or reactions from Anthropic/government/developers, or phrases like "what are people saying about", "search X for", "what's the conversation on Twitter about", "research this topic on X". Composes multiple `xplorertui search` queries, deduplicates JSONL, and synthesizes a cited answer. Use when the user runs /xplorertui-topic-research.
---

# xplorertui-topic-research

Search recent X discourse for any topic using the system-installed `xplorertui` binary, then answer the user's specific question from the evidence gathered.

## What this skill does

1. **Parse** the user's natural-language question into concrete sub-questions and 3–6 targeted X search queries.
2. **Fetch** recent tweets via `xplorertui search` (last ~7 days, bearer auth OK).
3. **Merge** results, deduplicate by tweet ID, sort newest-first.
4. **Synthesize** a structured answer that directly addresses every part of the user's question, citing authors and dates from the tweets.

Each tweet line is denormalized JSONL:

```json
{"tweet": {...}, "author": {...}, "media": [...]}
```

## When to use this skill

Trigger on research-style questions about X discourse:

- "What has been happening on X regarding [topic]?"
- "What are people saying about [model/event/policy]?"
- "Is [product] still closed to the public — what does X think?"
- "How are [company], the public, and the government responding to [topic] on X?"
- "Search X for [keywords] and summarize"
- "What's the conversation on Twitter about [subject]?"

Do **not** trigger for:

- A specific user's timeline → use `xplorertui-user-posts` (`search "from:USERNAME"`).
- Thread context around your mentions → use `xplorertui-mention-thread`.
- Expanding one tweet's replies → use `xplorertui open <id>`.
- Non-X research (web, news sites) → use firecrawl or other web tools.

## Prerequisites

- `xplorertui` on PATH (`cargo install xplorertui` or `cargo install --path .` in this repo).
- `jq` on PATH (used by the wrapper script for deduplication).
- Valid credentials in `~/.config/xplorertui/.env`, `~/.config/x-cli/.env`, or `./.env` (`X_BEARER_TOKEN` is sufficient for `search`).

## Step 1 — Decompose the user's question

Before searching, extract:

| Element | Example |
|---------|---------|
| **Core topic** | "Claude Fable 5" |
| **Spelling variants** | "Clade Fable 5" → also search "Claude Fable 5" |
| **Sub-questions** | availability, leaks, government response, public reaction |
| **Stakeholder queries** | `from:AnthropicAI`, `from:claudeai`, journalist handles if known |
| **Angle queries** | `"closed beta"`, `leak`, `regulation`, `government` combined with topic |

Write the sub-questions down — the final answer must address each one explicitly.

## Step 2 — Run searches

### Preferred: bundled wrapper script

```bash
SKILL_DIR="<repo>/.grok/skills/xplorertui-topic-research"
"$SKILL_DIR/scripts/topic-search.sh" -n 20 \
  "Claude Fable 5" \
  "from:AnthropicAI Fable" \
  "Claude Fable leak" \
  "Claude Fable closed" \
  > /tmp/topic-research.jsonl
```

Flags:

| Flag | Meaning | Default |
|------|---------|---------|
| `-n N` | Max tweets per query (1–100) | all returned by config |
| `-q QUERY` | Add a query (repeatable) | — |
| positional args | Queries when `-q` not used | — |

### Fallback: inline searches

When the script is unavailable, run queries individually:

```bash
xplorertui search "Claude Fable 5" 2>/dev/null | head -20
```

Save output to a file when iterating — the X API free tier is rate-limited.

### Query crafting tips

- Use quotes for exact phrases: `"Claude Fable 5"`
- Combine topic + angle: `Claude Fable government`, `Anthropic Fable regulation`
- Scope to accounts: `from:AnthropicAI`, `from:claudeai`
- Exclude noise: `-is:retweet` (X search operators work in xplorertui queries)
- Run 3–6 complementary queries; one query rarely captures the full discourse

## Step 3 — Inspect the JSONL

Useful `jq` recipes on the saved file:

```bash
# Chronological summary (newest first already from script)
jq -r '"\(.tweet.created_at) @\(.author.username // "unknown"): \(.tweet.text)"' topic.jsonl

# Count unique authors
jq -s '[.[].author.username] | unique | length' topic.jsonl

# Filter to original posts (skip RTs)
jq 'select(.tweet.text | startswith("RT ") | not)' topic.jsonl

# Posts mentioning a stakeholder
jq 'select(.tweet.text | test("Anthropic|government|regulat"; "i"))' topic.jsonl

# Top engagement
jq -s 'sort_by(-(.tweet.public_metrics.like_count // 0)) | .[0:5][]' topic.jsonl
```

If results are thin, broaden queries (drop quotes, try synonyms) or note the 7-day API window limitation.

## Step 4 — Synthesize the answer

Structure the response for the user:

```markdown
## Summary
One-paragraph direct answer to the user's core question.

## What people are saying on X
Bullet points grouped by theme (availability, leaks, capabilities, reactions).

## Stakeholder angles
- **Anthropic/official**: ...
- **Public/developers**: ...
- **Government/policy**: ... (or "no substantive government discourse found in recent search")

## Notable posts
2–5 representative tweets with @handle, date, and brief quote.

## Limitations
- Recent search only (~7 days via X API v2)
- Sample size and rate limits
- Distinguish confirmed news vs rumor/speculation
```

Rules:

- **Answer the actual question** — don't just dump tweets.
- **Cite evidence** — attribute claims to @handles and dates.
- **Separate fact from rumor** — leaked prompts, unverified claims, and satire are common on X.
- **Correct typos** — if the user wrote "Clade Fable 5", search "Claude Fable 5" but note the correction.
- **Say when evidence is missing** — if government response isn't in the results, say so plainly.

## Authentication & limits

- `search` uses bearer or any valid credential set; OAuth not required.
- Only the last ~7 days of tweets are searchable on the standard recent-search endpoint.
- `default_max_results` in `~/.config/xplorertui/config.toml` controls per-query volume (default 20, max 100).
- Re-run sparingly; save JSONL to avoid duplicate API calls.

## Relationship to other xplorertui skills

| Skill | Use when |
|-------|----------|
| `xplorertui-topic-research` | Broad topic research + answer a multi-part question |
| `xplorertui-user-posts` | One user's recent posts |
| `xplorertui-mention-thread` | Conversation around your mentions |
| `xplorertui similar` | Semantic re-ranking (needs OpenRouter) |

## Quick example

User asks: *"What's happening on X about Claude Fable 5 — is it still closed, and how are Anthropic, the public, and government responding?"*

```bash
./.grok/skills/xplorertui-topic-research/scripts/topic-search.sh -n 20 \
  "Claude Fable 5" \
  "from:claudeai Fable" \
  "Claude Fable closed" \
  "Claude Fable leak" \
  "Claude Fable government" \
  > /tmp/fable5.jsonl
```

Then read the JSONL, group by theme, and write the structured answer from Step 4.