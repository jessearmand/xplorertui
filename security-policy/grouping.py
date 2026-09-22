"""Collapse classified alerts into update groups.

Dependabot files one alert per advisory, but the unit of work is one version
bump per package release line per manifest. A group is that bump: every alert it closes,
the version that closes all of them, and what (if anything) holds it back.
"""
from __future__ import annotations

from dataclasses import dataclass, field

from holds import NO_HOLD, Hold
from names import normalize_name
from policy import DECISION_ORDER, decide, severity_rank
from ranges import parse_range, partition_by_overlap
from versions import version_key

@dataclass
class UpdateGroup:
    package: str
    ecosystem: str
    manifest: str
    alerts: list[dict] = field(default_factory=list)
    hold: Hold = NO_HOLD
    locked_versions: list[str] = field(default_factory=list)  # every locked copy the advisories apply to

    @property
    def patched_alerts(self) -> list[dict]:
        return [a for a in self.alerts if a.get("patched")]

    @property
    def blocked_alerts(self) -> list[dict]:
        return [a for a in self.alerts if not a.get("patched")]

    @property
    def target_version(self) -> str | None:
        """Lowest version that closes every patched alert in the group."""
        versions = [a["patched"] for a in self.patched_alerts]
        return max(versions, key=version_key) if versions else None

    @property
    def decision(self) -> str:
        """Unpatched alerts never stop a bump: they stay Blocked on their own rows."""
        return decide(self.target_version is not None, self.hold.kind)

    @property
    def locked_version(self) -> str | None:
        """Highest affected locked copy, for display; holds consider every copy."""
        return max(self.locked_versions, key=version_key) if self.locked_versions else None

    @property
    def max_severity(self) -> str:
        return min((a.get("severity") or "unknown" for a in self.alerts), key=severity_rank)

    @property
    def numbers(self) -> list[int]:
        return sorted(a["number"] for a in self.alerts)

    def to_dict(self) -> dict:
        return {
            "package": self.package,
            "ecosystem": self.ecosystem,
            "manifest": self.manifest,
            "decision": self.decision,
            "max_severity": self.max_severity,
            "hold": self.hold.kind,
            "hold_detail": self.hold.detail,
            "locked_versions": sorted(self.locked_versions, key=version_key),
            "target_version": self.target_version,
            "alerts": self.numbers,
            "blocked_alerts": sorted(a["number"] for a in self.blocked_alerts),
        }


def group_alerts(rows: list[dict]) -> list[UpdateGroup]:
    """Group alert rows into updates.

    Rows sharing (ecosystem, package, manifest) are split further into release
    lines: advisories with disjoint vulnerable ranges need separate bumps.
    """
    by_package: dict[tuple[str, str, str], list[dict]] = {}
    for row in rows:
        ecosystem = row.get("ecosystem") or ""
        package = normalize_name(row.get("package") or "", ecosystem)
        by_package.setdefault((ecosystem, package, row.get("manifest") or ""), []).append(row)

    return [
        UpdateGroup(package, ecosystem, manifest, line)
        for (ecosystem, package, manifest), members in by_package.items()
        for line in partition_by_overlap(members, lambda row: parse_range(row.get("range")))
    ]


def work_order(groups: list[UpdateGroup]) -> list[UpdateGroup]:
    """Actionable decisions first; within a decision, severity sets the order."""
    return sorted(
        groups,
        key=lambda g: (
            DECISION_ORDER.index(g.decision),
            severity_rank(g.max_severity),
            g.manifest,
            g.package,
            version_key(g.target_version or ""),
        ),
    )
