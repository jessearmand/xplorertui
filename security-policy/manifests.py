"""Find which packages a project declares directly.

GitHub files an alert against the lockfile even when the package is a direct
dependency, so the alert's manifest path cannot tell direct from transitive.
This reads the manifest that sits next to the lockfile and lists what it declares.
"""
from __future__ import annotations

import json
import re
import tomllib
from pathlib import Path

from grouping import normalize_name

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

_REQUIREMENT_NAME_RE = re.compile(r"\s*([A-Za-z0-9][A-Za-z0-9._-]*)")  # PEP 508
_CARGO_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")
_NPM_TABLES = ("dependencies", "devDependencies", "optionalDependencies", "peerDependencies")


def _requirement_names(requirements: list) -> set[str]:
    names = set()
    for requirement in requirements:
        # PEP 735 groups may hold {include-group = "..."} tables; those name no package
        match = _REQUIREMENT_NAME_RE.match(requirement) if isinstance(requirement, str) else None
        if match:
            names.add(match.group(1))
    return names


def _pyproject_names(data: dict) -> set[str]:
    project = data.get("project", {})
    requirement_lists = [
        project.get("dependencies", []),
        *project.get("optional-dependencies", {}).values(),
        *data.get("dependency-groups", {}).values(),
        data.get("tool", {}).get("uv", {}).get("dev-dependencies", []),
    ]
    poetry = data.get("tool", {}).get("poetry", {})
    poetry_tables = [poetry.get("dependencies", {}), *(g.get("dependencies", {}) for g in poetry.get("group", {}).values())]
    names = set().union(*map(_requirement_names, requirement_lists))
    return names | {name for table in poetry_tables for name in table if name != "python"}


def _cargo_table_names(table: dict) -> set[str]:
    # `alias = { package = "real-name" }` declares real-name, not alias
    return {spec.get("package", name) if isinstance(spec, dict) else name for name, spec in table.items()}


def _cargo_names(data: dict) -> set[str]:
    scopes = [data, data.get("workspace", {}), *data.get("target", {}).values()]
    return set().union(*(_cargo_table_names(scope.get(table, {})) for scope in scopes for table in _CARGO_TABLES))


def _npm_names(data: dict) -> set[str]:
    return set().union(*(data.get(table, {}).keys() for table in _NPM_TABLES))


def declared_packages(manifest: Path) -> set[str]:
    """Normalized names of every package the manifest declares. Empty if unreadable."""
    ecosystem = DECLARING_MANIFESTS.get(manifest.name)
    if ecosystem is None or not manifest.is_file():
        return set()
    if manifest.suffix == ".json":
        names = _npm_names(json.loads(manifest.read_text()))
    else:
        data = tomllib.loads(manifest.read_text())
        names = _pyproject_names(data) if ecosystem == "pip" else _cargo_names(data)
    return {normalize_name(name, ecosystem) for name in names}


class DirectDependencyResolver:
    """Answers whether an alert's package is declared directly in the repo."""

    def __init__(self, repo_root: Path):
        self.repo_root = repo_root
        self._declared: dict[Path, set[str]] = {}

    def _declared_in(self, manifest: Path) -> set[str]:
        if manifest not in self._declared:
            self._declared[manifest] = declared_packages(manifest)
        return self._declared[manifest]

    def is_direct(self, alert: dict) -> bool:
        path = Path(alert.get("manifest") or "")
        if path.name in DECLARING_MANIFESTS:
            return True
        if path.name not in LOCKFILE_MANIFESTS:
            return False
        manifest_name, ecosystem = LOCKFILE_MANIFESTS[path.name]
        package = normalize_name(alert.get("package") or "", ecosystem)
        return package in self._declared_in(self.repo_root / path.parent / manifest_name)
