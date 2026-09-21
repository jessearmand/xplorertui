"""The proven Bend policy and the Python policy must agree on every input."""
from __future__ import annotations

import os
import shutil
import subprocess
import unittest
from pathlib import Path

from reference import build_table

HERE = Path(__file__).parent
BEND = shutil.which("bend") or shutil.which("bend", path=os.path.expanduser("~/.bend/bin"))


def run_bend(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run([BEND, *args], cwd=HERE, capture_output=True, text=True, timeout=120)


def parse_row(line: str) -> dict:
    severity, patched, hold, decision = line.split()
    return {"severity": severity, "patched": patched == "true", "hold": hold, "decision": decision}


@unittest.skipUnless(BEND, "bend is not installed")
class BendConformanceTest(unittest.TestCase):
    def test_every_law_is_proven(self):
        result = run_bend("PROOF.bend", "--check-only")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_bend_decisions_match_python(self):
        result = run_bend("table.bend")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        rows = [parse_row(line) for line in result.stdout.splitlines() if line.strip()]
        self.assertEqual(rows, build_table())


if __name__ == "__main__":
    unittest.main()
