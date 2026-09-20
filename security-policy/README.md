# Dependabot must-merge policy (Bend harness)

Offline triage harness for Dependabot security alerts on **jessearmand/xplorertui**.

**Intent:** encode “which vulns must be fixed and merged” as Bend laws (`LAWS.bend`), run them over the live alert list, and only later wire CI. This branch does **not** add a GitHub Actions workflow and does **not** auto-merge anything.

## Why Bend

[Bend](https://bend-lang.com/) targets agent-written code with machine-checked laws. Here Bend is a **policy language**, not a rewrite of the app:

- `LAWS.bend` — source of truth for MustMerge / Review / Defer / Blocked
- `PROOF.bend` — stub for future `bend` proof-checking
- `classify.py` — executable stand-in implementing the same rules (Bend is young; install may be unavailable)

Keep `classify.py` aligned with `LAWS.bend`. When `bend` can check `PROOF.bend`, prefer that.

## Rules (summary)

| Decision | When |
|---|---|
| **MustMerge** | critical/high with a patched version; medium in a *direct* manifest (`pyproject.toml` / `Cargo.toml`) with a patch; or package on `ALWAYS_FIX` |
| **Review** | medium with a patch, but only appearing in a lockfile |
| **Defer** | low severity (unless allowlisted) |
| **Blocked** | critical/high/medium with **no** patched version |

## Offline run

```bash
# classify the committed fixture
python3 security-policy/classify.py security-policy/fixtures/open-alerts.json

# refresh fixture (needs gh auth), then re-classify
./security-policy/export_alerts.sh
python3 security-policy/classify.py
```

Writes `security-policy/fixtures/classification-report.md`.

## Fixture snapshot

`fixtures/open-alerts.json` was exported 2026-09-20 from open Dependabot alerts. Re-run `export_alerts.sh` before trusting counts for merge work.

## Next step (not in this PR)

After the offline report looks right, gate Dependabot security PRs / a triage bot on these decisions in CI.
