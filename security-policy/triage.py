"""The whole pipeline: alerts -> grouped updates -> holds -> decisions."""
from __future__ import annotations

from pathlib import Path

from grouping import UpdateGroup, group_alerts, work_order
from holds import HoldResolver
from manifests import ManifestIndex
from policy import decide


def triage(alerts: list[dict], repo_root: Path) -> tuple[list[dict], list[UpdateGroup]]:
    """Returns the alerts as decided rows, and the updates in work order."""
    manifests = ManifestIndex(repo_root)
    holds = HoldResolver(repo_root, manifests)
    rows = [dict(alert, direct=manifests.is_direct(alert)) for alert in alerts]
    groups = group_alerts(rows)
    for group in groups:
        group.locked_version = holds.locked_version(group)
        group.hold = holds.hold_for(group)
        for row in group.alerts:
            row["hold"] = group.hold.kind
            row["decision"] = decide(bool(row.get("patched")), group.hold.kind)
    return rows, work_order(groups)
