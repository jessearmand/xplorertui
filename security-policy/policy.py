"""The triage rule. Mirrors policy.bend; test_bend_conformance.py checks they agree.

A fix that exists gets merged unless something concrete holds it. Severity never
changes the decision; it only sets the order in which updates are worked.

Decisions: MustMerge | Held | Blocked
"""
from __future__ import annotations

# Report order, most actionable first.
DECISION_ORDER = ("MustMerge", "Held", "Blocked")

# Why a patched fix cannot simply be merged. "none" means nothing holds it.
HOLD_KINDS = ("none", "pinned", "major_jump")

SEVERITY_RANK = {"critical": 0, "high": 1, "medium": 2, "low": 3}
UNKNOWN_SEVERITY_RANK = len(SEVERITY_RANK)


def severity_rank(severity: str | None) -> int:
    """Work order: lower ranks first. Unknown severities sort last."""
    return SEVERITY_RANK.get((severity or "").lower(), UNKNOWN_SEVERITY_RANK)


def decide(patched: bool, hold: str) -> str:
    if hold not in HOLD_KINDS:
        raise ValueError(f"unknown hold kind: {hold!r}")
    if not patched:
        return "Blocked"
    return "MustMerge" if hold == "none" else "Held"
