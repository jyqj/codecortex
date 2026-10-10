"""Harmless subprocess and byte-capture controls; no product executable or study."""
import importlib.util
import json
import hashlib
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "diagnostic_capture", Path(__file__).parents[1] / "p8_scale_diagnostic_capture.py")
capture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(capture)


class CaptureControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        capture.write_new(self.root / "configuration.json", {"source": "fake-harmless-fixture"})
        capture.write_new(self.root / "observer-before.json", {"source": "fake-harmless-fixture"})
        capture.write_new(self.root / "observer-after.json", {"source": "fake-harmless-fixture"})

    def tearDown(self):
        self.temporary.cleanup()

    def command(self, text):
        return [sys.executable, "-c", text]

    def write(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def test_complete_typed_success_requires_original_native_and_driver_records(self):
        self.write("shard/native/report.json", b'{"status":"measurement_complete","exit_code":0,"worker_exit_code":0}')
        self.write("shard/shard.json", b'{"status":"passed","exit_code":0}')
        result = capture.supervise(self.root, self.command("print('harmless')"), wall_seconds=5)
        self.assertEqual(result["driver_returncode"], 0)
        self.assertEqual(result["native_worker_exit_code"], 0)
        self.assertEqual(capture.verdict(self.root), 0)

    def test_driver_zero_without_native_terminal_is_not_native_success(self):
        result = capture.supervise(self.root, self.command("pass"), wall_seconds=5)
        self.assertIsNone(result["native_exit_code"])
        self.assertEqual(capture.verdict(self.root), 1)

    def test_failure_keeps_real_exit_stderr_and_available_prefix(self):
        self.write("shard/native/raw.jsonl", b'{"event":"started"}\n{"unfinished":')
        result = capture.supervise(self.root, self.command("import sys;sys.stderr.write('fake failure\\n');sys.exit(7)"), wall_seconds=5)
        self.assertEqual(result["driver_returncode"], 7)
        self.assertEqual(capture.verdict(self.root), 7)
        _, manifest = capture.snapshot(self.root, 0)
        self.assertTrue(manifest["supervisor_terminal_observed"])
        self.assertFalse(manifest["native_EOF_claimed"])
        self.assertIn(b"fake failure", (self.root / "driver.stderr").read_bytes())

    @unittest.skipUnless(os.name == "posix", "signal receipt is POSIX-specific")
    def test_real_signal_is_not_invented_native_exit(self):
        result = capture.supervise(self.root, self.command("import os,signal;os.kill(os.getpid(),signal.SIGTERM)"), wall_seconds=5)
        self.assertEqual(result["driver_returncode"], -signal.SIGTERM)
        self.assertEqual(result["driver_signal"], signal.SIGTERM)
        self.assertIsNone(result["native_exit_code"])
        self.assertNotEqual(capture.verdict(self.root), 0)

    def test_finite_outer_timeout_never_claims_native_group_cleanup(self):
        result = capture.supervise(self.root, self.command("import time;time.sleep(5)"), wall_seconds=0.05)
        self.assertTrue(result["wrapper_timeout"])
        self.assertNotEqual(result["driver_returncode"], 0)
        self.assertIn("not independently established", result["native_process_cleanup"])
        self.assertNotEqual(capture.verdict(self.root), 0)

    def test_upload_ack_and_retry_preserve_exact_partial_line_bytes(self):
        original = b'{"a":1}\n{"part":'
        raw = self.write("shard/native/raw.jsonl", original)
        _, first = capture.snapshot(self.root, 0)
        self.assertEqual(first["acknowledged_prefix_bytes_before_this_upload"]["shard/native/raw.jsonl"], 0)
        raw.write_bytes(original + b'2}\n')
        _, second = capture.snapshot(self.root, 1)  # No upload result: no acknowledged custody.
        self.assertEqual(len(second["included_chunk_names"]), 2)
        self.assertEqual(second["acknowledged_prefix_bytes_before_this_upload"]["shard/native/raw.jsonl"], 0)
        _, third = capture.snapshot(self.root, 2, previous_id="1234", previous_digest="a" * 64)
        self.assertEqual(third["included_chunk_names"], [])
        self.assertEqual(third["acknowledged_prefix_bytes_before_this_upload"]["shard/native/raw.jsonl"], len(original) + 3)
        rows = sorted(third["all_captured_chunks"], key=lambda row: row["start"])
        recovered = b"".join((self.root / "chunks" / row["name"]).read_bytes() for row in rows)
        self.assertEqual(recovered, original + b'2}\n')
        self.assertFalse(third["native_EOF_claimed"])

    def test_truncation_preserves_old_bytes_and_explicit_fault(self):
        raw = self.write("shard/native/raw.jsonl", b"original-prefix")
        _, first = capture.snapshot(self.root, 0)
        raw.write_bytes(b"short")
        _, second = capture.snapshot(self.root, 1)
        self.assertEqual(second["capture_faults"][0]["reason"], "stream_replaced_or_truncated")
        name = first["included_chunk_names"][0]
        self.assertEqual((self.root / "chunks" / name).read_bytes(), b"original-prefix")
        self.assertEqual(second["all_captured_chunks"], first["all_captured_chunks"])

    def test_no_ack_from_missing_id_or_invalid_digest(self):
        self.write("shard/native/raw.jsonl", b"bytes")
        capture.snapshot(self.root, 0)
        with self.assertRaisesRegex(ValueError, "invalid upload digest"):
            capture.snapshot(self.root, 1, previous_id="1", previous_digest="not-a-digest")
        state = capture.read_json(self.root / "capture-state.json")
        self.assertEqual(state["acknowledged_chunks"], [])

    def test_checkpoint_clock_is_anchored_not_extended_by_upload_duration(self):
        (self.root / "configuration.json").write_text(json.dumps({
            "started_monotonic": 100, "outer_job_seconds": capture.JOB_SECONDS}))
        with mock.patch.object(capture, "wait_until", return_value=False) as wait, \
             mock.patch.object(capture, "snapshot", return_value=(self.root, {})), \
             mock.patch.object(capture, "emit_outputs"):
            capture.checkpoint(self.root, 3)
            wait.assert_called_once_with(self.root, 100 + 4 * 900)
        # Final preservation waits for the original driver's tail; checkpoint 20
        # is not a command to kill the child at the nominal five-hour boundary.
        with mock.patch.object(capture, "wait_until", return_value=True) as wait, \
             mock.patch.object(capture, "snapshot", return_value=(self.root, {})), \
             mock.patch.object(capture, "observer_after", return_value=True), \
             mock.patch.object(capture, "emit_outputs"):
            capture.checkpoint(self.root, 20, final=True)
            wait.assert_called_once_with(self.root, 100 + capture.JOB_SECONDS)
            self.assertGreater(capture.JOB_SECONDS, 18000 + 120)

    def test_early_terminal_and_already_elapsed_interval_do_not_sleep(self):
        (self.root / "configuration.json").write_text(json.dumps({
            "started_monotonic": 100, "outer_job_seconds": capture.JOB_SECONDS}))
        with mock.patch.object(capture.time, "monotonic", return_value=2000), \
             mock.patch.object(capture.time, "sleep") as sleep:
            self.assertFalse(capture.wait_until(self.root, 1000))
            sleep.assert_not_called()
        capture.write_new(self.root / "supervisor-terminal.json", {"driver_returncode": 7})
        with mock.patch.object(capture.time, "sleep") as sleep:
            self.assertTrue(capture.wait_until(self.root, 100000))
            sleep.assert_not_called()

    def test_receipt_publication_does_not_replace_prior_terminal(self):
        path = self.root / "supervisor-terminal.json"
        capture.write_new(path, {"driver_returncode": 7})
        original = path.read_bytes()
        with self.assertRaises(FileExistsError):
            capture.write_new(path, {"driver_returncode": 0})
        self.assertEqual(path.read_bytes(), original)

    def test_standalone_supervisor_keeps_receipt_after_launcher_returns(self):
        (self.root / "configuration.json").write_text(json.dumps({
            "command": self.command("import time,sys;time.sleep(0.03);sys.exit(9)")}))
        with (self.root / "wrapper.stdout").open("xb") as stdout, \
             (self.root / "wrapper.stderr").open("xb") as stderr:
            parent = subprocess.Popen(
                [sys.executable, str(Path(capture.__file__).resolve()), "supervise", "--directory", str(self.root)],
                stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, start_new_session=True)
        self.assertEqual(parent.wait(timeout=5), 0)
        terminal = capture.read_json(self.root / "supervisor-terminal.json")
        self.assertEqual(terminal["driver_returncode"], 9)
        self.assertEqual(capture.verdict(self.root), 9)

    def test_exact_observer_bytes_and_mode_bind_to_git_then_reject_mutation(self):
        expected = {}
        for relative in capture.OBSERVERS:
            path = self.write(relative, ("original:" + relative).encode())
            path.chmod(0o644)
            data = path.read_bytes()
            expected[relative] = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        def git(command, **kwargs):
            if command[3] == "rev-parse":
                return SimpleNamespace(stdout="a" * 40 + "\n")
            relative = command[-1]
            return SimpleNamespace(stdout=f"100644 blob {expected[relative]}\t{relative}\n")
        with mock.patch.object(capture.subprocess, "run", side_effect=git):
            snap = capture.observer_snapshot(self.root)
            self.assertEqual(snap["source"], "a" * 40)
            self.assertEqual(set(snap["inputs"]), set(capture.OBSERVERS))
            self.write(capture.OBSERVERS[1], b"different wrapper")
            with self.assertRaisesRegex(ValueError, "observer bytes differ"):
                capture.observer_snapshot(self.root)

    def test_changed_observer_cannot_be_a_successful_diagnostic(self):
        self.write("shard/native/report.json", b'{"status":"measurement_complete","exit_code":0,"worker_exit_code":0}')
        self.write("shard/shard.json", b'{"status":"passed","exit_code":0}')
        capture.supervise(self.root, self.command("pass"), wall_seconds=5)
        self.assertEqual(capture.verdict(self.root), 0)
        (self.root / "observer-after.json").write_text('{"source":"different"}')
        self.assertNotEqual(capture.verdict(self.root), 0)

    def test_wrapper_traceback_and_stdout_are_captured_without_terminal_receipt(self):
        streams = {
            "wrapper.stdout": b"supervisor diagnostic\nunfinished ",
            "wrapper.stderr": b"Traceback (most recent call last):\nraw-byte:\xff\x00",
        }
        for relative, data in streams.items():
            self.write(relative, data)
        bundle, manifest = capture.snapshot(self.root, 0)
        self.assertFalse(manifest["supervisor_terminal_observed"])
        self.assertFalse(manifest["native_EOF_claimed"])
        for relative, expected in streams.items():
            chunks = [row for row in manifest["all_captured_chunks"] if row["source"] == relative]
            self.assertEqual(len(chunks), 1)
            self.assertEqual((bundle / chunks[0]["name"]).read_bytes(), expected)
            self.assertEqual(chunks[0]["sha256"], hashlib.sha256(expected).hexdigest())
            self.assertEqual(manifest["acknowledged_prefix_bytes_before_this_upload"][relative], 0)
        self.assertNotEqual(capture.verdict(self.root), 0)


if __name__ == "__main__":
    unittest.main()
