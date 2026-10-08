"""Real path admission controls; no fixture is treated as a successful build."""
from argparse import Namespace
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_external_candidate as packager


class BuildValidationReached(Exception):
    """Stop after real path admission, before any build or package claim."""


class ExternalCandidatePathTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="p8-package-path-controls-")
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.source = self.base / "source"
        self.product = self.base / "product"
        self.runner = self.base / "runner"
        self.product.mkdir()
        self.runner.mkdir()
        origin = Path(packager.__file__).resolve().parents[1]
        names = packager.EXTRA_HELPERS + (
            "scripts/p8_release_evidence.py", packager.TEMPLATE, packager.RECOVERY)
        for name in names:
            destination = self.source / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(origin / name, destination)
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                 "commit", "-qm", "path controls only; not a product candidate")
        self.commit = self.git("rev-parse", "HEAD").decode().strip()

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.source), *args],
                                       stderr=subprocess.DEVNULL)

    def invoke(self, output, *, product=None, runner=None):
        args = Namespace(source_root=self.source, expected_source=self.commit,
                         product_build=product or self.product,
                         runner_build=runner or self.runner, output=output)
        with mock.patch.object(packager, "__file__",
                               str(self.source / "scripts/p8_external_candidate.py")), \
             mock.patch.object(packager.importlib, "import_module",
                               side_effect=BuildValidationReached):
            return packager.package(args)

    def test_output_parent_symlinks_are_rejected_before_any_creation(self):
        for protected in (self.source, self.product, self.runner):
            with self.subTest(protected=protected.name):
                alias = self.base / (protected.name + "-alias")
                alias.symlink_to(protected, target_is_directory=True)
                with self.assertRaisesRegex(ValueError, "symlink"):
                    self.invoke(alias / "must-not-exist")
                self.assertFalse((protected / "must-not-exist").exists())
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), b"")

    def test_parent_traversal_cannot_enter_source_or_build(self):
        spare = self.base / "spare"
        spare.mkdir()
        for protected in (self.source, self.product, self.runner):
            with self.subTest(protected=protected.name):
                output = spare / ".." / protected.name / "must-not-exist"
                with self.assertRaisesRegex(ValueError, "traversal"):
                    self.invoke(output)
                self.assertFalse((protected / "must-not-exist").exists())
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), b"")

    def test_build_directory_aliases_are_rejected_before_new_output(self):
        for role in ("product", "runner"):
            with self.subTest(role=role):
                alias = self.base / (role + "-alias")
                alias.symlink_to(getattr(self, role), target_is_directory=True)
                output = self.base / (role + "-output")
                with self.assertRaisesRegex(ValueError, "symlink"):
                    self.invoke(output, **{role: alias})
                self.assertFalse(output.exists())

    def test_existing_output_and_direct_overlaps_are_rejected(self):
        for protected in (self.source, self.product, self.runner):
            with self.subTest(protected=protected.name):
                with self.assertRaisesRegex(ValueError, "disjoint"):
                    self.invoke(protected / "must-not-exist")
                self.assertFalse((protected / "must-not-exist").exists())
        existing = self.base / "prior-output"
        existing.mkdir()
        marker = existing / "prior.txt"
        marker.write_bytes(b"preserved prior evidence\n")
        with self.assertRaisesRegex(ValueError, "new output required"):
            self.invoke(existing)
        self.assertEqual(marker.read_bytes(), b"preserved prior evidence\n")
        self.assertEqual(list(existing.iterdir()), [marker])

    def test_disjoint_new_directory_reaches_build_validation(self):
        output = self.base / "new-parent" / "output"
        with self.assertRaises(BuildValidationReached):
            self.invoke(output)
        self.assertTrue(output.is_dir())
        self.assertEqual(list(output.iterdir()), [])
        self.assertEqual(list(self.product.iterdir()), [])
        self.assertEqual(list(self.runner.iterdir()), [])
        self.assertEqual(self.git("status", "--porcelain", "--untracked-files=all"), b"")


if __name__ == "__main__":
    unittest.main()
