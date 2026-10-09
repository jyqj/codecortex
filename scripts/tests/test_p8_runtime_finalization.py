"""Real thread protocol controls for finalization; never native product evidence."""
from contextlib import ExitStack
import hashlib
import json
from pathlib import Path
import sys
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runtime as runtime
import test_p8_runtime as existing_controls


class FinalizationFailureControls(unittest.TestCase):
    """Reuse the original G4/8e thread fixture without editing its assertions."""

    def exercise(self, fault=None, oracle_exit=0):
        original_seal = runtime.seal_output
        original_verify = runtime.verify_output
        original_lstat = Path.lstat
        original_digest = runtime.digest
        snapshot, locations, calls = {}, {}, []
        interrupt_case = fault in ("seal_interrupt", "verify_interrupt")
        submitted = []

        class TrackedPool(runtime.ThreadPoolExecutor):
            def submit(self, *args, **kwargs):
                future = super().submit(*args, **kwargs)
                submitted.append(future)
                return future
        raw_names = ("raw.jsonl", "plan.json", "statistics.json", "parity.json")

        def sealing(output):
            calls.append(output)
            locations["output"] = output
            locations["parked"] = output.with_name("original-output-unavailable")
            snapshot.update({name: (output / name).read_bytes() for name in (*raw_names, "report.json")})
            if interrupt_case:
                self.assertTrue(submitted and all(future.done() for future in submitted))
                self.assertEqual([repr(future.exception()) for future in submitted
                                  if not future.cancelled() and future.exception() is not None], [])
            if fault == "seal_interrupt":
                raise KeyboardInterrupt("explicit protocol interrupt during sealing")
            if fault in ("inventory_enoent", "backup_collision"):
                vanished = output / "project/.index.sqlite3-wal.synthetic-negative-control"
                vanished.write_bytes(b"temporary file disappearing during actual inventory\n")
                if fault == "backup_collision":
                    (output / "report-before-seal-failure.json").write_bytes(b"pre-existing evidence must not be overwritten\n")

                def lstat(path, *args, **kwargs):
                    if path == vanished:
                        path.unlink()
                    return original_lstat(path, *args, **kwargs)

                with mock.patch.object(Path, "lstat", lstat):
                    return original_seal(output)
            if fault == "output_unavailable":
                output.rename(locations["parked"])
                return original_seal(output)
            result = original_seal(output)
            snapshot["seal.json"] = (output / "seal.json").read_bytes()
            if fault in ("verify_mismatch", "seal_fingerprint_denied"):
                (output / "post-seal-change.txt").write_bytes(b"actual new file invalidates the original inventory\n")
            return result

        def digest(path):
            if (fault == "seal_fingerprint_denied" and "output" in locations
                    and Path(path) == locations["output"] / "seal.json"):
                raise PermissionError("synthetic denied fingerprint on original failed seal")
            return original_digest(path)

        def verify(path):
            if fault == "verify_interrupt" and Path(path) == locations.get("output"):
                raise KeyboardInterrupt("explicit protocol interrupt during verification")
            return original_verify(path)

        with ExitStack() as stack:
            stack.enter_context(mock.patch.object(runtime, "seal_output", side_effect=sealing))
            stack.enter_context(mock.patch.object(runtime, "digest", side_effect=digest))
            stack.enter_context(mock.patch.object(runtime, "verify_output", side_effect=verify))
            if interrupt_case:
                stack.enter_context(mock.patch.object(runtime, "ThreadPoolExecutor", TrackedPool))
            propagated_interrupt = None
            try:
                state, output = existing_controls.RuntimeFailureLifecycleTests.control(self, oracle_code=oracle_exit)
            except KeyboardInterrupt as error:
                # The original control's finally has already released and
                # waited for its real workers. Convert an old-driver interrupt
                # into a test failure rather than interrupting the test suite.
                propagated_interrupt = repr(error)
            finally:
                if interrupt_case:
                    observation = dict(submitted=len(submitted), done=sum(f.done() for f in submitted),
                                       future_errors=[repr(f.exception()) for f in submitted
                                                      if f.done() and not f.cancelled() and f.exception() is not None],
                                       propagated_interrupt=propagated_interrupt)
                    (locations["output"].parent / "finalization-thread-observation.json").write_text(
                        json.dumps(observation, sort_keys=True, indent=2) + "\n")
                    self.assertTrue(submitted and all(f.done() for f in submitted))
                    self.assertEqual(observation["future_errors"], [])
            if propagated_interrupt is not None:
                self.fail("finalization interrupt escaped without a corrected report: " + propagated_interrupt)
        actual = locations["parked"] if fault == "output_unavailable" else output
        report = json.loads((actual / "report.json").read_text())
        self.assertEqual(len(calls), 1, "a failed sealing attempt must not be retried")
        for name in raw_names:
            self.assertEqual((actual / name).read_bytes(), snapshot[name], name)
        rows = [json.loads(line) for line in (actual / "raw.jsonl").read_text().splitlines()]
        self.assertEqual(sum(row["kind"] == "operation" for row in rows), 60)
        self.assertEqual(json.loads((actual / "statistics.json").read_text())["offered"], 60)
        self.assertEqual(report["outcomes"], {"success": 60})
        return actual, report, snapshot, state

    def assert_failed_finalization(self, fault, phase):
        out, report, before, state = self.exercise(fault)
        self.assertNotIn("raised", state)
        self.assertEqual(state["report"], report)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["exit_code"], 2)
        self.assertEqual(report["artifact_seal_status"], "unsealed_finalization_error")
        self.assertIsNone(report["artifact_seal"])
        self.assertEqual(report["finalization_error"]["phase"], phase)
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
        out, report, _ = self.assert_failed_finalization("inventory_enoent", "seal_output")
        self.assertIn("FileNotFoundError", report["finalization_error"]["error"])
        self.assertFalse((out / "seal.json").exists())
        self.assertNotIn("failed_artifact_seal", report)

    def test_verify_inventory_mismatch_preserves_original_failed_seal_bytes(self):
        out, report, before = self.assert_failed_finalization("verify_mismatch", "verify_output")
        self.assertEqual((out / "seal.json").read_bytes(), before["seal.json"])
        self.assertEqual(report["failed_artifact_seal"]["sha256"], hashlib.sha256(before["seal.json"]).hexdigest())
        self.assertIn("changed after sealing", report["finalization_error"]["error"])
        self.assertEqual(json.loads(before["seal.json"])["artifact_inventory"]["report.json"]["sha256"], report["pre_seal_report"]["sha256"])

    def test_failed_seal_fingerprint_error_does_not_block_final_failure_report(self):
        out, report, before = self.assert_failed_finalization("seal_fingerprint_denied", "verify_output")
        self.assertEqual((out / "seal.json").read_bytes(), before["seal.json"])
        self.assertEqual(report["failed_artifact_seal"]["metadata_status"], "unavailable")
        self.assertIn("PermissionError", report["failed_artifact_seal"]["error"])
        self.assertNotIn("sha256", report["failed_artifact_seal"])

    def test_successful_stopped_observation_keeps_original_valid_seal(self):
        out, report, _, state = self.exercise()
        self.assertNotIn("raised", state)
        self.assertEqual(state["report"], report)
        self.assertEqual(report["exit_code"], 0)
        self.assertEqual(report["status"], "passed_observation")
        self.assertEqual(report["artifact_seal_status"], "sealed")
        self.assertFalse((out / "report-before-seal-failure.json").exists())
        runtime.verify_output(out)

    def test_failed_stopped_observation_remains_exit_one_with_valid_failure_seal(self):
        out, report, _, state = self.exercise(oracle_exit=1)
        self.assertNotIn("raised", state)
        self.assertEqual(state["report"], report)
        self.assertEqual(report["exit_code"], 1)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["parity_exit_code"], 1)
        self.assertEqual(report["artifact_seal_status"], "sealed")
        self.assertNotIn("finalization_error", report)
        runtime.verify_output(out)

    def test_lost_output_keeps_explicit_finalization_error(self):
        out, _, before, state = self.exercise("output_unavailable")
        self.assertIn("final status failed/unsealed", state["raised"])
        self.assertIn("could not preserve/update failure metadata", state["raised"])
        self.assertEqual((out / "report.json").read_bytes(), before["report.json"])
        self.assertFalse((out / "report-before-seal-failure.json").exists())

    def test_occupied_backup_is_not_overwritten_or_retried(self):
        out, _, before, state = self.exercise("backup_collision")
        self.assertIn("FileExistsError", state["raised"])
        self.assertIn("final status failed/unsealed", state["raised"])
        self.assertEqual((out / "report-before-seal-failure.json").read_bytes(), b"pre-existing evidence must not be overwritten\n")
        self.assertEqual((out / "report.json").read_bytes(), before["report.json"])
        self.assertEqual(len(list(out.glob("report-before-seal-failure*"))), 1)

    def test_keyboard_interrupt_in_seal_or_verify_keeps_failed_report_after_workers_finish(self):
        for fault, phase in (("seal_interrupt", "seal_output"), ("verify_interrupt", "verify_output")):
            with self.subTest(fault=fault):
                out, report, before = self.assert_failed_finalization(fault, phase)
                self.assertTrue(report["interrupted"])
                self.assertIn("KeyboardInterrupt", report["finalization_error"]["error"])
                if fault == "seal_interrupt":
                    self.assertFalse((out / "seal.json").exists())
                else:
                    self.assertEqual((out / "seal.json").read_bytes(), before["seal.json"])


if __name__ == "__main__":
    unittest.main()
