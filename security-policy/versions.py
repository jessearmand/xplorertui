"""Version ordering shared across ecosystems (semver, PEP 440)."""
from __future__ import annotations

import re

_VERSION_RE = re.compile(r"v?(\d+(?:\.\d+)*)(.*)", re.IGNORECASE)
_PRE_RELEASE_RE = re.compile(r"[-._]?(a|b|c|rc|alpha|beta|pre|preview|dev)", re.IGNORECASE)
_PRE, _FINAL, _POST = 0, 1, 2


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
