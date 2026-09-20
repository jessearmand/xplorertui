"""Run: python3 -m unittest discover -s security-policy"""
from __future__ import annotations

import unittest

from grouping import group_alerts, normalize_name
from policy import classify
from ranges import parse_range
from versions import version_key


def alert(number: int, package: str, severity: str, patched: str | None, **extra) -> dict:
    base = {
        "number": number,
        "package": package,
        "severity": severity,
        "patched": patched,
        "ecosystem": "pip",
        "manifest": "uv.lock",
    }
    return base | extra


class NormalizeNameTest(unittest.TestCase):
    def test_pip_names_follow_pep503(self):
        self.assertEqual(normalize_name("Pillow", "pip"), "pillow")
        self.assertEqual(normalize_name("pydantic_settings", "pip"), "pydantic-settings")
        self.assertEqual(normalize_name("zope.interface", "pip"), "zope-interface")

    def test_rust_names_keep_case(self):
        self.assertEqual(normalize_name("quinn_proto", "rust"), "quinn-proto")
        self.assertEqual(normalize_name("Inflector", "rust"), "Inflector")


class VersionKeyTest(unittest.TestCase):
    def test_numeric_not_lexicographic(self):
        self.assertGreater(version_key("0.103.13"), version_key("0.103.9"))
        self.assertGreater(version_key("12.3.0"), version_key("2.33.0"))

    def test_trailing_zeros_are_equal(self):
        self.assertEqual(version_key("3.15"), version_key("3.15.0"))
        self.assertGreater(version_key("3.15"), version_key("3.14.3"))

    def test_pre_release_sorts_below_final_and_post_above(self):
        self.assertLess(version_key("1.0.0-rc.1"), version_key("1.0.0"))
        self.assertLess(version_key("2.0b1"), version_key("2.0"))
        self.assertGreater(version_key("2.0.post1"), version_key("2.0"))

    def test_unparseable_sorts_lowest(self):
        self.assertLess(version_key("unknown"), version_key("0.0.1"))


class ParseRangeTest(unittest.TestCase):
    def test_touching_bounds_overlap_only_when_both_inclusive(self):
        self.assertFalse(parse_range("< 1.0").overlaps(parse_range(">= 1.0")))
        self.assertTrue(parse_range("<= 1.0").overlaps(parse_range(">= 1.0")))
        self.assertTrue(parse_range("= 1.0").overlaps(parse_range(">= 0.5, < 2.0")))

    def test_missing_or_garbled_range_is_unbounded(self):
        self.assertTrue(parse_range(None).overlaps(parse_range("= 9.9")))
        self.assertTrue(parse_range("see advisory").overlaps(parse_range("< 0.1")))


class GroupAlertsTest(unittest.TestCase):
    def test_case_variants_share_a_group_with_highest_target(self):
        rows = classify([
            alert(1, "pillow", "high", "12.2.0"),
            alert(2, "Pillow", "high", "12.3.0"),
            alert(3, "pillow", "low", "12.10.0"),
        ])
        (group,) = group_alerts(rows)
        self.assertEqual(group.package, "pillow")
        self.assertEqual(group.target_version, "12.10.0")
        self.assertEqual(group.numbers, [1, 2, 3])
        self.assertEqual(group.decision, "MustMerge")
        self.assertEqual(group.max_severity, "high")

    def test_same_package_in_two_manifests_is_two_groups(self):
        rows = classify([
            alert(1, "transformers", "high", "5.5.0"),
            alert(2, "transformers", "high", "5.5.0", manifest="pyproject.toml"),
        ])
        self.assertEqual(len(group_alerts(rows)), 2)

    def test_unpatched_alert_does_not_block_a_patched_bump(self):
        rows = classify([
            alert(1, "aiohttp", "high", None),
            alert(2, "aiohttp", "medium", "3.14.0"),
        ])
        (group,) = group_alerts(rows)
        self.assertEqual(group.decision, "Review")
        self.assertEqual(group.target_version, "3.14.0")
        self.assertEqual([a["number"] for a in group.blocked_alerts], [1])

    def test_fully_unpatched_group_is_blocked(self):
        (group,) = group_alerts(classify([alert(1, "idna", "critical", None)]))
        self.assertEqual(group.decision, "Blocked")
        self.assertIsNone(group.target_version)

    def test_disjoint_ranges_split_into_release_lines(self):
        rows = classify([
            alert(1, "rand", "low", "0.8.6", range=">= 0.7.0, < 0.8.6"),
            alert(2, "rand", "low", "0.9.3", range=">= 0.9.0, < 0.9.3"),
            alert(3, "rand", "low", "0.10.1", range="= 0.10.0"),
        ])
        self.assertEqual([g.target_version for g in group_alerts(rows)], ["0.8.6", "0.9.3", "0.10.1"])

    def test_overlap_is_transitive_regardless_of_input_order(self):
        rows = classify([
            alert(1, "openssl", "high", "0.10.80", range=">= 0.10.50, < 0.10.80"),
            alert(2, "openssl", "high", "0.10.10", range=">= 0.9.0, < 0.10.10"),
            alert(3, "openssl", "high", "0.10.78", range=">= 0.10.5, < 0.10.78"),
        ])
        (group,) = group_alerts(rows)
        self.assertEqual(group.target_version, "0.10.80")

    def test_groups_sorted_most_urgent_first(self):
        rows = classify([
            alert(1, "rand", "low", "0.9.3"),
            alert(2, "requests", "medium", "2.33.0"),
            alert(3, "urllib3", "high", "2.7.0"),
            alert(4, "anyio", "critical", "4.14.2"),
        ])
        self.assertEqual([g.package for g in group_alerts(rows)], ["anyio", "urllib3", "requests", "rand"])


if __name__ == "__main__":
    unittest.main()
