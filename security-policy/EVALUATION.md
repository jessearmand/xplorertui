# Policy evaluation (Python reference run, 2026-09-20)

Fixture refreshed with `export_alerts.sh`: 81 open alerts, unchanged from the committed snapshot.
Every bump target was compared with the versions locked in `Cargo.lock` and `mlx-server/uv.lock`.

## Result

| Decision | Updates | Alerts |
|---|---:|---:|
| MustMerge | 11 | 31 |
| Review | 3 | 29 |
| Defer | 4 | 21 |
| Blocked | 0 | 0 |

All 18 updates are real: each locked version is below its bump target. No alert is already fixed.

## Where the policy is right

- The critical (`anyio`) and all high alerts land in MustMerge, ordered first.
- 81 alerts reduce to 18 bumps; `aiohttp` (24 alerts) and `pillow` (18) are one bump each.
- `rand` is locked at 0.8.5, 0.9.2 and 0.10.0; its three advisories have disjoint ranges and are kept as three bumps.

## Where the policy is wrong or blind

| Finding | Evidence | Rule gap |
|---|---|---|
| Direct dependency classed as lockfile noise | `idna` is declared in `mlx-server/pyproject.toml` but its alert names `uv.lock`, so it gets Review, not MustMerge | `is_direct_manifest` looks at the alert's manifest path; GitHub reports the lockfile even for direct deps. Needs the declared dependency list as an input. |
| MustMerge that cannot simply merge | `transformers` is pinned `==5.3.0` on purpose (Gemma 4 loader regression); target is 5.10.0 | No notion of a deliberate pin. Candidate decision: Blocked-by-pin, or a `PINNED` list parallel to `ALWAYS_FIX`. |
| Major-version jump treated like a patch bump | `starlette` 0.52.1 -> 1.3.1 (transitive via `fastapi`) | No upgrade-risk input. Compare locked and target major versions. |
| Same bump counted twice | `transformers` appears for `pyproject.toml` and `uv.lock` | Manifests in one directory could share a group. |
| Severity is the only risk signal | 26 of the 29 Review alerts sit in packages that already have a MustMerge bump, which closes them too; only `idna`, `pydantic-settings` and `requests` need a separate look | Grouping already absorbs these; per-alert counts overstate the review load. |

## Precedence, as Python resolves it

`fixtures/decision-table.json` enumerates all 40 input combinations
(severity x patched x direct manifest x allowlisted). It settles the overlaps in `LAWS.bend`:

- allowlisted and unpatched: the allowlist has no effect (Blocked for critical/high/medium)
- low and unpatched: Defer, allowlisted or not
- unknown severity: Review, unless allowlisted and patched

## Reference for the Bend port

1. `decide` in Bend must reproduce `fixtures/decision-table.json` row for row (`python3 reference.py` checks the Python side).
2. `test_reference.py` holds three invariants that pass by enumeration today and are the laws worth proving in Bend:
   an unpatched alert is never MustMerge; critical/high is never Defer; allowlisting never lowers urgency.
3. Grouping, version ordering and range overlap (`grouping.py`, `versions.py`, `ranges.py`) stay in Python unless the port needs them; `test_grouping.py` pins their behaviour.
