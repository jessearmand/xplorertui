#!/usr/bin/env python3
"""Classify Dependabot alerts using the same rules as LAWS.bend.

Decisions: MustMerge | Review | Defer | Blocked
Alerts are then grouped into updates (one bump per package per manifest).
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path

from grouping import UpdateGroup, group_alerts
from manifests import DirectDependencyResolver
from policy import classify
from report import render_md

FIXTURES = Path(__file__).parent / "fixtures"
REPO_ROOT = Path(__file__).parent.parent


def resolve_alerts_path(arg: str) -> Path | None:
    path = Path(arg)
    if path.is_file():
        return path
    # allow running from repo root
    alt = FIXTURES / "open-alerts.json"
    return alt if alt.is_file() else None


def print_summary(rows: list[dict], groups: list[UpdateGroup]) -> None:
    print("alerts:", dict(Counter(r["decision"] for r in rows)), "total:", len(rows))
    print("updates:", dict(Counter(g.decision for g in groups)), "total:", len(groups))
    for g in groups:
        if g.decision != "MustMerge":
            continue
        print(f"  MustMerge {g.package} -> {g.target_version} ({g.manifest}, {g.max_severity}, {len(g.alerts)} alerts)")


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("alerts_json", nargs="?", default="fixtures/open-alerts.json")
    p.add_argument("-o", "--report", help="Write markdown report path")
    p.add_argument("--json-out", help="Write classified per-alert JSON path")
    p.add_argument("--groups-out", help="Write grouped updates JSON path")
    p.add_argument("--repo-root", type=Path, default=REPO_ROOT, help="Checkout whose manifests decide direct vs transitive")
    args = p.parse_args()

    path = resolve_alerts_path(args.alerts_json)
    if path is None:
        print(f"missing alerts file: {args.alerts_json}", file=sys.stderr)
        return 1

    rows = classify(json.loads(path.read_text()), DirectDependencyResolver(args.repo_root))
    groups = group_alerts(rows)
    print_summary(rows, groups)

    report_path = Path(args.report) if args.report else FIXTURES / "classification-report.md"
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(render_md(rows, groups))
    print(f"wrote {report_path}")

    if args.json_out:
        Path(args.json_out).write_text(json.dumps(rows, indent=2))
        print(f"wrote {args.json_out}")
    if args.groups_out:
        Path(args.groups_out).write_text(json.dumps([g.to_dict() for g in groups], indent=2))
        print(f"wrote {args.groups_out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
