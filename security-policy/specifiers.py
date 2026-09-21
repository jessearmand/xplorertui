"""Does a declared requirement allow a given version?

Covers PEP 440 specifiers (pyproject.toml) and Cargo requirements. Anything
this cannot parse answers None: unknown, which callers treat as "no evidence".
"""
from __future__ import annotations

import re

from versions import version_key

_CLAUSE_RE = re.compile(r"\s*(===|==|~=|!=|<=|>=|<|>|=|\^|~)?\s*v?([0-9][^\s,;]*)\s*")


def _release(version: str) -> tuple[int, ...]:
    return version_key(version)[0]


def _has_prefix(version: str, prefix: tuple[int, ...]) -> bool:
    release = _release(version)
    padded = release + (0,) * (len(prefix) - len(release))
    return padded[: len(prefix)] == prefix


def _wildcard_prefix(bound: str) -> tuple[int, ...] | None:
    """`1.2.*` -> (1, 2); None when the bound has no wildcard."""
    if not re.search(r"\.(\*|x)$", bound, re.IGNORECASE):
        return None
    return tuple(int(part) for part in bound.split(".")[:-1])


def caret_compatible(version: str, base: str) -> bool:
    """Cargo's default: same leftmost non-zero component, and not older than base."""
    raw = tuple(int(part) for part in re.match(r"\d+(?:\.\d+)*", base).group(0).split("."))
    significant = next((i for i, part in enumerate(raw) if part), len(raw) - 1)
    return _has_prefix(version, raw[: significant + 1]) and version_key(version) >= version_key(base)


def _tilde_compatible(version: str, base: str, *, pep440: bool) -> bool:
    raw = tuple(int(part) for part in re.match(r"\d+(?:\.\d+)*", base).group(0).split("."))
    # PEP 440 `~=1.4.2` fixes all but the last component; Cargo `~1.4.2` fixes major.minor
    fixed = raw[:-1] if pep440 else raw[:2]
    return _has_prefix(version, fixed or raw[:1]) and version_key(version) >= version_key(base)


def _clause_allows(op: str, bound: str, version: str, default_op: str) -> bool:
    op = op or default_op
    prefix = _wildcard_prefix(bound)
    if prefix is not None:
        return _has_prefix(version, prefix) != (op == "!=")
    key, bound_key = version_key(version), version_key(bound)
    if op == "^":
        return caret_compatible(version, bound)
    if op in ("~", "~="):
        return _tilde_compatible(version, bound, pep440=op == "~=")
    return {
        "==": key == bound_key,
        "===": key == bound_key,
        "=": key == bound_key,
        "!=": key != bound_key,
        "<": key < bound_key,
        "<=": key <= bound_key,
        ">": key > bound_key,
        ">=": key >= bound_key,
    }[op]


def requirement_allows(specifier: str, version: str, ecosystem: str) -> bool | None:
    """True/False when the specifier can be evaluated, None when it cannot."""
    text = specifier.strip()
    if not text or text == "*":
        return True
    default_op = "^" if ecosystem == "rust" else "=="
    verdicts = []
    for clause in text.split(","):
        match = _CLAUSE_RE.fullmatch(clause)
        if not match:
            return None
        verdicts.append(_clause_allows(match.group(1), match.group(2), version, default_op))
    return all(verdicts)
