#!/usr/bin/env python3
"""Classify Dependabot alerts using the same rules as LAWS.bend.

Decisions: MustMerge | Review | Defer | Blocked
"""
from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path

ALWAYS_FIX: set[str] = set()  # keep in sync with LAWS.bend

DIRECT_SUFFIXES = ("pyproject.toml", "Cargo.toml", "package.json")


def is_direct_manifest(path: str | None) -> bool:
    if not path:
        return False
    return any(path.endswith(s) for s in DIRECT_SUFFIXES)


def decide(alert: dict) -> str:
    sev = (alert.get("severity") or "").lower()
    patched = alert.get("patched")
    pkg = alert.get("package") or ""
    manifest = alert.get("manifest") or ""

    if pkg in ALWAYS_FIX and patched:
        return "MustMerge"

    if sev in ("critical", "high"):
        return "MustMerge" if patched else "Blocked"

    if sev == "medium":
        if not patched:
            return "Blocked"
        return "MustMerge" if is_direct_manifest(manifest) else "Review"

    if sev == "low":
        if pkg in ALWAYS_FIX and patched:
            return "MustMerge"
        return "Defer"

    # unknown severity
    return "Review"


def classify(alerts: list[dict]) -> list[dict]:
    out = []
    for a in alerts:
        row = dict(a)
        row["decision"] = decide(a)
        out.append(row)
    return out


def render_md(rows: list[dict]) -> str:
    counts = Counter(r["decision"] for r in rows)
    order = ["MustMerge", "Review", "Defer", "Blocked"]
    lines = [
        "# Dependabot classification report",
        "",
        "Rules: `security-policy/LAWS.bend` (executable: `classify.py`).",
        "",
        "## Counts",
        "",
        "| Decision | Count |",
        "|---|---:|",
    ]
    for k in order:
        lines.append(f"| {k} | {counts.get(k, 0)} |")
    lines.append(f"| **Total** | **{len(rows)}** |")
    lines += ["", "## MustMerge (fix and merge)", ""]

    must = [r for r in rows if r["decision"] == "MustMerge"]
    must.sort(key=lambda r: ({"critical": 0, "high": 1, "medium": 2, "low": 3}.get(r["severity"], 9), r["number"]))
    if not must:
        lines.append("_None._")
    else:
        lines += [
            "| # | Sev | Package | Manifest | Patched | Summary |",
            "|---:|---|---|---|---|---|",
        ]
        for r in must:
            summary = (r.get("summary") or "").replace("|", "/")[:80]
            lines.append(
                f"| {r['number']} | {r['severity']} | `{r['package']}` | `{r.get('manifest','')}` | `{r.get('patched')}` | {summary} |"
            )

    for title, key in [
        ("Review (medium in lockfiles)", "Review"),
        ("Defer (low)", "Defer"),
        ("Blocked (no patch)", "Blocked"),
    ]:
        subset = [r for r in rows if r["decision"] == key]
        lines += ["", f"## {title} ({len(subset)})", ""]
        if not subset:
            lines.append("_None._")
            continue
        lines += [
            "| # | Sev | Package | Manifest | Patched |",
            "|---:|---|---|---|---|",
        ]
        for r in sorted(subset, key=lambda x: x["number"], reverse=True):
            lines.append(
                f"| {r['number']} | {r['severity']} | `{r['package']}` | `{r.get('manifest','')}` | `{r.get('patched')}` |"
            )
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("alerts_json", nargs="?", default="fixtures/open-alerts.json")
    p.add_argument("-o", "--report", help="Write markdown report path")
    p.add_argument("--json-out", help="Write classified JSON path")
    args = p.parse_args()

    path = Path(args.alerts_json)
    if not path.is_file():
        # allow running from repo root
        alt = Path(__file__).parent / "fixtures" / "open-alerts.json"
        if alt.is_file():
            path = alt
        else:
            print(f"missing alerts file: {args.alerts_json}", file=sys.stderr)
            return 1

    alerts = json.loads(path.read_text())
    rows = classify(alerts)
    counts = Counter(r["decision"] for r in rows)
    print("counts:", dict(counts))
    print("total:", len(rows))

    md = render_md(rows)
    report_path = Path(args.report) if args.report else Path(__file__).parent / "fixtures" / "classification-report.md"
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(md)
    print(f"wrote {report_path}")

    if args.json_out:
        Path(args.json_out).write_text(json.dumps(rows, indent=2))
        print(f"wrote {args.json_out}")

    # also print MustMerge numbers for quick scan
    must = [r for r in rows if r["decision"] == "MustMerge"]
    print("MustMerge:", ", ".join(f"#{r['number']}({r['severity']}/{r['package']})" for r in sorted(must, key=lambda r: r["number"], reverse=True)[:20]))
    if len(must) > 20:
        print(f"... and {len(must)-20} more")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
