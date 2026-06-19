#!/usr/bin/env bash
#
# user-posts.sh — Fetch the latest N posts/tweets from any X (Twitter) user.
#
# Wraps the xplorertui CLI non-interactive search:
#   xplorertui search 'from:USERNAME'
#
# Output is truncated to the first N results (newest-first) and emitted as
# JSONL on stdout. Each line is a self-contained denormalized object:
#
#   {"tweet": {...}, "author": {...}, "media": [...]}
#
# This shape is identical to the output of `xplorertui search`, `home`,
# `mentions`, `open`, etc. so it composes cleanly with jq, downstream LLMs,
# or other tools.
#
# NOTE: The underlying endpoint is /tweets/search/recent with a from: filter.
# It only returns tweets from the last ~7 days and is limited to the
# default_max_results value in ~/.config/xplorertui/config.toml (default 20,
# max 100). For a user's full historical timeline use the TUI or the
# /users/:id/tweets endpoint directly.
#
# Usage:
#   user-posts.sh jessearmand                # latest 10 posts (default N=10)
#   user-posts.sh jessearmand 5              # latest 5 posts (positional N)
#   user-posts.sh -u jessearmand -n 3        # explicit flags
#   user-posts.sh @jessearmand               # @ prefix is stripped
#   user-posts.sh -u @jessearmand -n 20
#   user-posts.sh jessearmand > posts.jsonl  # save for later
#
# Exit codes:
#   0   Success (even if fewer than N tweets existed)
#   2   Bad arguments (missing username, non-numeric N, unknown flag)
#   127 xplorertui not found on PATH

set -euo pipefail

usage() {
    sed -n '3,28p' "$0" | sed 's/^# \{0,1\}//'
}

username=""
n=10

# Parse options
while getopts ":u:n:h" opt; do
    case "$opt" in
        u) username="$OPTARG" ;;
        n) n="$OPTARG" ;;
        h) usage; exit 0 ;;
        \?) echo "user-posts: unknown option: -$OPTARG" >&2; usage >&2; exit 2 ;;
        :)  echo "user-posts: option -$OPTARG requires an argument" >&2; exit 2 ;;
    esac
done
shift $((OPTIND - 1))

# Positional username (if -u was not used)
if [[ -z "$username" && $# -gt 0 ]]; then
    username="$1"
    shift
fi

# Positional N (second positional, only if username was positional too)
if [[ $# -gt 0 ]]; then
    n="$1"
fi

if [[ -z "$username" ]]; then
    echo "user-posts: username is required (without or with leading @)" >&2
    usage >&2
    exit 2
fi

# Normalise: strip optional @
username="${username#@}"

if ! [[ "$n" =~ ^[1-9][0-9]*$ ]]; then
    echo "user-posts: N must be a positive integer (got: $n)" >&2
    exit 2
fi

if ! command -v xplorertui >/dev/null 2>&1; then
    echo "user-posts: xplorertui not found on PATH (install with \`cargo install --path .\` inside the repo)" >&2
    exit 127
fi

# Run the search and truncate to N newest posts.
# We redirect xplorertui's stderr and ignore its exit status because
# intentionally closing the pipe after N lines triggers a Rust "Broken pipe"
# panic in the writer; the first N lines have already been emitted cleanly.
( xplorertui search "from:${username}" 2>/dev/null || true ) | head -n "$n"
