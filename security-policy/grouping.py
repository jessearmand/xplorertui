"""Collapse classified alerts into update groups.

Dependabot files one alert per advisory, but the unit of work is one version
bump per package per manifest. A group is that bump: every alert it closes,
the version that closes all of them, and the most urgent decision among them.
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field

from policy import DECISION_ORDER, severity_rank

# Decisions that a version bump can act on, most urgent first. Blocked alerts
# have no patch, so they never drive a bump; they are carried on the group.
ACTIONABLE_ORDER = tuple(d for d in DECISION_ORDER if d != "Blocked")

_VERSION_RE = re.compile(r"v?(\d+(?:\.\d+)*)(.*)", re.IGNORECASE)
_PRE_RELEASE_RE = re.compile(r"[-._]?(a|b|c|rc|alpha|beta|pre|preview|dev)", re.IGNORECASE)
_PRE, _FINAL, _POST = 0, 1, 2


def normalize_name(package: str, ecosystem: str | None) -> str:
    """Canonical package name, so `Pillow` and `pillow` land in one group."""
    name = package.strip()
    if (ecosystem or "").lower() == "pip":
        return re.sub(r"[-_.]+", "-", name).lower()  # PEP 503
    if (ecosystem or "").lower() == "rust":
        return name.replace("_", "-")  # crates.io treats - and _ as equal
    return name


def version_key(version: str) -> tuple:
    """Sort key for release versions across ecosystems (semver, PEP 440).

    Compares the numeric release first (`3.15` == `3.15.0`), then orders
    pre-release < final < post/build. Unparseable strings sort lowest.
    """
    match = _VERSION_RE.fullmatch(version.strip())
    if not match:
        return ((), _PRE, version)
    release = [int(part) for part in match.group(1).split(".")]
    while len(release) > 1 and release[-1] == 0:
        release.pop()
    suffix = match.group(2)
    if not suffix:
        phase = _FINAL
    elif _PRE_RELEASE_RE.match(suffix):
        phase = _PRE
    else:
        phase = _POST
    return (tuple(release), phase, suffix)


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
    """Group classified rows by (ecosystem, package, manifest), most urgent first."""
    groups: dict[tuple[str, str, str], UpdateGroup] = {}
    for row in rows:
        ecosystem = row.get("ecosystem") or ""
        manifest = row.get("manifest") or ""
        package = normalize_name(row.get("package") or "", ecosystem)
        key = (ecosystem, package, manifest)
        if key not in groups:
            groups[key] = UpdateGroup(package, ecosystem, manifest)
        groups[key].alerts.append(row)
    return sorted(
        groups.values(),
        key=lambda g: (
            DECISION_ORDER.index(g.decision),
            severity_rank(g.max_severity),
            g.manifest,
            g.package,
        ),
    )
