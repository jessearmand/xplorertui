"""Vulnerable-range intervals, used to split a package into release lines.

A lockfile can hold several versions of one package (`rand` 0.8, 0.9, 0.10).
Advisories whose vulnerable ranges are disjoint hit different locked versions,
so each needs its own bump; overlapping ranges are closed by a single bump.
"""
from __future__ import annotations

import re
from dataclasses import dataclass

from versions import version_key

_CONSTRAINT_RE = re.compile(r"\s*(<=|>=|<|>|=)\s*(\S+)\s*")


@dataclass(frozen=True)
class Interval:
    """Version interval; a bound of None is unbounded on that side."""

    lo: tuple | None = None
    hi: tuple | None = None
    lo_inclusive: bool = True
    hi_inclusive: bool = False

    def starts_before_end_of(self, other: Interval) -> bool:
        if self.lo is None or other.hi is None:
            return True
        if self.lo == other.hi:
            return self.lo_inclusive and other.hi_inclusive
        return self.lo < other.hi

    def contains(self, key: tuple) -> bool:
        return Interval(key, key, True, True).overlaps(self)

    def overlaps(self, other: Interval) -> bool:
        return self.starts_before_end_of(other) and other.starts_before_end_of(self)

    def union(self, other: Interval) -> Interval:
        lo, lo_inc = _outer((self.lo, self.lo_inclusive), (other.lo, other.lo_inclusive), pick_low=True)
        hi, hi_inc = _outer((self.hi, self.hi_inclusive), (other.hi, other.hi_inclusive), pick_low=False)
        return Interval(lo, hi, lo_inc, hi_inc)


def _outer(a: tuple, b: tuple, *, pick_low: bool) -> tuple:
    (a_key, a_inc), (b_key, b_inc) = a, b
    if a_key is None or b_key is None:
        return (None, True)
    if a_key == b_key:
        return (a_key, a_inc or b_inc)
    a_wins = a_key < b_key if pick_low else a_key > b_key
    return a if a_wins else b


def parse_range(text: str | None) -> Interval:
    """Parse a GitHub advisory range such as `>= 0.9.0, < 0.9.3` or `= 0.10.0`.

    Anything unparseable yields the unbounded interval, which overlaps every
    other range and so never splits a group by mistake.
    """
    lo = hi = None
    lo_inclusive, hi_inclusive = True, False
    for part in (text or "").split(","):
        match = _CONSTRAINT_RE.fullmatch(part)
        if not match:
            return Interval()
        op, key = match.group(1), version_key(match.group(2))
        if op == "=":
            return Interval(key, key, True, True)
        if op in (">", ">="):
            lo, lo_inclusive = key, op == ">="
        else:
            hi, hi_inclusive = key, op == "<="
    return Interval(lo, hi, lo_inclusive, hi_inclusive)


def _sweep_order(interval: Interval) -> tuple:
    return (interval.lo is not None, interval.lo or (), not interval.lo_inclusive)


def partition_by_overlap(items: list, interval_of) -> list[list]:
    """Split items into clusters whose intervals are transitively overlapping."""
    clusters: list[list] = []
    reach: Interval | None = None
    for interval, item in sorted(((interval_of(i), i) for i in items), key=lambda p: _sweep_order(p[0])):
        if reach is not None and interval.overlaps(reach):
            clusters[-1].append(item)
            reach = reach.union(interval)
        else:
            clusters.append([item])
            reach = interval
    return clusters
