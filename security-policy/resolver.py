"""Ask the package manager what an upgrade can actually reach.

Lockfiles do not record the version constraints of the packages that pull a
dependency in, so whether a fix is reachable cannot be read from files. A
resolver dry run answers it without touching the lockfile. When the resolver
cannot be run, the answer is unknown, and unknown is never MustMerge.
"""
from __future__ import annotations

import os
import re
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

from names import normalize_name
from specifiers import caret_compatible
from versions import version_key

TIMEOUT_SECONDS = 300
_CHANGE_RE = re.compile(r"^\s*(?:Updat\w*|Upgrad\w*|Downgrad\w*)\s+(\S+)\s+v(\S+)\s+->\s+v(\S+)", re.MULTILINE)
_ADDED_RE = re.compile(r"^\s*Add\w*\s+(\S+)\s+v(\S+)", re.MULTILINE)
_REMOVED_RE = re.compile(r"^\s*Remov\w*\s+(\S+)\s+v(\S+)", re.MULTILINE)


@dataclass(frozen=True)
class Reach:
    """The version an upgrade would land on; None with a reason when unknown."""

    version: str | None
    reason: str = ""
    removed: bool = False  # the upgrade drops this copy of the package altogether


Package = tuple[str, str]  # (normalized name, locked version)


def _cargo_command(packages: list[Package]) -> list[str]:
    specs = [arg for name, version in packages for arg in ("-p", f"{name}@{version}")]
    return ["cargo", "update", "--dry-run", *specs]


def _uv_command(packages: list[Package]) -> list[str]:
    names = [arg for name, _ in packages for arg in ("--upgrade-package", name)]
    return ["uv", "lock", "--dry-run", *names]


# Lockfile name -> (ecosystem, dry-run command builder)
RESOLVERS = {"Cargo.lock": ("rust", _cargo_command), "uv.lock": ("pip", _uv_command)}


@dataclass(frozen=True)
class Changes:
    """What a dry run reported, keyed by normalized package name."""

    updated: dict[Package, str]
    added: dict[str, list[str]]
    removed: set[Package]


def parse_changes(output: str, ecosystem: str) -> Changes:
    added: dict[str, list[str]] = {}
    for name, version in _ADDED_RE.findall(output):
        added.setdefault(normalize_name(name, ecosystem), []).append(version)
    return Changes(
        updated={(normalize_name(name, ecosystem), old): new for name, old, new in _CHANGE_RE.findall(output)},
        added=added,
        removed={(normalize_name(name, ecosystem), version) for name, version in _REMOVED_RE.findall(output)},
    )


def _succeeds(candidate: str, locked: str, ecosystem: str) -> bool:
    """Is candidate the same release line as locked, moved forward?"""
    if ecosystem == "rust":
        return caret_compatible(candidate, locked)
    return version_key(candidate) >= version_key(locked)


def reach_of(package: Package, changes: Changes, ecosystem: str) -> Reach:
    """Where one locked copy ends up. Unmentioned packages stay where they are.

    Cargo reports a package locked at several versions as removals and
    additions, so a removed copy is matched to the added version that succeeds it.
    """
    name, locked = package
    if package in changes.updated:
        return Reach(changes.updated[package])
    if package not in changes.removed:
        return Reach(locked)
    successors = [v for v in changes.added.get(name, []) if _succeeds(v, locked, ecosystem)]
    if successors:
        return Reach(max(successors, key=version_key))
    return Reach(None, "the upgrade removes this copy", removed=True)


class ResolverProbe:
    """Runs one dry run per lockfile and answers reachability for its packages."""

    def __init__(self, run=subprocess.run):
        self._run = run

    def _unknown(self, packages: list[Package], reason: str) -> dict[Package, Reach]:
        return {package: Reach(None, reason) for package in packages}

    def probe(self, lockfile: Path, packages: list[Package]) -> dict[Package, Reach]:
        if lockfile.name not in RESOLVERS:
            return self._unknown(packages, f"no resolver for {lockfile.name}")
        ecosystem, build_command = RESOLVERS[lockfile.name]
        command = build_command(packages)
        if shutil.which(command[0]) is None:
            return self._unknown(packages, f"`{command[0]}` is not installed")
        try:
            result = self._run(
                command,
                cwd=lockfile.parent,
                capture_output=True,
                text=True,
                timeout=TIMEOUT_SECONDS,
                env={**os.environ, "NO_COLOR": "1"},
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            return self._unknown(packages, f"`{command[0]}` dry run failed: {error}")
        if result.returncode != 0:
            last_line = (result.stderr.strip().splitlines() or ["no output"])[-1]
            return self._unknown(packages, f"`{command[0]}` dry run failed: {last_line}")
        changes = parse_changes(result.stdout + result.stderr, ecosystem)
        return {package: reach_of(package, changes, ecosystem) for package in packages}


class OfflineProbe:
    """Used with --offline: nothing is verified."""

    def probe(self, lockfile: Path, packages: list[Package]) -> dict[Package, Reach]:
        return {package: Reach(None, "resolver not run (--offline)") for package in packages}
