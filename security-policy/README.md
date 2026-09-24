# Dependabot must-merge policy (Bend harness)

Offline triage harness for Dependabot security alerts on **jessearmand/xplorertui**.

**Intent:** a fix that exists gets merged unless something concrete holds it. Severity never changes the decision; it only sets the order of work. This branch does **not** add a GitHub Actions workflow and does **not** auto-merge anything.

## Rule

| Decision | When |
|---|---|
| **MustMerge** | a patched version exists, the resolver confirmed it is reachable, and nothing holds it, at any severity |
| **Held** | a patched version exists, but a plain merge is not possible (see holds) |
| **Blocked** | no patched version exists: nothing to merge. Replace, pin below the range, disable the feature, or dismiss with a reason |

Holds, detected from the checkout (`holds.py`):

| Hold | Meaning | Example |
|---|---|---|
| `pinned` | the project's own requirement excludes the fixed version | `transformers==5.3.0`, fix is 5.10.0 |
| `constrained` | a resolver dry run cannot reach the fixed version: some other package's constraint excludes it | — |
| `major_jump` | the fixed version is a breaking upgrade from the locked one (Cargo: outside the caret range; elsewhere: leading component changes) | `starlette` 0.52.1 -> 1.3.1 |

| `unverified` | reachability was not checked (`--offline`, tool missing, no network, no resolver for that lockfile) | — |

Lockfiles do not record the constraints of the packages that pull a dependency in, so reachability is asked of the package manager (`resolver.py`): one `cargo update --dry-run -p name@version ...` per `Cargo.lock` and one `uv lock --dry-run --upgrade-package ...` per `uv.lock`. Dry runs need the network and never write the lockfile. **Unknown is never MustMerge:** without a verified answer the update is Held as `unverified`. When several holds apply, the order is pinned, constrained, major_jump, unverified.

## Bend and Python: who does what

[Bend](https://bend-lang.com/) proves laws about Bend code. The rule is small and pure, so it lives in Bend and is proven; everything that touches files, JSON or version strings stays in Python.

- `policy.bend` — `decide`: severity x patched x hold -> MustMerge / Held / Blocked, and `rank` for work order
- `LAWS.bend` — 3 rules and 5 invariants `decide` must obey. Humans edit this file.
- `PROOF.bend` — proofs of every law. **Gate: `bend PROOF.bend` must print `All terms check.`**
- `table.bend` — prints `decide` for all 50 inputs
- `policy.py` — the same rule in Python. `test_bend_conformance.py` runs the proof gate and asserts that Bend and Python agree on all 50 inputs (skipped when `bend` is not installed).

Changing the rule: edit `LAWS.bend` (intent), then `policy.bend` + `PROOF.bend` until the gate passes, then `policy.py`, then `python3 security-policy/reference.py --write`.

Install Bend: `curl -fsSL https://bend-lang.com/install.sh | sh` (macOS/Linux only), add `~/.bend/bin` to `PATH`.

## Pipeline (`triage.py`)

1. **Direct or transitive** (`manifests.py`): GitHub files alerts against the lockfile even for direct dependencies, so the manifest next to the lockfile is read (`uv.lock` -> `pyproject.toml`, `Cargo.lock` -> `Cargo.toml`, ...). Informational, and the source of declared requirements for pin detection.
2. **Group into updates** (`grouping.py`, `ranges.py`): one bump per package release line per manifest. Names are normalized (`Pillow` = `pillow`, PEP 503). Advisories with disjoint vulnerable ranges are separate bumps (`rand` 0.8 / 0.9 / 0.10). **Bump to** is the highest `first_patched_version` in the group, compared numerically (`versions.py`).
3. **Check reachability** (`resolver.py`): one dry run per lockfile, covering every patched update it pins.
4. **Find holds** (`holds.py`, `lockfiles.py`, `specifiers.py`): the locked version the advisories apply to, the declared requirement governing it, what the resolver reached, and whether the bump crosses a breaking boundary.
5. **Decide** (`policy.py`): per update, and per alert. An unpatched alert never stops its siblings' bump; it stays Blocked on its own row.
6. **Order**: MustMerge, Held, Blocked; within each, by severity.

Results depend on the checkout: pass `--repo-root` to triage against another one.

## Run

```bash
# triage the committed fixture (runs cargo and uv dry runs; needs the network)
python3 security-policy/classify.py

# without the network: every patched update is Held as unverified
python3 security-policy/classify.py --offline

# refresh the fixture (needs gh auth), then triage again
./security-policy/export_alerts.sh
python3 security-policy/classify.py

# tests (includes the Bend gate when bend is installed)
python3 -m unittest discover -s security-policy
```

Writes `fixtures/classification-report.md`. `--json-out` and `--groups-out` write per-alert and per-update JSON.

`fixtures/open-alerts.json` was exported 2026-09-20. Re-run `export_alerts.sh` before trusting counts for merge work. `EVALUATION.md` records what running the policy against the lockfiles showed.

## Upgrade impact (`impact/`)

`impact/` answers a different question: what a Held upgrade would change in the app. It holds
old-vs-new runs of mlx-server (`impact/harness/run.sh`) and a Bend proof of the blast radius
over every feature and package (`bend impact/PROOF.bend`). `impact/EVIDENCE.md` covers the
starlette 1.x and transformers 5.17 bumps.

## Next step (not in this PR)

Gate Dependabot security PRs / a triage bot on these decisions in CI.
