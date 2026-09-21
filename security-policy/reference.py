#!/usr/bin/env python3
"""Reference decision table for porting the policy to Bend.

Enumerates every combination of the inputs `decide` looks at and records the
Python decision. A Bend `decide` must reproduce this table exactly; the laws
in LAWS.bend should then hold over it by proof rather than by enumeration.

Regenerate after an intentional rule change:
    python3 security-policy/reference.py --write
"""
from __future__ import annotations

import argparse
import json
from itertools import product
from pathlib import Path

from policy import SEVERITY_RANK, decide

TABLE_PATH = Path(__file__).parent / "fixtures" / "decision-table.json"

SEVERITIES = (*SEVERITY_RANK, "unknown")
_PACKAGE = "pkg"


def build_table() -> list[dict]:
    rows = []
    for severity, patched, direct, allowlisted in product(SEVERITIES, (True, False), (True, False), (True, False)):
        alert = {
            "severity": severity,
            "patched": "1.0.0" if patched else None,
            "package": _PACKAGE,
            "manifest": "Cargo.lock",
            "direct": direct,
        }
        rows.append({
            "severity": severity,
            "patched": patched,
            "direct": direct,
            "allowlisted": allowlisted,
            "decision": decide(alert, always_fix={_PACKAGE} if allowlisted else set()),
        })
    return rows


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
