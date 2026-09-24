from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from manifests import ManifestIndex, declared_requirements

PYPROJECT = """
[project]
dependencies = [
    "idna>=3.10",
    "Pillow >= 12.1.1",
    "fastapi[standard]>=0.115",
    "pkg @ git+https://example.com/pkg.git",
    "transformers==5.3.0",
]
[project.optional-dependencies]
gpu = ["torch==2.0; sys_platform == 'linux'"]
[dependency-groups]
dev = ["pytest", {include-group = "lint"}]
lint = ["ruff"]
"""

CARGO = """
[dependencies]
rand = "0.10"
tls = { package = "rustls-webpki", version = "0.103" }
local = { path = "../local" }
[dev-dependencies]
insta = "1"
[target.'cfg(unix)'.dependencies]
nix = "0.29"
"""


class TempRepoTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def write(self, relative: str, text: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path


class DeclaredRequirementsTest(TempRepoTest):
    def test_pyproject_names_and_specifiers_across_all_sections(self):
        requirements = declared_requirements(self.write("pyproject.toml", PYPROJECT))
        self.assertEqual(
            requirements,
            {
                "idna": [">=3.10"],
                "pillow": [">= 12.1.1"],
                "fastapi": [">=0.115"],
                "pkg": [""],
                "transformers": ["==5.3.0"],
                "torch": ["==2.0"],
                "pytest": [""],
                "ruff": [""],
            },
        )

    def test_cargo_uses_real_package_name_and_table_versions(self):
        requirements = declared_requirements(self.write("Cargo.toml", CARGO))
        self.assertEqual(
            requirements,
            {"rand": ["0.10"], "rustls-webpki": ["0.103"], "local": [""], "insta": ["1"], "nix": ["0.29"]},
        )

    def test_package_json(self):
        path = self.write("package.json", '{"dependencies": {"left-pad": "^1.3"}, "devDependencies": {"vitest": "2"}}')
        self.assertEqual(declared_requirements(path), {"left-pad": ["^1.3"], "vitest": ["2"]})

    def test_missing_manifest_declares_nothing(self):
        self.assertEqual(declared_requirements(self.root / "pyproject.toml"), {})


class ManifestIndexTest(TempRepoTest):
    def test_lockfile_alert_is_direct_only_when_declared_next_to_it(self):
        self.write("svc/pyproject.toml", PYPROJECT)
        index = ManifestIndex(self.root)
        alerts = [
            {"package": "Pillow", "manifest": "svc/uv.lock"},
            {"package": "requests", "manifest": "svc/uv.lock"},
            {"package": "idna", "manifest": "other/uv.lock"},
            {"package": "anything", "manifest": "svc/pyproject.toml"},
        ]
        self.assertEqual([index.is_direct(a) for a in alerts], [True, False, False, True])


if __name__ == "__main__":
    unittest.main()
