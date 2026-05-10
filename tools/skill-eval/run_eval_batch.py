#!/usr/bin/env python3
"""
run_eval_batch.py — Trigger eval using Anthropic Batch API + Haiku 4.5 + caching.

Why this exists
---------------
The skill-creator's run_eval.py runs `claude -p` once per query×run, which:
  - costs Opus 4.7 rates (~$72 for our 5×30 sweep)
  - has a detection bug that ignored the canonical skill name (patched separately)
  - tests against Opus's permissive triggering threshold, not the floor where
    smaller coding-agent models live

This script replaces the trigger-eval phase only. It treats each eval as a
single Messages API call (no agentic loop) — the trigger question is a 1-turn
question: "does the description make the model emit a tool_use(Skill, ...)
in its first response?". Bundle all such calls into one batch for the 50%
discount, prime the cache once before the batch fires (otherwise concurrent
requests each pay a write), and you get ~15× cheaper than the original.

Run-time difference: live API ≈ seconds, batch ≈ minutes to ~1 hour. For
description optimization that's a fine trade.

Usage
-----
    uv run --with anthropic python run_eval_batch.py \\
        --eval-set ../xplorertui-mention-thread-workspace/eval_set.json \\
        --skill-path ../xplorertui-mention-thread \\
        --model claude-haiku-4-5 \\
        --runs-per-query 3 \\
        --out results-batch.json

Requires: ANTHROPIC_API_KEY in env (we use mise.toml in this project).
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys
import time
from typing import Any

import anthropic
from anthropic.types.message_create_params import MessageCreateParamsNonStreaming
from anthropic.types.messages.batch_create_params import Request


SKILL_TOOL = {
    "name": "Skill",
    "description": (
        "Invoke a named skill by passing its name. Use when the user's request "
        "matches one of the available skills described in the system prompt."
    ),
    "input_schema": {
        "type": "object",
        "properties": {
            "skill": {
                "type": "string",
                "description": "The exact skill name to invoke.",
            },
        },
        "required": ["skill"],
    },
}


def parse_skill_md(skill_path: pathlib.Path) -> tuple[str, str]:
    """Extract `name` and `description` from SKILL.md frontmatter."""
    text = (skill_path / "SKILL.md").read_text()
    if not text.startswith("---"):
        raise ValueError(f"{skill_path}/SKILL.md missing frontmatter")

    _, frontmatter, _body = text.split("---", 2)
    name: str | None = None
    in_description = False
    desc_lines: list[str] = []

    for line in frontmatter.splitlines():
        if line.startswith("name:"):
            name = line.split(":", 1)[1].strip()
            in_description = False
        elif line.startswith("description:"):
            after = line.split(":", 1)[1].strip()
            if after:
                desc_lines.append(after)
            in_description = True
        elif in_description and (line.startswith("  ") or line.startswith("\t")):
            desc_lines.append(line.strip())
        elif in_description and line.strip() == "":
            continue
        else:
            in_description = False

    if not name:
        raise ValueError("SKILL.md missing `name:` field")
    description = " ".join(desc_lines).strip()
    if not description:
        raise ValueError("SKILL.md missing `description:` field")
    return name, description


def build_system_prompt(skill_name: str, skill_description: str) -> str:
    """Mimic the format coding agents use to register skills.

    Kept deliberately minimal — we want to test whether the description text
    alone triggers, not whether a verbose preamble pushes the model toward it.
    """
    return (
        "You are a helpful coding assistant. You have access to one skill that "
        "you can invoke via the Skill tool when the user's request matches its "
        "purpose. If the user's request does not match, respond normally "
        "without calling the Skill tool.\n\n"
        "Available skills:\n"
        f"- {skill_name}: {skill_description}\n"
    )


def warm_cache(
    client: anthropic.Anthropic, model: str, system_prompt: str
) -> dict[str, int]:
    """Synchronous call to populate the prompt cache before the batch.

    Concurrent batch requests don't share cache writes — they all start before
    any of them finishes — so without this primer every batch request pays
    1.25× cache-write rates. One warm call writes the cache; the batch reads it.

    Returns the usage record so the caller can confirm cache_creation_input_tokens.
    """
    resp = client.messages.create(
        model=model,
        max_tokens=64,
        system=[
            {
                "type": "text",
                "text": system_prompt,
                "cache_control": {"type": "ephemeral"},
            }
        ],
        tools=[SKILL_TOOL],
        messages=[
            {
                "role": "user",
                "content": "Acknowledge readiness with a single word.",
            }
        ],
    )
    return {
        "input_tokens": resp.usage.input_tokens,
        "output_tokens": resp.usage.output_tokens,
        "cache_creation_input_tokens": getattr(
            resp.usage, "cache_creation_input_tokens", 0
        ),
        "cache_read_input_tokens": getattr(
            resp.usage, "cache_read_input_tokens", 0
        ),
    }


def build_requests(
    eval_set: list[dict[str, Any]],
    runs_per_query: int,
    model: str,
    system_prompt: str,
) -> list[Request]:
    requests: list[Request] = []
    for q_idx, item in enumerate(eval_set):
        for run in range(runs_per_query):
            custom_id = f"q{q_idx:02d}_r{run}"
            requests.append(
                Request(
                    custom_id=custom_id,
                    params=MessageCreateParamsNonStreaming(
                        model=model,
                        max_tokens=512,
                        system=[
                            {
                                "type": "text",
                                "text": system_prompt,
                                "cache_control": {"type": "ephemeral"},
                            }
                        ],
                        tools=[SKILL_TOOL],
                        messages=[
                            {"role": "user", "content": item["query"]}
                        ],
                    ),
                )
            )
    return requests


def poll_batch(
    client: anthropic.Anthropic, batch_id: str, poll_seconds: int = 15
) -> Any:
    """Poll until processing_status == 'ended'. Prints progress to stderr."""
    started = time.time()
    while True:
        batch = client.messages.batches.retrieve(batch_id)
        counts = batch.request_counts
        elapsed = int(time.time() - started)
        print(
            f"[{elapsed}s] batch {batch_id} status={batch.processing_status} "
            f"processing={counts.processing} succeeded={counts.succeeded} "
            f"errored={counts.errored} expired={counts.expired} "
            f"canceled={counts.canceled}",
            file=sys.stderr,
        )
        if batch.processing_status == "ended":
            return batch
        time.sleep(poll_seconds)


def collect_results(
    client: anthropic.Anthropic,
    batch_id: str,
    skill_name: str,
) -> tuple[dict[str, bool], dict[str, Any]]:
    """Iterate batch results.

    Returns (per_request_triggered, usage_totals).
    Triggered means the model's first tool_use was Skill(skill=skill_name).
    """
    triggered: dict[str, bool] = {}
    usage = {
        "input_tokens": 0,
        "output_tokens": 0,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 0,
    }

    for entry in client.messages.batches.results(batch_id):
        custom_id = entry.custom_id
        if entry.result.type != "succeeded":
            triggered[custom_id] = False
            continue

        message = entry.result.message
        u = message.usage
        usage["input_tokens"] += u.input_tokens
        usage["output_tokens"] += u.output_tokens
        usage["cache_creation_input_tokens"] += getattr(
            u, "cache_creation_input_tokens", 0
        )
        usage["cache_read_input_tokens"] += getattr(
            u, "cache_read_input_tokens", 0
        )

        # Look for the first tool_use block; trigger only if it's Skill with our name.
        first_tool_use = next(
            (b for b in message.content if b.type == "tool_use"), None
        )
        if first_tool_use is None:
            triggered[custom_id] = False
        elif first_tool_use.name == "Skill" and (
            first_tool_use.input.get("skill", "") == skill_name
        ):
            triggered[custom_id] = True
        else:
            triggered[custom_id] = False

    return triggered, usage


def estimate_cost(model: str, usage: dict[str, int]) -> dict[str, Any]:
    """Approximate $ cost from per-MTok rates after batch discount.

    Rates from https://platform.claude.com/docs/en/about-claude/pricing
    (batch input/output already 50% discounted; cache read = 0.10× base
    input; cache write 5m = 1.25× base input. Multipliers stack with batch.)
    """
    rates = {
        "claude-haiku-4-5": {"in": 0.50, "out": 2.50},
        "claude-sonnet-4-6": {"in": 1.50, "out": 7.50},
        "claude-opus-4-7": {"in": 2.50, "out": 12.50},
    }
    r = rates.get(model)
    if r is None:
        return {"note": f"unknown model {model}, no estimate"}

    base_in_per_mtok = r["in"] * 2  # invert batch 50% to get base
    cache_read_per_mtok = base_in_per_mtok * 0.10 * 0.5  # ×0.10 + batch
    cache_write_per_mtok = base_in_per_mtok * 1.25 * 0.5  # ×1.25 + batch
    out_per_mtok = r["out"]
    in_per_mtok = r["in"]

    new_input = max(
        0,
        usage["input_tokens"]
        - usage["cache_creation_input_tokens"]
        - usage["cache_read_input_tokens"],
    )
    cost = (
        new_input * in_per_mtok / 1_000_000
        + usage["cache_creation_input_tokens"] * cache_write_per_mtok / 1_000_000
        + usage["cache_read_input_tokens"] * cache_read_per_mtok / 1_000_000
        + usage["output_tokens"] * out_per_mtok / 1_000_000
    )
    return {
        "new_input_tokens": new_input,
        "cache_writes": usage["cache_creation_input_tokens"],
        "cache_reads": usage["cache_read_input_tokens"],
        "output_tokens": usage["output_tokens"],
        "estimated_cost_usd": round(cost, 4),
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Trigger eval via Batch API + caching.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument("--eval-set", required=True, type=pathlib.Path)
    parser.add_argument("--skill-path", required=True, type=pathlib.Path)
    parser.add_argument("--model", default="claude-haiku-4-5")
    parser.add_argument("--runs-per-query", type=int, default=3)
    parser.add_argument(
        "--trigger-threshold",
        type=float,
        default=0.5,
        help="Trigger rate >= threshold counts as 'fired' for should_trigger=true.",
    )
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=None,
        help="Optional path to write results.json. Always prints to stdout.",
    )
    parser.add_argument(
        "--skip-warmup",
        action="store_true",
        help="Skip the synchronous cache-prime call (saves one live API call "
        "but every batch request pays cache-write rates instead of read rates).",
    )
    args = parser.parse_args()

    eval_set = json.loads(args.eval_set.read_text())
    skill_name, skill_description = parse_skill_md(args.skill_path)
    system_prompt = build_system_prompt(skill_name, skill_description)
    total_requests = len(eval_set) * args.runs_per_query

    print(
        f"skill: {skill_name}\n"
        f"model: {args.model}\n"
        f"queries: {len(eval_set)}, runs/query: {args.runs_per_query}, "
        f"total requests: {total_requests}",
        file=sys.stderr,
    )

    client = anthropic.Anthropic()

    if not args.skip_warmup:
        print("Warming cache (1 synchronous call)...", file=sys.stderr)
        warm_usage = warm_cache(client, args.model, system_prompt)
        print(
            f"  warm-call usage: input={warm_usage['input_tokens']} "
            f"cache_write={warm_usage['cache_creation_input_tokens']} "
            f"cache_read={warm_usage['cache_read_input_tokens']}",
            file=sys.stderr,
        )

    print(f"Submitting batch of {total_requests} requests...", file=sys.stderr)
    requests = build_requests(
        eval_set, args.runs_per_query, args.model, system_prompt
    )
    batch = client.messages.batches.create(requests=requests)
    print(f"  batch_id: {batch.id}", file=sys.stderr)

    print("Polling for completion...", file=sys.stderr)
    final_batch = poll_batch(client, batch.id)
    print(f"Batch ended after {int(time.time())}s", file=sys.stderr)

    print("Collecting results...", file=sys.stderr)
    triggered, usage = collect_results(client, batch.id, skill_name)

    # Aggregate by query.
    per_query: list[dict[str, Any]] = []
    for q_idx, item in enumerate(eval_set):
        run_results = [
            triggered.get(f"q{q_idx:02d}_r{r}", False)
            for r in range(args.runs_per_query)
        ]
        n_trigger = sum(run_results)
        rate = n_trigger / args.runs_per_query
        should = item["should_trigger"]
        passed = (rate >= args.trigger_threshold) if should else (
            rate < args.trigger_threshold
        )
        per_query.append(
            {
                "query": item["query"],
                "should_trigger": should,
                "trigger_rate": rate,
                "triggers": n_trigger,
                "runs": args.runs_per_query,
                "pass": passed,
            }
        )

    n_pass = sum(1 for r in per_query if r["pass"])
    output = {
        "model": args.model,
        "skill_name": skill_name,
        "batch_id": batch.id,
        "trigger_threshold": args.trigger_threshold,
        "summary": {
            "total": len(per_query),
            "passed": n_pass,
            "failed": len(per_query) - n_pass,
            "request_counts": {
                "succeeded": final_batch.request_counts.succeeded,
                "errored": final_batch.request_counts.errored,
                "expired": final_batch.request_counts.expired,
                "canceled": final_batch.request_counts.canceled,
            },
        },
        "usage": usage,
        "cost_estimate": estimate_cost(args.model, usage),
        "results": per_query,
    }

    text = json.dumps(output, indent=2)
    if args.out:
        args.out.write_text(text)
        print(f"\nWrote {args.out}", file=sys.stderr)
    print(text)

    return 0 if n_pass == len(per_query) else 1


if __name__ == "__main__":
    sys.exit(main())
