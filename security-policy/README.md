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
| **MustMerge** | critical/high with a patched version; medium in a *direct* dependency (declared in `pyproject.toml` / `Cargo.toml` / `package.json`) with a patch; or package on `ALWAYS_FIX` |
| **Review** | medium with a patch, in a transitive dependency |
| **Defer** | low severity (unless allowlisted) |
| **Blocked** | critical/high/medium with **no** patched version |

## Direct vs transitive

GitHub files alerts against the lockfile even for direct dependencies, so the alert's manifest path says nothing. `manifests.py` reads the manifest next to the lockfile (`uv.lock` -> `pyproject.toml`, `Cargo.lock` -> `Cargo.toml`, ...) and marks each alert `direct` when its package is declared there. `decide` only sees that boolean. Results therefore depend on the checkout: pass `--repo-root` to classify against another one.

## Grouping

Dependabot files one alert per advisory; the work is one bump per package per manifest. After per-alert classification, `grouping.py` collapses alerts into *updates*:

- key: ecosystem + normalized package name (`Pillow` = `pillow`, PEP 503) + manifest
- **Bump to**: the highest `first_patched_version` in the group, compared numerically
- decision: the most urgent of MustMerge > Review > Defer among its alerts; unpatched alerts never stop a bump and are listed as *still unpatched*; a group with no patch at all is Blocked
- packages locked at several versions are split into release lines: advisories with disjoint vulnerable ranges get separate bumps (`ranges.py`)

Modules: `policy.py` (rules), `manifests.py`, `grouping.py`, `ranges.py`, `versions.py`, `report.py`, `classify.py` (CLI).

Reference for the Bend port: `fixtures/decision-table.json` (all 40 input combinations, checked by `python3 security-policy/reference.py`) and `EVALUATION.md`. Tests: `python3 -m unittest discover -s security-policy`.

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
