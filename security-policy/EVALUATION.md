# Policy evaluation

Fixture: 81 open alerts exported 2026-09-20. Every bump target was compared with the versions locked in
`Cargo.lock` and `mlx-server/uv.lock`; all 18 updates are real (each locked version is below its target).

## Result under the current rule

| Decision | Updates | Alerts |
|---|---:|---:|
| MustMerge | 15 | 72 |
| Held | 3 | 9 |
| Blocked | 0 | 0 |

Held:

| Update | Hold |
|---|---|
| `transformers` -> 5.10.0 (`pyproject.toml` and `uv.lock`) | pinned: declared `==5.3.0` on purpose (Gemma 4 loader regression) |
| `starlette` 0.52.1 -> 1.3.1 | major jump; transitive via `fastapi`, which must allow 1.x first |

## How the rule got here

The first rule decided by severity: critical/high MustMerge, medium Review unless direct, low Defer.
Running it against the repo showed severity was standing in for the real question, "can this be merged?":

| Finding | Evidence | Outcome |
|---|---|---|
| One bump target for a package locked at several versions | `rand` locked at 0.8.5, 0.9.2, 0.10.0, each with its own advisory | groups split by disjoint vulnerable range |
| Direct dependency classed as lockfile noise | `idna` is declared in `pyproject.toml`, but GitHub names `uv.lock` | declared manifests are read |
| MustMerge that cannot merge | `transformers==5.3.0` pin; `starlette` major jump | the Held decision, with detected reasons |
| Review/Defer with nothing actually in the way | `pydantic-settings`, `requests`, `pygments`, `rand` | MustMerge, ordered after the severe ones |

## Known gaps

- Parent constraints are not detected (needs a resolver dry run such as `uv lock --upgrade-package`).
- `transformers` appears twice, for `pyproject.toml` and `uv.lock`; it is one bump.
- Only TOML lockfiles are read (`uv.lock`, `poetry.lock`, `Cargo.lock`); npm requirement syntax is not evaluated. Both fail open: no evidence, no hold.
- Severity is the only ordering signal; EPSS, attack vector and runtime-vs-dev scope are not used.
