"""Command-profile controls; fake failed build is never a build-success witness."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p7_stdio_build_receipt as builder


class BuildProfileTests(unittest.TestCase):
    def test_default_and_release_record_the_actual_cargo_invocation_even_on_failure(self):
        for release in (False, True):
            with self.subTest(release=release), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                output = root / "evidence"
                observed = []
                def failed(command, **kwargs):
                    observed.append(list(command))
                    return subprocess.CompletedProcess(command, 17)
                snapshot = {"inputs": [], "input_count": 0, "source_commit": "a" * 40}
                with mock.patch.object(builder, "source_snapshot", return_value=snapshot), \
                     mock.patch.object(builder, "compiler_environment", return_value=({}, {})), \
                     mock.patch.object(builder, "toolchain_identity", return_value={"fixture": True}), \
                     mock.patch.object(builder.subprocess, "run", side_effect=failed):
                    code = builder.build(root, output, "default", False, release=release)
                self.assertEqual(code, 17)
                self.assertEqual(len(observed), 1)
                self.assertEqual(observed[0].count("--release"), int(release))
                self.assertIn("--no-default-features", observed[0])
                self.assertIn("--locked", observed[0])
                self.assertFalse((output / "build-receipt.json").exists())
                self.assertTrue((output / "build-failure.json").is_file())

    def test_cli_release_is_explicit_and_default_is_unchanged(self):
        with tempfile.TemporaryDirectory() as temporary:
            for flag in ([], ["--release"]):
                argv = ["p7_stdio_build_receipt.py", "--package-kind", "default",
                        "--output-dir", temporary] + flag
                with mock.patch.object(sys, "argv", argv), mock.patch.object(builder, "build", return_value=0) as build:
                    self.assertEqual(builder.main(), 0)
                    self.assertEqual(build.call_args.kwargs, {"release": bool(flag)})


if __name__ == "__main__":
    unittest.main()
