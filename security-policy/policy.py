"""Per-alert triage rules. Keep in sync with LAWS.bend.

Decisions: MustMerge | Review | Defer | Blocked
"""
from __future__ import annotations

ALWAYS_FIX: set[str] = set()  # keep in sync with LAWS.bend

DIRECT_SUFFIXES = ("pyproject.toml", "Cargo.toml", "package.json")

# Report order, most actionable first.
DECISION_ORDER = ("MustMerge", "Review", "Defer", "Blocked")

SEVERITY_RANK = {"critical": 0, "high": 1, "medium": 2, "low": 3}
UNKNOWN_SEVERITY_RANK = len(SEVERITY_RANK)


def severity_rank(severity: str | None) -> int:
    return SEVERITY_RANK.get((severity or "").lower(), UNKNOWN_SEVERITY_RANK)


def is_direct_manifest(path: str | None) -> bool:
    if not path:
        return False
    return any(path.endswith(s) for s in DIRECT_SUFFIXES)


def decide(alert: dict, always_fix: frozenset[str] | set[str] | None = None) -> str:
    always_fix = ALWAYS_FIX if always_fix is None else always_fix
    sev = (alert.get("severity") or "").lower()
    patched = alert.get("patched")
    pkg = alert.get("package") or ""
    manifest = alert.get("manifest") or ""

    if pkg in always_fix and patched:
        return "MustMerge"

    if sev in ("critical", "high"):
        return "MustMerge" if patched else "Blocked"

    if sev == "medium":
        if not patched:
            return "Blocked"
        return "MustMerge" if is_direct_manifest(manifest) else "Review"

    if sev == "low":
        return "Defer"

    # unknown severity
    return "Review"


def classify(alerts: list[dict]) -> list[dict]:
    out = []
    for a in alerts:
        row = dict(a)
        row["decision"] = decide(a)
        out.append(row)
    return out
