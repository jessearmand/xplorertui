#!/usr/bin/env bash
#
# topic-search.sh — Run one or more xplorertui search queries and emit deduplicated JSONL.
#
# Wraps the installed xplorertui CLI (cargo install) non-interactive search:
#   xplorertui search 'QUERY'
#
# Each line is a self-contained denormalized object:
#   {"tweet": {...}, "author": {...}, "media": [...]}
#
# Multiple queries are merged and deduplicated by tweet.id (first occurrence wins).
# Results are sorted newest-first by tweet.created_at.
#
# NOTE: The underlying endpoint is /tweets/search/recent — only the last ~7 days
# are visible. max_results per query comes from ~/.config/xplorertui/config.toml
# (default 20, max 100).
#
# Usage:
#   topic-search.sh "Claude Fable 5"
#   topic-search.sh "Anthropic model" "from:AnthropicAI"
#   topic-search.sh -n 15 "AI regulation" "government AI"
#   topic-search.sh -q "Claude Fable" -q "Fable 5 leak" > topic.jsonl
#
# Exit codes:
#   0   Success (even if zero tweets matched)
#   2   Bad arguments
#   127 xplorertui or jq not found on PATH

set -euo pipefail

usage() {
    sed -n '3,22p' "$0" | sed 's/^# \{0,1\}//'
}

n=""
queries=()

while getopts ":n:q:h" opt; do
    case "$opt" in
        n) n="$OPTARG" ;;
        q) queries+=("$OPTARG") ;;
        h) usage; exit 0 ;;
        \?) echo "topic-search: unknown option: -$OPTARG" >&2; usage >&2; exit 2 ;;
        :)  echo "topic-search: option -$OPTARG requires an argument" >&2; exit 2 ;;
    esac
done
shift $((OPTIND - 1))

if [[ ${#queries[@]} -eq 0 ]]; then
    queries=("$@")
fi

if [[ ${#queries[@]} -eq 0 ]]; then
    echo "topic-search: at least one search query is required" >&2
    usage >&2
    exit 2
fi

if [[ -n "$n" ]] && ! [[ "$n" =~ ^[1-9][0-9]*$ ]]; then
    echo "topic-search: -n must be a positive integer (got: $n)" >&2
    exit 2
fi

if ! command -v xplorertui >/dev/null 2>&1; then
    echo "topic-search: xplorertui not found on PATH (install with \`cargo install xplorertui\`)" >&2
    exit 127
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "topic-search: jq not found on PATH (required for deduplication)" >&2
    exit 127
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

for query in "${queries[@]}"; do
    if [[ -n "$n" ]]; then
        ( xplorertui search "$query" 2>/dev/null || true ) | head -n "$n" >>"$tmp"
    else
        ( xplorertui search "$query" 2>/dev/null || true ) >>"$tmp"
    fi
done

if [[ ! -s "$tmp" ]]; then
    exit 0
fi

jq -s '
  [ .[] | select(.tweet.id != null) ]
  | unique_by(.tweet.id)
  | sort_by(.tweet.created_at)
  | reverse
  | .[]
' -c "$tmp"