"""Command-profile controls; fake failed build is never a build-success witness."""
import copy
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p7_stdio_build_receipt as builder
import p8_candidate_execution as candidate


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

    def test_release_rejects_unoptimized_or_mislabeled_actual_profile(self):
        valid = {"opt_level": "3", "debug_assertions": False, "test": False}
        builder.verify_release_profile(valid)
        for changed in ({"opt_level": "0"}, {"opt_level": 3}, {"debug_assertions": True},
                        {"debug_assertions": 0}, {"test": True}, {"test": 0}):
            with self.subTest(changed=changed), self.assertRaisesRegex(ValueError, "not optimized"):
                builder.verify_release_profile({**valid, **changed})
        for missing in valid:
            incomplete = dict(valid)
            del incomplete[missing]
            with self.subTest(missing=missing), self.assertRaisesRegex(ValueError, "not optimized"):
                builder.verify_release_profile(incomplete)

    def test_candidate_release_validator_rejects_mismatch_and_checks_retained_cargo(self):
        root = Path("/fixed-candidate")
        profile = {"opt_level": "3", "debug_assertions": False, "test": False}
        artifact = {"reason": "compiler-artifact", "features": [], "profile": profile,
                    "package_id": "path+file:///fixed-candidate/crates/cc-eval#0.1.0",
                    "manifest_path": str(root / "crates/cc-eval/Cargo.toml"),
                    "target": {"name": "cc-eval", "kind": ["bin"],
                               "src_path": str(root / "crates/cc-eval/src/bin/cc-eval.rs")}}
        source = {"source_commit": "a" * 40}
        inputs = {"Cargo.toml": "1" * 64}
        receipt = {"build_exit_code": 0, "source_before": source, "source_after": source,
                   "binary_sha256": "2" * 64, "cargo_artifact": artifact,
                   "build_profile": "release", "actual_cargo_profile": dict(profile),
                   "build_command": ["cargo", "build", "--locked", "--no-default-features",
                                     "--bin", "cc-eval", "--release"],
                   "source_manifest": "source-inputs.json", "toolchain": {"fixture": True}}
        active = copy.deepcopy(receipt)
        def read(path):
            return active if Path(path).name == "build-receipt.json" else inputs
        with mock.patch.object(candidate.lock, "read_json", side_effect=read), \
             mock.patch.object(candidate, "file_sha256", return_value="2" * 64), \
             mock.patch.object(candidate, "source_snapshot", return_value={"inputs": inputs}), \
             mock.patch.object(candidate, "artifact_from_log", return_value=active["cargo_artifact"]) as raw:
            result = candidate.validate_build(root / "cc-eval", root / "build-receipt.json",
                                              root, source, "runner")
            self.assertEqual(result["receipt"], active)
            raw.assert_called_once()
            raw.reset_mock()
            active["actual_cargo_profile"]["opt_level"] = "0"
            with self.assertRaisesRegex(ValueError, "differs from Cargo"):
                candidate.validate_build(root / "cc-eval", root / "build-receipt.json",
                                         root, source, "runner")
            raw.assert_not_called()
            active["cargo_artifact"]["profile"]["opt_level"] = "0"
            with self.assertRaisesRegex(ValueError, "not optimized"):
                candidate.validate_build(root / "cc-eval", root / "build-receipt.json",
                                         root, source, "runner")
            raw.assert_not_called()

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
