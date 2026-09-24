"""Versions a lockfile pins, per package."""
from __future__ import annotations

import tomllib
from pathlib import Path

from names import normalize_name

# TOML lockfiles that list `[[package]]` tables with name and version
TOML_LOCKFILES = {"uv.lock": "pip", "poetry.lock": "pip", "Cargo.lock": "rust"}


def locked_versions(lockfile: Path) -> dict[str, list[str]]:
    """Normalized package name -> every locked version. Empty when unreadable or unsupported."""
    ecosystem = TOML_LOCKFILES.get(lockfile.name)
    if ecosystem is None or not lockfile.is_file():
        return {}
    versions: dict[str, list[str]] = {}
    for package in tomllib.loads(lockfile.read_text()).get("package", []):
        if "name" in package and "version" in package:
            versions.setdefault(normalize_name(package["name"], ecosystem), []).append(package["version"])
    return versions
