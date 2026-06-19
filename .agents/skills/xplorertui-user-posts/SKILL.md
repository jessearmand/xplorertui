---
name: xplorertui-user-posts
description: Fetch the latest posts (tweet timeline) from any X user by username using the xplorertui CLI non-interactive mode (`search "from:USERNAME"`), emitting JSONL. Use this whenever the user asks for "latest tweets from @jessearmand", "recent posts by jessearmand", "what has jessearmand been posting", "jessearmand's last 10 tweets", or similar. The skill wraps `xplorertui search` + head to produce a clean, truncated JSONL stream of the user's most recent activity. Works for any public username.
---

# xplorertui-user-posts

Fetch the most recent posts from any X (Twitter) account as a JSONL stream using only the installed `xplorertui` binary in non-interactive CLI mode.

## What this skill does

Given a username (with or without `@`) and an optional count N (default 10), the skill runs:

```
xplorertui search 'from:USERNAME'
```

then truncates the output to the first N lines (newest posts first) and writes them to stdout as JSONL.

Each line has the standard denormalized shape used by the entire xplorertui CLI:

```json
{
  "tweet": { "id": "...", "text": "...", "created_at": "...", ... },
  "author": { "id": "...", "username": "jessearmand", "name": "Jesse", ... },
  "media": [ ... ]
}
```

The stream is ready for `jq`, `grep`, saving to a file, or feeding to another model.

## When to use this skill

Trigger on any request that needs a specific user's recent public activity:

- "Show me jessearmand's latest tweets"
- "What has @jessearmand posted in the last few days?"
- "Get the 5 most recent posts from jessearmand"
- "Pull jessearmand's timeline as JSONL"
- "Read the last 10 things jessearmand said on X"

Do **not** trigger for:
- Your own home timeline / mentions / bookmarks — use the direct `xplorertui home|mentions|bookmarks` commands.
- Searching across all of X — use `xplorertui search "keywords"`.
- Expanding a single tweet thread — use `xplorertui open <id>` or the mention-thread skill.

## The bundled wrapper script

The pipeline lives at `scripts/user-posts.sh` next to this `SKILL.md`.

Canonical location inside the xplorertui repo:

```
<repo>/.agents/skills/xplorertui-user-posts/scripts/user-posts.sh
```

A symlink is also maintained at:

```
<repo>/.claude/skills/xplorertui-user-posts/scripts/user-posts.sh
```

(so both the `.agents` and legacy `.claude` skill loaders discover it).

To install globally for your user (so the skill works from any working directory):

```bash
# Option A — via .agents (preferred for new skills)
ln -s "$(pwd)/.agents/skills/xplorertui-user-posts" \
      ~/.agents/skills/xplorertui-user-posts

# Option B — via .claude (for Claude Code / older agents)
ln -s "$(pwd)/.agents/skills/xplorertui-user-posts" \
      ~/.claude/skills/xplorertui-user-posts
```

After the symlink, you (or any agent) can invoke the script directly:

```bash
~/.agents/skills/xplorertui-user-posts/scripts/user-posts.sh jessearmand
```

## Usage

```bash
# Latest 10 posts from jessearmand (default)
./.agents/skills/xplorertui-user-posts/scripts/user-posts.sh jessearmand

# Or with the global symlink
~/.agents/skills/xplorertui-user-posts/scripts/user-posts.sh jessearmand

# Explicit count
.../user-posts.sh jessearmand 5
.../user-posts.sh -u jessearmand -n 5

# @ prefix is tolerated
.../user-posts.sh @jessearmand -n 3

# Save to a file for later processing
.../user-posts.sh jessearmand 20 > jessearmand-latest.jsonl
```

### Flags

| Flag | Meaning | Default |
|------|---------|---------|
| `-u USER` | Username (required if not given positionally) | — |
| `-n N` | Number of posts to return (1–100) | 10 |
| `-h` | Show usage | — |

Positional arguments are also accepted for convenience:

```
user-posts.sh USERNAME [N]
```

## What the script does internally

```bash
username="${1#@}"          # normalise
n="${2:-10}"

( xplorertui search "from:${username}" 2>/dev/null || true ) \
  | head -n "$n"
```

- `xplorertui search "from:..."` performs a recent-tweet search scoped to that author and prints up to `default_max_results` (configurable, default 20, max 100) newest-first JSONL lines.
- The subshell + `2>/dev/null || true` combination swallows the expected Rust "Broken pipe" panic that occurs when `head` stops reading after N lines.
- The first N clean JSON objects are passed through to the caller.

No `jq` is required inside the script itself (the raw JSONL is the product). Users who need to reshape the data run `jq` on the output.

## Output post-processing recipes

Once you have the JSONL, common `jq` one-liners:

```bash
# Just the tweet text + created_at (newest first)
.../user-posts.sh jessearmand | jq -r '"\(.tweet.created_at) \(.tweet.text)"'

# Author + text, one line per post
.../user-posts.sh jessearmand 5 | jq -r '"@\(.author.username): \(.tweet.text)"'

# Only original tweets (no RTs)
... | jq 'select(.tweet.text | startswith("RT ") | not)'

# Tweets that contain a mention or hashtag
... | jq 'select(.tweet.entities.mentions or .tweet.entities.hashtags)'

# Convert the whole stream into a single JSON array
... | jq -s '.'

# Count of returned posts
... | jq -s 'length'

# Oldest of the returned set (reverse the array)
... | jq -s 'reverse | .[0]'
```

Because each object already contains the full `author` and `media`, you rarely need a second API call.

## Authentication & rate limits

- The `search` command uses app-only bearer authentication (or any valid credential set). It works even if you only have `X_BEARER_TOKEN`.
- No user-context (OAuth) is required, therefore the skill works for any public account.
- The X API "recent search" endpoint is heavily rate-limited on the free tier. If you iterate rapidly, save results to a file (`> file.jsonl`) instead of re-running the command.
- Only the last ~7 days of a user's tweets are visible through this path. For older posts you must use the interactive TUI (which can paginate the proper `/users/:id/tweets` endpoint) or the academic/full-archive search product.

## Edge cases

- Fewer than N posts exist in the last 7 days → you receive however many the API returned (0 is possible for very quiet accounts).
- User does not exist → `xplorertui search` returns no results (empty output); the script exits 0.
- `default_max_results` in config is set higher than 20 → you can request up to that many (or 100) by using `-n` and having the config value large enough.
- Trying to request N > what the CLI will ever emit → you simply get all available lines (no error).

## Relationship to other xplorertui skills / commands

- `xplorertui-mention-thread` — expands the conversation around one of *your* mentions.
- `xplorertui open <id>` — fetches a single tweet + its full reply thread.
- Direct CLI: `xplorertui search "from:jessearmand"` (or any query) when you don't need the wrapper.

The user-posts skill is the canonical, documented way for agents to pull "what has this person been posting lately?" in a machine-readable JSONL form.
