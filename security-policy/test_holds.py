from __future__ import annotations

import unittest
from pathlib import Path

from holds import is_breaking
from specifiers import requirement_allows
from test_manifests import CARGO, PYPROJECT, TempRepoTest
from triage import triage

UV_LOCK = """
[[package]]
name = "transformers"
version = "5.3.0"
[[package]]
name = "starlette"
version = "0.52.1"
[[package]]
name = "idna"
version = "3.11"
"""

CARGO_LOCK = """
[[package]]
name = "rand"
version = "0.8.5"
[[package]]
name = "rand"
version = "0.10.0"
"""


class RequirementAllowsTest(unittest.TestCase):
    def test_pep440(self):
        self.assertFalse(requirement_allows("==5.3.0", "5.10.0", "pip"))
        self.assertTrue(requirement_allows(">=3.10", "3.15", "pip"))
        self.assertFalse(requirement_allows(">=1.0.9,<2", "2.0.0", "pip"))
        self.assertTrue(requirement_allows("==1.*", "1.9", "pip"))
        self.assertFalse(requirement_allows("!=1.*", "1.9", "pip"))
        self.assertTrue(requirement_allows("~=2.2", "2.9", "pip"))
        self.assertFalse(requirement_allows("~=2.2.1", "2.3.0", "pip"))
        self.assertTrue(requirement_allows("", "9.9", "pip"))

    def test_cargo(self):
        self.assertTrue(requirement_allows("0.10", "0.10.1", "rust"))
        self.assertFalse(requirement_allows("0.10", "0.11.0", "rust"))
        self.assertTrue(requirement_allows("1", "1.9.0", "rust"))
        self.assertFalse(requirement_allows("=1.2.3", "1.2.4", "rust"))
        self.assertTrue(requirement_allows("~1.2", "1.2.9", "rust"))
        self.assertFalse(requirement_allows("~1.2", "1.3.0", "rust"))
        self.assertFalse(requirement_allows("0.0.3", "0.0.4", "rust"))

    def test_unparseable_is_unknown(self):
        self.assertIsNone(requirement_allows("^1.0 || ^2.0", "2.1.0", "npm"))


class IsBreakingTest(unittest.TestCase):
    def test_leading_component_outside_cargo(self):
        self.assertTrue(is_breaking("0.52.1", "1.3.1", "pip"))
        self.assertFalse(is_breaking("0.0.22", "0.0.31", "pip"))
        self.assertFalse(is_breaking("3.11", "3.15", "pip"))

    def test_caret_boundary_in_cargo(self):
        self.assertFalse(is_breaking("0.10.75", "0.10.80", "rust"))
        self.assertTrue(is_breaking("0.8.5", "0.9.3", "rust"))
        self.assertTrue(is_breaking("1.9.0", "2.0.0", "rust"))


def alert(number: int, package: str, patched: str | None, manifest: str, ecosystem: str, range_: str) -> dict:
    return {
        "number": number,
        "package": package,
        "severity": "high",
        "patched": patched,
        "manifest": manifest,
        "ecosystem": ecosystem,
        "range": range_,
    }


class TriageTest(TempRepoTest):
    def setUp(self):
        super().setUp()
        self.write("svc/pyproject.toml", PYPROJECT)
        self.write("svc/uv.lock", UV_LOCK)
        self.write("Cargo.toml", CARGO)
        self.write("Cargo.lock", CARGO_LOCK)

    def decisions(self, alerts: list[dict]) -> dict[str, tuple[str, str]]:
        _, groups = triage(alerts, Path(self.root))
        return {f"{g.package}@{g.target_version}": (g.decision, g.hold.kind) for g in groups}

    def test_pin_major_jump_and_plain_merge(self):
        result = self.decisions([
            alert(1, "transformers", "5.10.0", "svc/uv.lock", "pip", "< 5.10.0"),
            alert(2, "starlette", "1.3.1", "svc/uv.lock", "pip", "< 1.3.1"),
            alert(3, "idna", "3.15", "svc/uv.lock", "pip", "< 3.15"),
            alert(4, "idna", None, "svc/uv.lock", "pip", "< 99"),
        ])
        self.assertEqual(result["transformers@5.10.0"], ("Held", "pinned"))
        self.assertEqual(result["starlette@1.3.1"], ("Held", "major_jump"))
        self.assertEqual(result["idna@3.15"], ("MustMerge", "none"))

    def test_alert_in_the_declaring_manifest_finds_the_sibling_lockfile(self):
        result = self.decisions([alert(1, "starlette", "1.3.1", "svc/pyproject.toml", "pip", "< 1.3.1")])
        self.assertEqual(result["starlette@1.3.1"], ("Held", "major_jump"))

    def test_declared_requirement_does_not_govern_transitive_copies(self):
        result = self.decisions([
            alert(1, "rand", "0.8.6", "Cargo.lock", "rust", ">= 0.7.0, < 0.8.6"),
            alert(2, "rand", "0.10.1", "Cargo.lock", "rust", "= 0.10.0"),
        ])
        self.assertEqual(result, {"rand@0.8.6": ("MustMerge", "none"), "rand@0.10.1": ("MustMerge", "none")})

    def test_unpatched_rows_stay_blocked_inside_a_mergeable_group(self):
        rows, _ = triage(
            [
                alert(3, "idna", "3.15", "svc/uv.lock", "pip", "< 3.15"),
                alert(4, "idna", None, "svc/uv.lock", "pip", "< 99"),
            ],
            Path(self.root),
        )
        self.assertEqual({r["number"]: r["decision"] for r in rows}, {3: "MustMerge", 4: "Blocked"})

    def test_missing_lockfile_means_no_evidence_of_a_hold(self):
        result = self.decisions([alert(1, "starlette", "1.3.1", "elsewhere/uv.lock", "pip", "< 1.3.1")])
        self.assertEqual(result["starlette@1.3.1"], ("MustMerge", "none"))


if __name__ == "__main__":
    unittest.main()
