"""Markdown rendering for the classification report."""
from __future__ import annotations

from collections import Counter

from grouping import UpdateGroup
from policy import DECISION_ORDER
from versions import version_key

SECTION_TITLES = {
    "MustMerge": "MustMerge (patched, nothing in the way)",
    "Held": "Held (patched, but something blocks a plain merge)",
    "Blocked": "Blocked (no patched version exists)",
}


def _cell(text: object) -> str:
    return str(text).replace("|", "/")


def _alert_links(alerts: list[dict]) -> str:
    return ", ".join(f"#{a['number']}" for a in sorted(alerts, key=lambda a: a["number"]))


def _counts_table(rows: list[dict], groups: list[UpdateGroup]) -> list[str]:
    alert_counts = Counter(r["decision"] for r in rows)
    group_counts = Counter(g.decision for g in groups)
    lines = ["| Decision | Updates | Alerts |", "|---|---:|---:|"]
    for decision in DECISION_ORDER:
        lines.append(f"| {decision} | {group_counts.get(decision, 0)} | {alert_counts.get(decision, 0)} |")
    lines.append(f"| **Total** | **{len(groups)}** | **{len(rows)}** |")
    return lines


def _group_table(groups: list[UpdateGroup]) -> list[str]:
    lines = [
        "| Package | Manifest | Locked | Bump to | Max sev | Dep | Held by | Alerts closed | Still unpatched |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    for g in groups:
        locked = ", ".join(f"`{v}`" for v in sorted(g.locked_versions, key=version_key)) or "—"
        target = f"`{g.target_version}`" if g.target_version else "—"
        dep = "direct" if any(a.get("direct") for a in g.alerts) else "transitive"
        lines.append(
            f"| `{_cell(g.package)}` | `{_cell(g.manifest)}` | {locked} | {target} | {g.max_severity} | {dep} "
            f"| {_cell(g.hold.detail) or '—'} | {_alert_links(g.patched_alerts) or '—'} "
            f"| {_alert_links(g.blocked_alerts) or '—'} |"
        )
    return lines


def _detail_table(groups: list[UpdateGroup]) -> list[str]:
    lines = ["| # | Sev | Package | Patched | Decision | Summary |", "|---:|---|---|---|---|---|"]
    for g in groups:
        for a in sorted(g.alerts, key=lambda a: a["number"]):
            summary = _cell(a.get("summary") or "")[:80]
            lines.append(
                f"| {a['number']} | {a.get('severity')} | `{_cell(g.package)}` "
                f"| `{a.get('patched')}` | {a['decision']} | {summary} |"
            )
    return lines


def render_md(rows: list[dict], groups: list[UpdateGroup]) -> str:
    lines = [
        "# Dependabot classification report",
        "",
        "Rules: `security-policy/LAWS.bend` (executable: `classify.py`).",
        "",
        "An *update* is one version bump of one package release line in one manifest.",
        "A patched fix is MustMerge unless something holds it; severity only sets the order.",
        "",
        "## Counts",
        "",
        *_counts_table(rows, groups),
    ]
    for decision in DECISION_ORDER:
        subset = [g for g in groups if g.decision == decision]
        lines += ["", f"## {SECTION_TITLES[decision]} ({len(subset)})", ""]
        lines += _group_table(subset) if subset else ["_None._"]

    lines += ["", "## Alert detail", "", *_detail_table(groups), ""]
    return "\n".join(lines)
