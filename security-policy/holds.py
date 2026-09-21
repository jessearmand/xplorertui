"""Find what holds a patched fix back from a plain merge.

pinned      the project's own requirement excludes the fixed version
major_jump  the fixed version is a breaking upgrade from the locked one
"""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from lockfiles import locked_versions
from manifests import DECLARING_MANIFESTS, LOCKFILE_MANIFESTS, ManifestIndex
from ranges import parse_range
from specifiers import caret_compatible, requirement_allows
from versions import version_key


@dataclass(frozen=True)
class Hold:
    kind: str = "none"
    detail: str = ""


NO_HOLD = Hold()


def is_breaking(locked: str, target: str, ecosystem: str) -> bool:
    """Would moving locked -> target cross a compatibility boundary?

    Cargo resolves within caret ranges, so 0.8 -> 0.9 is breaking there;
    elsewhere only a change of the leading component counts.
    """
    if ecosystem == "rust":
        return not caret_compatible(target, locked)
    return version_key(locked)[0][:1] != version_key(target)[0][:1]


class HoldResolver:
    def __init__(self, repo_root: Path, manifests: ManifestIndex):
        self.repo_root = repo_root
        self.manifests = manifests
        self._locked: dict[Path, dict[str, list[str]]] = {}

    def _lockfiles_for(self, alert_manifest: str) -> list[Path]:
        path = Path(alert_manifest)
        if path.name in LOCKFILE_MANIFESTS:
            return [self.repo_root / path]
        if path.name in DECLARING_MANIFESTS:
            siblings = [lock for lock, (manifest, _) in LOCKFILE_MANIFESTS.items() if manifest == path.name]
            return [self.repo_root / path.parent / lock for lock in siblings]
        return []

    def locked_version(self, group) -> str | None:
        """The locked version this group's advisories apply to (highest, if several)."""
        intervals = [parse_range(a.get("range")) for a in group.alerts]
        affected = []
        for lockfile in self._lockfiles_for(group.manifest):
            if lockfile not in self._locked:
                self._locked[lockfile] = locked_versions(lockfile)
            for version in self._locked[lockfile].get(group.package, []):
                if any(interval.contains(version_key(version)) for interval in intervals):
                    affected.append(version)
        return max(affected, key=version_key) if affected else None

    def _declared_specifiers(self, group, locked: str | None) -> list[str]:
        """Specifiers governing this group's copy of the package.

        A lockfile may hold other versions pulled in transitively (`rand` 0.8
        beside a declared `rand = "0.10"`); the declaration does not govern those.
        """
        specifiers = self.manifests.specifiers(group.manifest, group.package) or []
        if locked is None:
            return specifiers
        return [s for s in specifiers if requirement_allows(s, locked, group.ecosystem) is not False]

    def hold_for(self, group) -> Hold:
        target = group.target_version
        if target is None:
            return NO_HOLD
        locked = self.locked_version(group)
        for specifier in self._declared_specifiers(group, locked):
            if requirement_allows(specifier, target, group.ecosystem) is False:
                return Hold("pinned", f"declared `{specifier}` excludes {target}")
        if locked and is_breaking(locked, target, group.ecosystem):
            return Hold("major_jump", f"{locked} -> {target} is a breaking upgrade")
        return NO_HOLD
