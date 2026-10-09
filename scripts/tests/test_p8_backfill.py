"""Backfill provenance/failure controls; these fixtures never run a product test."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_backfill as backfill
import p8_runtime_build as evidence
from test_p8_runtime_evidence import ReceiptFixture


class BackfillBuildTests(unittest.TestCase):
    def test_reused_or_foreign_target_test_executable_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = ReceiptFixture(Path(temporary))
            source = fixture.root / "crates/cc-eval/tests/p7_worker_contention.rs"
            source.parent.mkdir()
            source.write_text("// provenance control only\n")
            executable = fixture.target / "p7_worker_contention-control"
            executable.write_bytes(b"control only; never executed\n")
            row = {"reason": "compiler-artifact", "manifest_path": str(fixture.root / "crates/cc-eval/Cargo.toml"),
                   "target": {"name": "p7_worker_contention", "kind": ["test"], "src_path": str(source)},
                   "profile": {"test": True, "opt_level": "3", "debug_assertions": False},
                   "features": ["semantic"], "executable": str(executable), "fresh": False}
            finished = {"reason": "build-finished", "success": True}
            self.assertEqual(backfill.artifact([row, finished], fixture.root, fixture.target), row)
            for change, error in (({"features": ["default", "semantic"]}, "actual release semantic test profile"),
                                  ({"fresh": True}, "was reused"),
                                  ({"executable": str(fixture.out / "codecortex")}, "outside its private target")):
                with self.subTest(change=change), self.assertRaisesRegex(ValueError, error):
                    backfill.artifact([{**row, **change}, finished], fixture.root, fixture.target)

    def test_failed_build_uses_private_target_and_retains_sealed_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            fixture = ReceiptFixture(directory)
            output, target = directory / "failed-backfill", directory / "fresh-backfill-target"
            observed = []
            tools = copy.deepcopy(fixture.receipt["toolchain_before"])
            selected = tools["rustc"]["invocation"]
            overrides = {"RUSTC": selected, "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": ""}

            def failed(command, **kwargs):
                observed.append((command, kwargs["env"]))
                kwargs["stderr"].write(b"intentional build failure control\n")
                return subprocess.CompletedProcess(command, 17)

            with mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": str(target)}), \
                    mock.patch.object(evidence, "compiler_environment", return_value=(dict(overrides), overrides)), \
                    mock.patch.object(backfill, "source_snapshot", return_value=fixture.receipt["source_before"]), \
                    mock.patch.object(backfill, "observer_snapshot", return_value=fixture.receipt["observer_before"]), \
                    mock.patch.object(backfill, "toolchain_identity", return_value=tools), \
                    mock.patch.object(backfill.subprocess, "run", side_effect=failed):
                receipt = backfill.execute(fixture.root, output)
            self.assertEqual(receipt["status"], "failed")
            self.assertEqual(receipt["build_exit_code"], 17)
            self.assertNotEqual(receipt["exit_code"], 0)
            self.assertEqual(len(observed), 1)
            command, environment = observed[0]
            self.assertIn("--no-default-features", command)
            self.assertEqual(command[command.index("--features") + 1], "semantic")
            self.assertEqual(command[-2:], ["--target-dir", str(target)])
            self.assertEqual(environment["CARGO_TARGET_DIR"], str(target))
            self.assertEqual(environment["CARGO_BUILD_BUILD_DIR"], str(target))
            self.assertEqual(environment["RUSTC"], selected)
            evidence.verify_output(output)
            self.assertEqual(json.loads((output / "receipt.json").read_text())["build_exit_code"], 17)
            self.assertFalse((output / "p7_worker_contention").exists())
            (output / "build.stderr").write_text("rewritten failure\n")
            with self.assertRaisesRegex(ValueError, "inventory changed"):
                evidence.verify_output(output)


if __name__ == "__main__":
    unittest.main()
