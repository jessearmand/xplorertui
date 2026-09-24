"""What a project declares directly, and with which version requirements.

GitHub files an alert against the lockfile even when the package is a direct
dependency, so the alert's manifest path cannot tell direct from transitive.
This reads the manifest that sits next to the lockfile.
"""
from __future__ import annotations

import json
import re
import tomllib
from pathlib import Path

from names import normalize_name

# Lockfile name -> (declaring manifest next to it, ecosystem of its package names)
LOCKFILE_MANIFESTS = {
    "uv.lock": ("pyproject.toml", "pip"),
    "poetry.lock": ("pyproject.toml", "pip"),
    "Cargo.lock": ("Cargo.toml", "rust"),
    "package-lock.json": ("package.json", "npm"),
    "yarn.lock": ("package.json", "npm"),
    "pnpm-lock.yaml": ("package.json", "npm"),
}
DECLARING_MANIFESTS = {name: ecosystem for name, ecosystem in LOCKFILE_MANIFESTS.values()}

# PEP 508: name, optional [extras], then a specifier, `@ url`, or `; marker`
_REQUIREMENT_RE = re.compile(r"\s*([A-Za-z0-9][A-Za-z0-9._-]*)\s*(?:\[[^\]]*\])?\s*([^;@]*)")
_CARGO_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")
_NPM_TABLES = ("dependencies", "devDependencies", "optionalDependencies", "peerDependencies")

Requirements = dict[str, list[str]]  # package name -> declared version specifiers ("" = unconstrained)


def _add(requirements: Requirements, name: str, specifier: object) -> None:
    requirements.setdefault(name, []).append(specifier.strip() if isinstance(specifier, str) else "")


def _pep508_requirements(entries: list, requirements: Requirements) -> None:
    for entry in entries:
        # PEP 735 groups may hold {include-group = "..."} tables; those name no package
        match = _REQUIREMENT_RE.match(entry) if isinstance(entry, str) else None
        if match:
            _add(requirements, match.group(1), match.group(2).strip("() "))


def _table_requirements(table: dict, requirements: Requirements, skip: tuple[str, ...] = ()) -> None:
    for name, spec in table.items():
        if name in skip:
            continue
        if isinstance(spec, dict):
            # Cargo: `alias = { package = "real-name" }` declares real-name, not alias
            _add(requirements, spec.get("package", name), spec.get("version", ""))
        else:
            _add(requirements, name, spec)


def _pyproject_requirements(data: dict) -> Requirements:
    requirements: Requirements = {}
    project = data.get("project", {})
    entry_lists = [
        project.get("dependencies", []),
        *project.get("optional-dependencies", {}).values(),
        *data.get("dependency-groups", {}).values(),
        data.get("tool", {}).get("uv", {}).get("dev-dependencies", []),
    ]
    for entries in entry_lists:
        _pep508_requirements(entries, requirements)
    poetry = data.get("tool", {}).get("poetry", {})
    for table in (poetry.get("dependencies", {}), *(g.get("dependencies", {}) for g in poetry.get("group", {}).values())):
        _table_requirements(table, requirements, skip=("python",))
    return requirements


def _cargo_requirements(data: dict) -> Requirements:
    requirements: Requirements = {}
    for scope in (data, data.get("workspace", {}), *data.get("target", {}).values()):
        for table in _CARGO_TABLES:
            _table_requirements(scope.get(table, {}), requirements)
    return requirements


def _npm_requirements(data: dict) -> Requirements:
    requirements: Requirements = {}
    for table in _NPM_TABLES:
        _table_requirements(data.get(table, {}), requirements)
    return requirements


def declared_requirements(manifest: Path) -> Requirements:
    """Normalized package name -> declared specifiers. Empty if unreadable."""
    ecosystem = DECLARING_MANIFESTS.get(manifest.name)
    if ecosystem is None or not manifest.is_file():
        return {}
    if manifest.suffix == ".json":
        raw = _npm_requirements(json.loads(manifest.read_text()))
    else:
        data = tomllib.loads(manifest.read_text())
        raw = _pyproject_requirements(data) if ecosystem == "pip" else _cargo_requirements(data)
    merged: Requirements = {}
    for name, specifiers in raw.items():
        merged.setdefault(normalize_name(name, ecosystem), []).extend(specifiers)
    return merged


def declared_packages(manifest: Path) -> set[str]:
    return set(declared_requirements(manifest))


class ManifestIndex:
    """Cached view of the manifests in a checkout, addressed by an alert's manifest path."""

    def __init__(self, repo_root: Path):
        self.repo_root = repo_root
        self._requirements: dict[Path, Requirements] = {}

    def _declaring_manifest(self, alert_manifest: str) -> tuple[Path, str] | None:
        path = Path(alert_manifest)
        if path.name in DECLARING_MANIFESTS:
            return self.repo_root / path, DECLARING_MANIFESTS[path.name]
        if path.name in LOCKFILE_MANIFESTS:
            manifest_name, ecosystem = LOCKFILE_MANIFESTS[path.name]
            return self.repo_root / path.parent / manifest_name, ecosystem
        return None

    def specifiers(self, alert_manifest: str, package: str) -> list[str] | None:
        """Declared specifiers for the package, or None when it is not declared directly."""
        found = self._declaring_manifest(alert_manifest)
        if found is None:
            return None
        manifest, ecosystem = found
        if manifest not in self._requirements:
            self._requirements[manifest] = declared_requirements(manifest)
        return self._requirements[manifest].get(normalize_name(package, ecosystem))

    def is_direct(self, alert: dict) -> bool:
        manifest = alert.get("manifest") or ""
        if Path(manifest).name in DECLARING_MANIFESTS:
            return True
        return self.specifiers(manifest, alert.get("package") or "") is not None
