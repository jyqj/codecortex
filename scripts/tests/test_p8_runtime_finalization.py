"""Small real stdio driver controls; no native product/acceptance claims."""
from contextlib import ExitStack, redirect_stderr, redirect_stdout
import hashlib
import io
import json
from pathlib import Path
import sys
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runtime as runtime
import test_p8_runtime as existing_controls


class FinalizationFailureControls(unittest.TestCase):
    prepare_control = existing_controls.RuntimeDriverControls.prepare_control

    def exercise(self, fault=None, oracle_exit=0):
        args, identity = self.prepare_control(oracle_exit)
        original_seal = runtime.seal_output
        original_lstat = Path.lstat
        original_digest = runtime.digest
        snapshot = {}
        output = args.output
        parked = output.with_name("original-output-unavailable")
        calls = []
        raw_names = ("raw.jsonl", "plan.json", "statistics.json", "statistics-replay.json", "parity.json")

        def sealing(directory):
            calls.append(directory)
            snapshot.update({name: (output / name).read_bytes() for name in (*raw_names, "report.json")})
            if fault in ("inventory_enoent", "backup_collision"):
                vanished = output / "project/.index.sqlite3-wal.synthetic-negative-control"
                vanished.write_bytes(b"temporary file disappearing during actual inventory\n")
                if fault == "backup_collision":
                    (output / "report-before-seal-failure.json").write_bytes(b"pre-existing evidence must not be overwritten\n")
                def lstat(path, *a, **kw):
                    if path == vanished:
                        path.unlink()
                        # Exercise the real lstat ENOENT after the directory walk
                        # enumerated this path; do not fake an inventory result.
                    return original_lstat(path, *a, **kw)
                with mock.patch.object(Path, "lstat", lstat):
                    return original_seal(directory)
            if fault == "output_unavailable":
                output.rename(parked)
                return original_seal(directory)
            result = original_seal(directory)
            snapshot["seal.json"] = (output / "seal.json").read_bytes()
            if fault in ("verify_mismatch", "seal_fingerprint_denied"):
                (output / "post-seal-change.txt").write_bytes(b"actual new file invalidates the original inventory\n")
            return result

        def digest(path):
            if fault == "seal_fingerprint_denied" and Path(path) == output / "seal.json":
                raise PermissionError("synthetic denied fingerprint on original failed seal")
            return original_digest(path)

        argv = ["p8_runtime.py", "--binary", str(args.binary), "--oracle", str(args.oracle),
                "--statistics", str(args.statistics), "--build-receipt", str(args.build_receipt),
                "--output", str(output), "--concurrency", str(args.concurrency),
                "--operations", str(args.operations), "--files", str(args.files),
                "--interval-ms", str(args.interval_ms)]
        stdout, stderr = io.StringIO(), io.StringIO()
        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(runtime, "verify_build", return_value=identity))
            stack.enter_context(mock.patch.object(runtime, "verify_receipt", return_value=identity["build_identity"]))
            stack.enter_context(mock.patch.object(runtime, "seal_output", side_effect=sealing))
            stack.enter_context(mock.patch.object(runtime, "digest", side_effect=digest))
            stack.enter_context(mock.patch.object(sys, "argv", argv))
            stack.enter_context(redirect_stdout(stdout)); stack.enter_context(redirect_stderr(stderr))
            code = runtime.main()
        actual = parked if fault == "output_unavailable" else output
        report = json.loads((actual / "report.json").read_text())
        self.assertEqual(len(calls), 1, "a failed sealing attempt must not be retried")
        for name in raw_names:
            self.assertEqual((actual / name).read_bytes(), snapshot[name], name)
        rows = [json.loads(line) for line in (actual / "raw.jsonl").read_text().splitlines()]
        self.assertEqual(sum(row["kind"] == "operation" for row in rows), 60)
        self.assertEqual(json.loads((actual / "statistics.json").read_text())["recorded_samples"], 60)
        self.assertEqual(report["outcomes"], {"success": 60})
        return actual, report, snapshot, code, stdout.getvalue(), stderr.getvalue()

    def assert_failed_finalization(self, fault, phase):
        out, report, before, code, stdout, stderr = self.exercise(fault)
        self.assertEqual(code, 2)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["exit_code"], 2)
        self.assertEqual(report["artifact_seal_status"], "unsealed_finalization_error")
        self.assertIsNone(report["artifact_seal"])
        self.assertEqual(report["finalization_error"]["phase"], phase)
        self.assertEqual(json.loads(stdout), report)
        self.assertEqual((out / "report-before-seal-failure.json").read_bytes(), before["report.json"])
        self.assertEqual(report["pre_seal_report"]["sha256"], hashlib.sha256(before["report.json"]).hexdigest())
        self.assertEqual(report["pre_seal_report"]["bytes"], len(before["report.json"]))
        self.assertEqual(json.loads(before["report.json"])["status"], "passed_observation")
        self.assertEqual(report["latency"]["n"], 60)
        self.assertEqual(report["parity_exit_code"], 0)
        with self.assertRaises((ValueError, FileNotFoundError)):
            runtime.verify_output(out)
        return out, report, before

    def test_inventory_enoent_before_seal_retains_final_failure_and_original_report(self):
        out, report, before = self.assert_failed_finalization("inventory_enoent", "seal_output")
        self.assertIn("FileNotFoundError", report["finalization_error"]["error"])
        self.assertFalse((out / "seal.json").exists())
        self.assertNotIn("failed_artifact_seal", report)

    def test_verify_inventory_mismatch_preserves_original_failed_seal_bytes(self):
        out, report, before = self.assert_failed_finalization("verify_mismatch", "verify_output")
        self.assertEqual((out / "seal.json").read_bytes(), before["seal.json"])
        self.assertEqual(report["failed_artifact_seal"]["sha256"], hashlib.sha256(before["seal.json"]).hexdigest())
        self.assertIn("changed after sealing", report["finalization_error"]["error"])
        self.assertEqual(json.loads(before["seal.json"])["artifact_inventory"]["report.json"]["sha256"],
                         report["pre_seal_report"]["sha256"])

    def test_failed_seal_fingerprint_error_does_not_block_final_failure_report(self):
        out, report, before = self.assert_failed_finalization("seal_fingerprint_denied", "verify_output")
        self.assertEqual((out / "seal.json").read_bytes(), before["seal.json"])
        self.assertEqual(report["failed_artifact_seal"]["metadata_status"], "unavailable")
        self.assertIn("PermissionError", report["failed_artifact_seal"]["error"])
        self.assertNotIn("sha256", report["failed_artifact_seal"])

    def test_successful_stopped_observation_keeps_original_valid_seal(self):
        out, report, _, code, _, _ = self.exercise()
        self.assertEqual(code, 0)
        self.assertEqual(report["status"], "passed_observation")
        self.assertEqual(report["artifact_seal_status"], "sealed")
        self.assertFalse((out / "report-before-seal-failure.json").exists())
        runtime.verify_output(out)

    def test_failed_stopped_observation_remains_exit_one_with_valid_failure_seal(self):
        out, report, _, code, _, _ = self.exercise(oracle_exit=1)
        self.assertEqual(code, 1)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["parity_exit_code"], 1)
        self.assertEqual(report["artifact_seal_status"], "sealed")
        self.assertNotIn("finalization_error", report)
        runtime.verify_output(out)

    def test_lost_output_keeps_cli_two_and_explicit_metadata_failure_stderr(self):
        out, report, before, code, _, stderr = self.exercise("output_unavailable")
        self.assertEqual(code, 2)
        self.assertIn("final status failed/unsealed", stderr)
        self.assertIn("could not preserve/update failure metadata", stderr)
        self.assertEqual((out / "report.json").read_bytes(), before["report.json"])
        self.assertFalse((out / "report-before-seal-failure.json").exists())

    def test_occupied_backup_is_not_overwritten_or_retried(self):
        out, report, before, code, _, stderr = self.exercise("backup_collision")
        self.assertEqual(code, 2)
        self.assertIn("FileExistsError", stderr)
        self.assertIn("final status failed/unsealed", stderr)
        self.assertEqual((out / "report-before-seal-failure.json").read_bytes(), b"pre-existing evidence must not be overwritten\n")
        self.assertEqual((out / "report.json").read_bytes(), before["report.json"])
        self.assertEqual(len(list(out.glob("report-before-seal-failure*"))), 1)


if __name__ == "__main__":
    unittest.main()
