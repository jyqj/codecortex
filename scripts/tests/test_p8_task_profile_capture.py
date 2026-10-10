"""Controls for new nested paths, module isolation and actual child outcomes."""
import hashlib
import importlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_scale_diagnostic_capture as original
import p8_task_profile_capture as task


class TaskProfileCapture(unittest.TestCase):
    def put(self, root, relative, value):
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def successful_fixture(self, root):
        self.put(root, "observer-before.json", {"source": "fixed-test-source"})
        self.put(root, "observer-after.json", {"source": "fixed-test-source"})
        self.put(root, "shard/task-shard.json",
                 dict(schema=task.SCHEMA, stage_scope=task.SCOPE, status="passed", passed=True))
        self.put(root, "shard/native-shard/shard.json", dict(status="passed", exit_code=0))
        report = self.put(root, "shard/native-shard/native/report.json",
                          dict(status="measurement_complete", exit_code=0, worker_exit_code=0))
        self.put(root, "supervisor-terminal.json",
                 dict(driver_returncode=0, native_report_sha256=task.core.sha(report),
                      native_report_status="measurement_complete", native_exit_code=0,
                      wrapper_timeout=False))

    def test_nested_partial_bytes_and_original_module_are_preserved(self):
        original_streams = original.STREAMS
        original_observers = original.OBSERVERS
        importlib.reload(task)
        self.assertIsNot(task.core, original)
        self.assertEqual(original.STREAMS, original_streams)
        self.assertEqual(original.OBSERVERS, original_observers)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.put(root, "configuration.json", {"task_protocol": task.SCHEMA})
            stream = root / "shard/native-shard/native/raw.jsonl"
            stream.parent.mkdir(parents=True)
            raw = b'{"event":"unfinished"'
            stream.write_bytes(raw)
            bundle, manifest = task.core.snapshot(root, 0)
            self.assertFalse(manifest["native_EOF_claimed"])
            self.assertEqual(len(manifest["all_captured_chunks"]), 1)
            chunk = manifest["all_captured_chunks"][0]
            self.assertEqual(chunk["source"], "shard/native-shard/native/raw.jsonl")
            self.assertEqual((chunk["start"], chunk["end"]), (0, len(raw)))
            self.assertEqual((bundle / chunk["name"]).read_bytes(), raw)
            self.assertEqual(chunk["sha256"], hashlib.sha256(raw).hexdigest())
            self.assertEqual(task.verdict(root), 1)

    def test_real_driver_failure_is_retained_without_native_success(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.put(root, "observer-before.json", {})
            self.put(root, "observer-after.json", {})
            result = task.supervise(root, [sys.executable, "-c", "print('original child'); raise SystemExit(7)"],
                                    wall_seconds=10)
            self.assertEqual(result["driver_returncode"], 7)
            self.assertIsNone(result["native_exit_code"])
            self.assertFalse(result["wrapper_timeout"])
            self.assertEqual((root / "driver.stdout").read_text(), "original child\n")
            self.assertEqual(task.verdict(root), 7)

    def test_complete_nested_native_receipt_is_required(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.successful_fixture(root)
            self.assertEqual(task.verdict(root), 0)
            report = root / "shard/native-shard/native/report.json"
            report.write_text('{"status":"measurement_complete","exit_code":3}')
            self.assertEqual(task.verdict(root), 1)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.successful_fixture(root)
            (root / "shard/task-shard.json").unlink()
            self.assertEqual(task.verdict(root), 1)
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.successful_fixture(root)
            self.put(root, "observer-after.json", {"source": "different-source"})
            self.assertEqual(task.verdict(root), 1)


if __name__ == "__main__":
    unittest.main()
