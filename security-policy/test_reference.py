from __future__ import annotations

import json
import unittest

from policy import decide
from reference import TABLE_PATH, build_table


class DecisionTableTest(unittest.TestCase):
    def setUp(self):
        self.table = build_table()

    def test_committed_table_matches_policy(self):
        self.assertEqual(json.loads(TABLE_PATH.read_text()), self.table)

    def test_must_merge_exactly_when_patched_and_unheld(self):
        for row in self.table:
            mergeable = row["patched"] and row["hold"] == "none"
            self.assertEqual(row["decision"] == "MustMerge", mergeable, row)

    def test_unpatched_is_always_blocked(self):
        self.assertEqual({r["decision"] for r in self.table if not r["patched"]}, {"Blocked"})

    def test_severity_never_changes_the_decision(self):
        by_inputs: dict[tuple, set[str]] = {}
        for row in self.table:
            by_inputs.setdefault((row["patched"], row["hold"]), set()).add(row["decision"])
        self.assertTrue(all(len(decisions) == 1 for decisions in by_inputs.values()))

    def test_unknown_hold_kind_is_rejected(self):
        with self.assertRaises(ValueError):
            decide(True, "vibes")


if __name__ == "__main__":
    unittest.main()
