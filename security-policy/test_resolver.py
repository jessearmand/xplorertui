from __future__ import annotations

import subprocess
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from resolver import Reach, ResolverProbe, parse_changes, reach_of

CARGO_OUTPUT = """    Updating crates.io index
     Locking 3 packages to latest Rust 1.93.1 compatible versions
    Updating openssl v0.10.75 -> v0.10.81
    Updating rand v0.8.5 -> v0.8.8
warning: not updating lockfile due to dry run
"""

UV_OUTPUT = """Resolved 88 packages in 1.43s
Update anyio v4.12.1 -> v4.14.2
Update Pydantic_Settings v2.13.1 -> v2.14.2
"""


# Cargo reports a package locked at several versions as removals and additions.
CARGO_MULTI_VERSION = """    Removing rand v0.8.5
    Removing rand v0.9.2
    Removing rand v0.10.0
      Adding rand v0.8.8
      Adding rand v0.10.3
"""


class ParseChangesTest(unittest.TestCase):
    def test_cargo(self):
        self.assertEqual(
            parse_changes(CARGO_OUTPUT, "rust").updated,
            {("openssl", "0.10.75"): "0.10.81", ("rand", "0.8.5"): "0.8.8"},
        )

    def test_uv_names_are_normalized(self):
        self.assertEqual(
            parse_changes(UV_OUTPUT, "pip").updated,
            {("anyio", "4.12.1"): "4.14.2", ("pydantic-settings", "2.13.1"): "2.14.2"},
        )

    def test_removed_copies_are_matched_to_their_successor_line(self):
        changes = parse_changes(CARGO_MULTI_VERSION, "rust")
        self.assertEqual(reach_of(("rand", "0.8.5"), changes, "rust"), Reach("0.8.8"))
        self.assertEqual(reach_of(("rand", "0.10.0"), changes, "rust"), Reach("0.10.3"))
        dropped = reach_of(("rand", "0.9.2"), changes, "rust")
        self.assertTrue(dropped.removed and dropped.version is None)


class ResolverProbeTest(unittest.TestCase):
    def probe(self, run, lockfile="Cargo.lock", packages=(("openssl", "0.10.75"), ("rand", "0.10.0"))):
        with mock.patch("resolver.shutil.which", return_value="/usr/bin/tool"):
            return ResolverProbe(run).probe(Path("repo") / lockfile, list(packages))

    def test_unmentioned_package_stays_at_its_locked_version(self):
        run = mock.Mock(return_value=SimpleNamespace(returncode=0, stdout="", stderr=CARGO_OUTPUT))
        self.assertEqual(
            self.probe(run), {("openssl", "0.10.75"): Reach("0.10.81"), ("rand", "0.10.0"): Reach("0.10.0")}
        )
        command = run.call_args.args[0]
        self.assertEqual(command, ["cargo", "update", "--dry-run", "-p", "openssl@0.10.75", "-p", "rand@0.10.0"])
        self.assertEqual(run.call_args.kwargs["cwd"], Path("repo"))

    def test_failure_timeout_and_missing_tool_are_unknown(self):
        failed = mock.Mock(return_value=SimpleNamespace(returncode=101, stdout="", stderr="error: no network\n"))
        timed_out = mock.Mock(side_effect=subprocess.TimeoutExpired("cargo", 1))
        for run in (failed, timed_out):
            self.assertTrue(all(reach.version is None and reach.reason for reach in self.probe(run).values()))
        with mock.patch("resolver.shutil.which", return_value=None):
            answers = ResolverProbe(failed).probe(Path("Cargo.lock"), [("rand", "0.10.0")])
        self.assertEqual(answers[("rand", "0.10.0")], Reach(None, "`cargo` is not installed"))

    def test_lockfile_without_a_resolver_is_unknown(self):
        answers = self.probe(mock.Mock(), lockfile="yarn.lock", packages=[("left-pad", "1.0.0")])
        self.assertEqual(answers[("left-pad", "1.0.0")], Reach(None, "no resolver for yarn.lock"))


if __name__ == "__main__":
    unittest.main()
