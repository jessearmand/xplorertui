# Dependabot must-merge policy for xplorertui
# Source of truth for triage. Executable stand-in: classify.py
# Run `bend PROOF.bend` when Bend is installed; until then classify.py enforces these rules.

# Types (informal model of a Dependabot alert)
# Alert { number, severity, package, ecosystem, manifest, range, patched, summary }

# Decision: MustMerge | Review | Defer | Blocked

# LAW: critical or high with a known patch must be fixed and merged.
law must_merge_severe:
  for a: Alert
  (a.severity == "critical" or a.severity == "high") and a.patched != None
  implies decision(a) == MustMerge

# LAW: critical or high with no patch cannot be auto-merged — escalate.
law blocked_unpatched_severe:
  for a: Alert
  (a.severity == "critical" or a.severity == "high") and a.patched == None
  implies decision(a) == Blocked

# LAW: medium in a direct-declare manifest must merge when patched.
# Direct manifests: pyproject.toml, Cargo.toml (not lockfiles alone).
law must_merge_medium_direct:
  for a: Alert
  a.severity == "medium" and a.patched != None and is_direct_manifest(a.manifest)
  implies decision(a) == MustMerge

# LAW: medium only in lockfiles needs human review (transitive noise).
law review_medium_lockfile:
  for a: Alert
  a.severity == "medium" and a.patched != None and not is_direct_manifest(a.manifest)
  implies decision(a) == Review

# LAW: medium with no patch is blocked from auto-merge.
law blocked_unpatched_medium:
  for a: Alert
  a.severity == "medium" and a.patched == None
  implies decision(a) == Blocked

# LAW: low severity is deferred unless package is on the always-fix allowlist.
law defer_low:
  for a: Alert
  a.severity == "low" and a.package not in ALWAYS_FIX
  implies decision(a) == Defer

# LAW: allowlisted packages must merge even at low severity when patched.
law must_merge_allowlist:
  for a: Alert
  a.package in ALWAYS_FIX and a.patched != None
  implies decision(a) == MustMerge

# Helpers
def is_direct_manifest(path: String) -> Bool:
  path.ends_with("pyproject.toml") or path.ends_with("Cargo.toml") or path.ends_with("package.json")

# ALWAYS_FIX starts empty — add package names here when low-severity deps are still must-fix.
ALWAYS_FIX = []
