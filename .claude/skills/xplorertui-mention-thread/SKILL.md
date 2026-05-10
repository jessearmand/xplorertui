---
name: xplorertui-mention-thread
description: Fetch the conversation thread surrounding a recent X (Twitter) mention using the xplorertui CLI, emitting JSONL. Use this whenever the user wants to "expand", "see the thread of", "see the conversation around", or "get the context of" a mention from their X account — including phrases like "my latest mention", "newest @mention", "the thread my last mention is in", "what people are saying to me on X", or when they paste a tweet ID/URL and ask for the surrounding conversation. Trigger this even if the user does not explicitly say "xplorertui" — if they reference X/Twitter mentions and want thread context, this is the right skill. The skill composes `xplorertui mentions` and `xplorertui open <id>` into one JSONL stream so downstream tools (jq, grep, an LLM) can read the whole conversation as a single artifact.
---

# xplorertui-mention-thread

Wrap two `xplorertui` CLI commands into a single pipeline that fetches the full
conversation thread for a recent mention.

## What this skill does

Given a position N (default 1 = newest mention) or an explicit tweet ID/URL,
this skill produces a JSONL stream on stdout where:

- **Line 1** is the *queried tweet* — i.e., the mention itself (when invoked by
  default or `-n N`) or whatever tweet was passed via `-i`.
- **Lines 2..k** are every other tweet in the same conversation
  (same `conversation_id`), in the order returned by the X API.

The *conversation root* (the tweet whose `id` equals its own `conversation_id`,
i.e. the original post that started the thread) may appear anywhere in
lines 2..k, not necessarily first. To find it, filter:

```bash
... | jq 'select(.tweet.id == .tweet.conversation_id)'
```

Each line is a self-contained JSON object of shape:

```json
{"tweet": {...}, "author": {...}, "media": [...]}
```

This is the same shape used everywhere else in the xplorertui CLI, so the
output composes naturally with `jq`, `grep`, file writes, or feeding back into
another model.

## When to use this skill

Trigger when the user asks for thread/conversation context around a mention,
in any of these phrasings:

- "Expand my latest mention" / "show me the thread of my newest @mention"
- "What's the conversation around the last person who mentioned me?"
- "Give me the thread context for my third most recent mention"
- "Pull the full thread for tweet 1234..." (when paired with mention/X context)
- "What were people replying to in the thread my mention came from?"

Don't trigger for plain mention listing ("show me my mentions" with no thread
intent — `xplorertui mentions` alone is enough) or for arbitrary tweet expansion
unrelated to mentions (use `xplorertui open` directly).

## The bundled wrapper script

The pipeline lives at `scripts/mention-thread.sh` *next to this SKILL.md*. The
canonical version is checked into the xplorertui repo at
`<repo>/.claude/skills/xplorertui-mention-thread/scripts/mention-thread.sh`,
and the user-level location `~/.claude/skills/xplorertui-mention-thread/` is
typically a symlink pointing at the repo copy so the skill loads from any cwd.

To run the script, prefer in this order:

1. `./.claude/skills/xplorertui-mention-thread/scripts/mention-thread.sh`
   — when the cwd is the xplorertui repo (always works after cloning).
2. `~/.claude/skills/xplorertui-mention-thread/scripts/mention-thread.sh`
   — works from any cwd if the user has the global skill installed
   (typically a symlink to the repo copy).

### Usage

(Examples below show the global-symlink path. Substitute the project-relative
path if you're inside the xplorertui repo and the symlink isn't set up.)

```bash
# Default: newest mention, dump JSONL to stdout
~/.claude/skills/xplorertui-mention-thread/scripts/mention-thread.sh

# Nth most recent mention (1-indexed, 1 = newest)
~/.claude/skills/xplorertui-mention-thread/scripts/mention-thread.sh -n 3

# Bypass mentions entirely — expand a specific tweet ID or URL
~/.claude/skills/xplorertui-mention-thread/scripts/mention-thread.sh -i 1234567890
~/.claude/skills/xplorertui-mention-thread/scripts/mention-thread.sh -i https://x.com/user/status/1234567890

# Save to file
~/.claude/skills/xplorertui-mention-thread/scripts/mention-thread.sh > thread.jsonl
```

### Setting up the symlink on a fresh clone

If you clone this repo on a new machine and want the skill to load globally:

```bash
ln -s "$(pwd)/.claude/skills/xplorertui-mention-thread" \
      ~/.claude/skills/xplorertui-mention-thread
```

### Exit codes

| Code | Meaning |
|------|---------|
| 0    | Success — JSONL written to stdout |
| 1    | Logical failure (no mention at position N, empty mentions list) |
| 2    | Invalid argument (non-numeric N, unknown flag) |
| 127  | `xplorertui` or `jq` not on PATH |

## What the script does internally

The pipeline is small enough that it's worth understanding rather than treating
as a black box:

```bash
# Step 1 — pick the Nth mention's tweet ID
id=$(xplorertui mentions | awk -v n="$N" 'NR==n {print; exit}' | jq -r '.tweet.id')

# Step 2 — expand its conversation thread
xplorertui open "$id"
```

`xplorertui mentions` already returns mentions newest-first as JSONL, so picking
the Nth line gives the Nth-most-recent. `xplorertui open` then resolves the
tweet's `conversation_id` (a field set by the X API on every tweet that points
at the *original* tweet that started the conversation) and prints the root
followed by every reply.

If the user wants to skip the wrapper for any reason — e.g., they want to
inspect the mention list before deciding which one to expand — fall back to
the inline form:

```bash
xplorertui mentions | head -1 | jq -r '.tweet.id' | xargs xplorertui open
```

## Output post-processing recipes

Once the JSONL is on stdout (or saved), a few useful jq one-liners:

```bash
# Author + text, prefixed (chronological order requires explicit sort below)
... | jq -r '"@\(.author.username): \(.tweet.text)"'

# Filter to replies that mention a specific handle
... | jq 'select(.tweet.entities.mentions[]?.username == "alice")'

# Find the actual conversation root (id == conversation_id)
... | jq 'select(.tweet.id == .tweet.conversation_id)'

# Sort the whole thread by created_at (the API order isn't strictly
# chronological — newer replies sometimes appear before older ones)
... | jq -s 'sort_by(.tweet.created_at) | .[]' -c

# Convert the JSONL stream into a single wrapped document
# (queried tweet + the rest of the conversation as one JSON object)
... | jq -s '{queried: .[0], conversation: .[1:]}'
```

The last recipe is the answer if a downstream consumer needs a single JSON
document instead of a stream — the script intentionally emits JSONL by default
because that matches the rest of the xplorertui CLI surface, but `jq -s` upgrades
it to a wrapped document in one step.

## Authentication

This skill assumes `xplorertui` is already authenticated. The `mentions`
endpoint requires user-context auth (OAuth 2.0 PKCE or OAuth 1.0a — bearer-only
auth will not work). If the user hits an auth error:

- Run `xplorertui auth` to start the OAuth 2.0 PKCE flow, **or**
- Confirm `~/.config/xplorertui/.env` has `X_CONSUMER_KEY` / `X_ACCESS_TOKEN`
  pairs for OAuth 1.0a.

The script does not try to handle these — it lets `xplorertui`'s own error
message surface, which is the most informative thing to do.

## Edge cases worth knowing

- **Line 1 is the queried tweet, not the conversation root.** Most mentions
  are replies, so line 1 is the reply itself. The actual conversation root
  may show up later in the stream (or not at all if the API search misses
  it). Use `jq 'select(.tweet.id == .tweet.conversation_id)'` to locate it.

- **The mention IS the root.** When the mention is a top-level tweet (someone
  wrote a fresh tweet mentioning you), `tweet.id == tweet.conversation_id`
  on line 1, and lines 2..k are replies *to* the mention.

- **Empty thread.** If the X API returns no other tweets in the conversation
  (because the original was deleted or the search endpoint can't see them),
  only the queried tweet is printed. This is normal.

- **Rate limits.** The script makes 2 API calls per invocation. The X API
  free tier is unforgiving — if the user is iterating, suggest saving output
  to a file (`> thread.jsonl`) and inspecting the file rather than re-running.

- **Mentions newest-first ordering.** This is true today by virtue of the
  underlying `users/:id/mentions` endpoint. If the user asks for "the latest"
  mention, position 1 is correct. If they ask for "a mention from yesterday",
  they may need a larger N — there's no date filter at the moment, so suggest
  inspecting `xplorertui mentions | jq '.tweet.created_at'` first.
