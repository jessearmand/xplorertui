"""Find what holds a patched fix back from a plain merge.

pinned       the project's own requirement excludes the fixed version
constrained  the resolver cannot reach the fixed version (some other package's constraint)
major_jump   the fixed version is a breaking upgrade from the locked one
unverified   reachability could not be checked; unknown is never MustMerge
"""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from lockfiles import locked_versions
from manifests import DECLARING_MANIFESTS, LOCKFILE_MANIFESTS, ManifestIndex
from ranges import parse_range
from resolver import Reach
from specifiers import caret_compatible, requirement_allows
from versions import version_key


@dataclass(frozen=True)
class Hold:
    kind: str = "none"
    detail: str = ""


NO_HOLD = Hold()
UNPROBED = Reach(None, "reachability not checked")

# Strongest first: a definite reason outranks "unverified", which outranks no hold.
HOLD_PRIORITY = ("pinned", "constrained", "major_jump", "unverified", "none")


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

    def lockfile_for(self, group) -> Path | None:
        """The lockfile that pins this group's package, if one is present."""
        for lockfile in self._lockfiles_for(group.manifest):
            if lockfile not in self._locked:
                self._locked[lockfile] = locked_versions(lockfile)
            if group.package in self._locked[lockfile]:
                return lockfile
        return None

    def affected_locked_versions(self, group) -> list[str]:
        """Every locked copy of the package that this group's advisories apply to."""
        intervals = [parse_range(a.get("range")) for a in group.alerts]
        affected = []
        for lockfile in self._lockfiles_for(group.manifest):
            if lockfile not in self._locked:
                self._locked[lockfile] = locked_versions(lockfile)
            for version in self._locked[lockfile].get(group.package, []):
                if any(interval.contains(version_key(version)) for interval in intervals):
                    affected.append(version)
        return sorted(set(affected), key=version_key)

    def _declared_specifiers(self, group, locked: str | None) -> list[str]:
        """Specifiers governing this group's copy of the package.

        A lockfile may hold other versions pulled in transitively (`rand` 0.8
        beside a declared `rand = "0.10"`); the declaration does not govern those.
        """
        specifiers = self.manifests.specifiers(group.manifest, group.package) or []
        if locked is None:
            return specifiers
        return [s for s in specifiers if requirement_allows(s, locked, group.ecosystem) is not False]

    def _hold_for_copy(self, group, locked: str, reach: Reach) -> Hold:
        target = group.target_version
        if reach.removed:
            return NO_HOLD  # the upgrade drops this copy, so nothing about it can hold
        for specifier in self._declared_specifiers(group, locked):
            if requirement_allows(specifier, target, group.ecosystem) is False:
                return Hold("pinned", f"declared `{specifier}` excludes {target}")
        if reach.version is not None and version_key(reach.version) < version_key(target):
            return Hold("constrained", f"resolver only reaches {reach.version} from {locked}; the fix needs {target}")
        if is_breaking(locked, target, group.ecosystem):
            return Hold("major_jump", f"{locked} -> {target} is a breaking upgrade")
        if reach.version is None and not reach.removed:
            return Hold("unverified", reach.reason or "reachability not checked")
        return NO_HOLD

    def hold_for(self, group, reach: dict[str, Reach]) -> Hold:
        """The strongest hold across every affected locked copy; a bump must fix all of them.

        Known reasons rank above "unverified", and a fix nobody checked is never waved through.
        """
        target = group.target_version
        if target is None:
            return NO_HOLD
        if not group.locked_versions:
            return self._hold_for_copy_unlocked(group)
        holds = [self._hold_for_copy(group, locked, reach.get(locked, UNPROBED)) for locked in group.locked_versions]
        return min(holds, key=lambda hold: HOLD_PRIORITY.index(hold.kind))

    def _hold_for_copy_unlocked(self, group) -> Hold:
        """No lockfile entry: pins can still be read, but reachability cannot be checked."""
        target = group.target_version
        for specifier in self._declared_specifiers(group, None):
            if requirement_allows(specifier, target, group.ecosystem) is False:
                return Hold("pinned", f"declared `{specifier}` excludes {target}")
        return Hold("unverified", "no lockfile entry found for this package")
