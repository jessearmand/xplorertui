"""The whole pipeline: alerts -> grouped updates -> reachability -> holds -> decisions."""
from __future__ import annotations

from pathlib import Path

from grouping import UpdateGroup, group_alerts, work_order
from holds import HoldResolver
from manifests import ManifestIndex
from policy import decide
from resolver import Reach, ResolverProbe


def _probe_reachability(groups: list[UpdateGroup], holds: HoldResolver, probe) -> dict[int, dict[str, Reach]]:
    """One resolver dry run per lockfile, covering every affected copy of every patched group it pins."""
    by_lockfile: dict[Path, list[UpdateGroup]] = {}
    for group in groups:
        lockfile = holds.lockfile_for(group)
        if lockfile is not None and group.locked_versions and group.target_version:
            by_lockfile.setdefault(lockfile, []).append(group)

    reach: dict[int, dict[str, Reach]] = {}
    for lockfile, members in by_lockfile.items():
        packages = sorted({(g.package, locked) for g in members for locked in g.locked_versions})
        answers = probe.probe(lockfile, packages)
        for group in members:
            reach[id(group)] = {locked: answers[(group.package, locked)] for locked in group.locked_versions}
    return reach


def triage(alerts: list[dict], repo_root: Path, probe=None) -> tuple[list[dict], list[UpdateGroup]]:
    """Returns the alerts as decided rows, and the updates in work order."""
    probe = ResolverProbe() if probe is None else probe
    manifests = ManifestIndex(repo_root)
    holds = HoldResolver(repo_root, manifests)
    rows = [dict(alert, direct=manifests.is_direct(alert)) for alert in alerts]
    groups = group_alerts(rows)
    for group in groups:
        group.locked_versions = holds.affected_locked_versions(group)
    reach = _probe_reachability(groups, holds, probe)
    for group in groups:
        group.hold = holds.hold_for(group, reach.get(id(group), {}))
        for row in group.alerts:
            row["hold"] = group.hold.kind
            row["decision"] = decide(bool(row.get("patched")), group.hold.kind)
    return rows, work_order(groups)
