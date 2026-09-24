#!/usr/bin/env python3
"""Reference decision table shared by policy.py and policy.bend.

Enumerates every input of the rule and records the Python decision. Severity
is part of the table to show that it never changes the outcome. table.bend
prints the same rows from the proven Bend rule; the two must match.

Regenerate after an intentional rule change:
    python3 security-policy/reference.py --write
"""
from __future__ import annotations

import argparse
import json
from itertools import product
from pathlib import Path

from policy import HOLD_KINDS, SEVERITY_RANK, decide

TABLE_PATH = Path(__file__).parent / "fixtures" / "decision-table.json"

SEVERITIES = (*SEVERITY_RANK, "unknown")


def build_table() -> list[dict]:
    return [
        {"severity": severity, "patched": patched, "hold": hold, "decision": decide(patched, hold)}
        for severity, patched, hold in product(SEVERITIES, (True, False), HOLD_KINDS)
    ]


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--write", action="store_true", help=f"Rewrite {TABLE_PATH.name}")
    args = p.parse_args()
    table = build_table()
    if args.write:
        TABLE_PATH.write_text(json.dumps(table, indent=2) + "\n")
        print(f"wrote {len(table)} rows -> {TABLE_PATH}")
        return 0
    if json.loads(TABLE_PATH.read_text()) != table:
        print(f"{TABLE_PATH.name} is stale; rerun with --write if the rule change is intended")
        return 1
    print(f"{TABLE_PATH.name} matches policy.py ({len(table)} rows)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
