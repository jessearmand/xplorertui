"""Collapse classified alerts into update groups.

Dependabot files one alert per advisory, but the unit of work is one version
bump per package release line per manifest. A group is that bump: every alert it closes,
the version that closes all of them, and the most urgent decision among them.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field

from policy import DECISION_ORDER, severity_rank
from ranges import parse_range, partition_by_overlap
from versions import version_key

# Decisions that a version bump can act on, most urgent first. Blocked alerts
# have no patch, so they never drive a bump; they are carried on the group.
ACTIONABLE_ORDER = tuple(d for d in DECISION_ORDER if d != "Blocked")


def normalize_name(package: str, ecosystem: str | None) -> str:
    """Canonical package name, so `Pillow` and `pillow` land in one group."""
    name = package.strip()
    if (ecosystem or "").lower() == "pip":
        return re.sub(r"[-_.]+", "-", name).lower()  # PEP 503
    if (ecosystem or "").lower() == "rust":
        return name.replace("_", "-")  # crates.io treats - and _ as equal
    return name


@dataclass
class UpdateGroup:
    package: str
    ecosystem: str
    manifest: str
    alerts: list[dict] = field(default_factory=list)

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
        decisions = {a["decision"] for a in self.alerts}
        for candidate in ACTIONABLE_ORDER:
            if candidate in decisions:
                return candidate
        return "Blocked"

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
            "target_version": self.target_version,
            "alerts": self.numbers,
            "blocked_alerts": sorted(a["number"] for a in self.blocked_alerts),
        }


def group_alerts(rows: list[dict]) -> list[UpdateGroup]:
    """Group classified rows into updates, most urgent first.

    Rows sharing (ecosystem, package, manifest) are split further into release
    lines: advisories with disjoint vulnerable ranges need separate bumps.
    """
    by_package: dict[tuple[str, str, str], list[dict]] = {}
    for row in rows:
        ecosystem = row.get("ecosystem") or ""
        package = normalize_name(row.get("package") or "", ecosystem)
        by_package.setdefault((ecosystem, package, row.get("manifest") or ""), []).append(row)

    groups = [
        UpdateGroup(package, ecosystem, manifest, line)
        for (ecosystem, package, manifest), members in by_package.items()
        for line in partition_by_overlap(members, lambda row: parse_range(row.get("range")))
    ]
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
