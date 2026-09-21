from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from manifests import DirectDependencyResolver, declared_packages
from policy import classify

PYPROJECT = """
[project]
dependencies = ["idna>=3.10", "Pillow >= 12.1.1", "fastapi[standard]>=0.115", "pkg @ git+https://example.com/pkg.git"]
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
[dev-dependencies]
insta = "1"
[target.'cfg(unix)'.dependencies]
nix = "0.29"
"""


class DeclaredPackagesTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def write(self, relative: str, text: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def test_pyproject_names_are_normalized_across_all_sections(self):
        names = declared_packages(self.write("pyproject.toml", PYPROJECT))
        self.assertEqual(names, {"idna", "pillow", "fastapi", "pkg", "torch", "pytest", "ruff"})

    def test_cargo_uses_real_package_name_for_renamed_deps(self):
        names = declared_packages(self.write("Cargo.toml", CARGO))
        self.assertEqual(names, {"rand", "rustls-webpki", "insta", "nix"})

    def test_package_json(self):
        path = self.write("package.json", '{"dependencies": {"left-pad": "1"}, "devDependencies": {"vitest": "2"}}')
        self.assertEqual(declared_packages(path), {"left-pad", "vitest"})

    def test_missing_manifest_declares_nothing(self):
        self.assertEqual(declared_packages(self.root / "pyproject.toml"), set())

    def test_lockfile_alert_is_direct_only_when_declared_next_to_it(self):
        self.write("svc/pyproject.toml", PYPROJECT)
        resolver = DirectDependencyResolver(self.root)
        alert = {"severity": "medium", "patched": "1", "ecosystem": "pip"}
        rows = classify(
            [
                alert | {"package": "Pillow", "manifest": "svc/uv.lock"},
                alert | {"package": "requests", "manifest": "svc/uv.lock"},
                alert | {"package": "idna", "manifest": "other/uv.lock"},
                alert | {"package": "anything", "manifest": "svc/pyproject.toml"},
            ],
            resolver,
        )
        self.assertEqual([r["direct"] for r in rows], [True, False, False, True])
        self.assertEqual([r["decision"] for r in rows], ["MustMerge", "Review", "Review", "MustMerge"])


if __name__ == "__main__":
    unittest.main()
