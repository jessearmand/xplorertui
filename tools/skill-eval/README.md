# tools/skill-eval

Trigger eval harness for project skills.

`run_eval_batch.py` validates a skill's description by asking whether it
makes the model invoke the `Skill` tool on its first turn for a curated
set of queries. It uses the Anthropic Batch API + Haiku 4.5 because:

- **Batch API** gives a 50% discount and lets all eval requests run as
  one job (≈1 minute for ~30 requests in our experience).
- **Haiku 4.5** is the capability floor for skill triggering — if a
  description fires reliably here, it's robust on stronger models too.
  Optimizing only against Opus risks descriptions that depend on Opus's
  permissive triggering threshold and degrade for other coding agents
  consuming the same skill.

## Run an eval

Requires `ANTHROPIC_API_KEY` in env. We keep ours in `mise.toml` and run
via `mise exec`:

```bash
mise exec -- uv run --with anthropic python tools/skill-eval/run_eval_batch.py \
    --eval-set tools/skill-eval/xplorertui-mention-thread.eval.json \
    --skill-path .claude/skills/xplorertui-mention-thread \
    --model claude-haiku-4-5 \
    --runs-per-query 3 \
    --out /tmp/eval-out.json
```

The script prints progress to stderr while it polls the batch and a JSON
results document to stdout (also written to `--out` if given). Each
query is scored as `pass` if its trigger rate matches `should_trigger`
relative to `--trigger-threshold` (default 0.5).

## Eval set format

`<skill>.eval.json` is a JSON array of `{query, should_trigger}` objects.
Aim for roughly equal numbers of should-trigger and should-not-trigger
queries. Negative cases are most useful when they share keywords or
phrasing with the skill but have a different intent — they probe whether
the description is too greedy.

Use anonymized placeholder usernames (`alice`, `example`) and tweet IDs
in the eval set; do not include real third-party handles.

## Notes

- **Cache control.** The script sets `cache_control: ephemeral` on the
  system block. Caching only engages above the model's minimum cacheable
  block size (1024 tokens for Haiku 4.5). For small skill descriptions
  the cache won't fire and only the batch discount applies; that's still
  ~600× cheaper than running the same eval through `claude -p` on Opus.
- **Single-turn assumption.** This harness does not run an agentic loop.
  It measures *"does the description make the model want to invoke the
  Skill tool first?"* — that's enough for trigger eval. For behavior
  eval (does the skill produce correct output when invoked) you'd want a
  different harness that exercises the bundled scripts end-to-end.
