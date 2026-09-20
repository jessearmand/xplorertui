#!/usr/bin/env bash
# Refresh the open-alert fixture from GitHub Dependabot.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-$ROOT/fixtures/open-alerts.json}"
REPO="${REPO:-jessearmand/xplorertui}"

tmp="$(mktemp)"
gh api "repos/${REPO}/dependabot/alerts?state=open&per_page=100" --paginate > "$tmp"

python3 - "$tmp" "$OUT" <<'PY'
import json, sys
raw = json.load(open(sys.argv[1]))
rows = []
for a in raw:
    patched = (a.get("security_vulnerability") or {}).get("first_patched_version") or {}
    rows.append({
        "number": a["number"],
        "severity": a["security_advisory"]["severity"],
        "package": a["security_vulnerability"]["package"]["name"],
        "ecosystem": a["security_vulnerability"]["package"]["ecosystem"],
        "manifest": a["dependency"]["manifest_path"],
        "range": a["security_vulnerability"]["vulnerable_version_range"],
        "patched": patched.get("identifier"),
        "summary": a["security_advisory"]["summary"],
        "ghsa": a["security_advisory"].get("ghsa_id"),
        "cve": a["security_advisory"].get("cve_id"),
        "created_at": a["created_at"],
        "url": a["html_url"],
    })
json.dump(rows, open(sys.argv[2], "w"), indent=2)
print(f"wrote {len(rows)} alerts -> {sys.argv[2]}")
PY
rm -f "$tmp"
