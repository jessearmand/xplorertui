#!/usr/bin/env bash
#
# mention-thread.sh — Fetch the conversation thread of a recent mention.
#
# Composes two xplorertui CLI commands:
#   1. `xplorertui mentions` to list recent mentions (newest first)
#   2. `xplorertui open <id>` to fetch the full conversation thread
#
# Output: JSONL on stdout. The first line is the root tweet of the conversation
# (which may or may not be the mention itself — a mention is usually a reply, so
# the root is whatever started the thread). Subsequent lines are replies in the
# order returned by the X API conversation_id search.
#
# Each line has shape: {"tweet": {...}, "author": {...}, "media": [...]}
#
# Usage:
#   mention-thread.sh                  # latest mention's thread
#   mention-thread.sh -n 3             # 3rd most recent mention's thread
#   mention-thread.sh -i 1234567890    # bypass mentions, expand a specific tweet
#   mention-thread.sh -i https://x.com/user/status/1234567890   # URL also works

set -euo pipefail

usage() {
    sed -n '3,21p' "$0" | sed 's/^# \{0,1\}//'
}

n=1
id=""

while getopts ":n:i:h" opt; do
    case "$opt" in
        n) n="$OPTARG" ;;
        i) id="$OPTARG" ;;
        h) usage; exit 0 ;;
        \?) echo "mention-thread: unknown option: -$OPTARG" >&2; usage >&2; exit 2 ;;
        :)  echo "mention-thread: option -$OPTARG requires an argument" >&2; exit 2 ;;
    esac
done

if ! command -v xplorertui >/dev/null 2>&1; then
    echo "mention-thread: xplorertui not found on PATH (install with \`cargo install --path .\`)" >&2
    exit 127
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "mention-thread: jq is required to parse JSONL" >&2
    exit 127
fi

# When -i is not given, look up the Nth mention.
if [[ -z "$id" ]]; then
    if ! [[ "$n" =~ ^[1-9][0-9]*$ ]]; then
        echo "mention-thread: -n must be a positive integer (got: $n)" >&2
        exit 2
    fi

    # `xplorertui mentions` already orders newest-first. Take the Nth line.
    # Using `awk` over `sed -n "${n}p"` so we can also short-circuit and
    # detect "fewer than N mentions returned" cleanly.
    id=$(
        xplorertui mentions \
            | awk -v n="$n" 'NR==n { print; exit } END { exit (NR < n) }' \
            | jq -r '.tweet.id // empty'
    ) || {
        echo "mention-thread: fewer than $n mention(s) available" >&2
        exit 1
    }

    if [[ -z "$id" ]]; then
        echo "mention-thread: no mention at position $n (mentions list may be empty)" >&2
        exit 1
    fi
fi

exec xplorertui open "$id"
