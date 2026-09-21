from __future__ import annotations

import json
import unittest

from reference import TABLE_PATH, build_table


class DecisionTableTest(unittest.TestCase):
    def setUp(self):
        self.table = build_table()

    def test_committed_table_matches_policy(self):
        self.assertEqual(json.loads(TABLE_PATH.read_text()), self.table)

    def test_unpatched_is_never_must_merge(self):
        self.assertFalse([r for r in self.table if not r["patched"] and r["decision"] == "MustMerge"])

    def test_severe_is_never_deferred(self):
        severe = [r for r in self.table if r["severity"] in ("critical", "high")]
        self.assertFalse([r for r in severe if r["decision"] == "Defer"])

    def test_allowlisting_never_lowers_urgency(self):
        urgency = {"Defer": 0, "Review": 1, "Blocked": 2, "MustMerge": 3}
        by_inputs = {(r["severity"], r["patched"], r["direct"], r["allowlisted"]): r["decision"] for r in self.table}
        for (severity, patched, direct, allowlisted), decision in by_inputs.items():
            if allowlisted:
                plain = by_inputs[(severity, patched, direct, False)]
                self.assertGreaterEqual(urgency[decision], urgency[plain], (severity, patched, direct))


if __name__ == "__main__":
    unittest.main()
