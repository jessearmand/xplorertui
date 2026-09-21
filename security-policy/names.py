"""Package name normalization per ecosystem."""
from __future__ import annotations

import re


def normalize_name(package: str, ecosystem: str | None) -> str:
    """Canonical package name, so `Pillow` and `pillow` land in one group."""
    name = package.strip()
    if (ecosystem or "").lower() == "pip":
        return re.sub(r"[-_.]+", "-", name).lower()  # PEP 503
    if (ecosystem or "").lower() == "rust":
        return name.replace("_", "-")  # crates.io treats - and _ as equal
    return name
