import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runtime as runtime


class RuntimeEvidenceTests(unittest.TestCase):
    def test_exact_public_hit_rejects_query_echo_and_wrong_path(self):
        runtime.require_stable_symbol([{"name": runtime.QUERY, "file_path": "stable.py"}])
        for response in ({"query": runtime.QUERY}, [{"query": runtime.QUERY}],
                         [{"name": runtime.QUERY, "file_path": "wrong.py"}],
                         [{"name": "wrong", "file_path": "stable.py"}]):
            with self.assertRaises((AssertionError, RuntimeError, ValueError)):
                runtime.require_stable_symbol(response)

    def test_clustered_rss_cannot_certify_an_hour(self):
        second = 1_000_000_000
        complete = [{"at_ns": i * second} for i in range(3601)]
        self.assertTrue(runtime.sample_coverage(complete, 0, 3600 * second)["passed"])
        self.assertFalse(runtime.sample_coverage(complete[:10], 0, 3600 * second)["passed"])
        self.assertFalse(runtime.sample_coverage(complete[10:], 0, 3600 * second)["passed"])
        self.assertFalse(runtime.sample_coverage(complete[:10] + complete[20:], 0, 3600 * second)["passed"])
        self.assertFalse(runtime.sample_coverage(complete[::-1], 0, 3600 * second)["passed"])

    def test_zero_or_missing_rss_cannot_pass(self):
        for value in (None, 0, -1, True):
            result = runtime.rss_trend([{"server": {"resident_bytes": value}}] * 12)
            self.assertFalse(result["passed"])
            self.assertEqual(result["status"], "unavailable")

    def test_existing_memory_threshold_is_preserved(self):
        mib = 1024 * 1024
        samples = [{"server": {"resident_bytes": 100 * mib}}] * 12
        self.assertTrue(runtime.rss_trend(samples)["passed"])
        samples[-3:] = [{"server": {"resident_bytes": 158 * mib}}] * 3
        result = runtime.rss_trend(samples)
        self.assertFalse(result["passed"])
        self.assertEqual(result["allowed_bytes"], 157 * mib)

    def test_failed_outcomes_still_enter_latency_denominator(self):
        rows = [{"offered_ns": 0, "finished_ns": value, "status": status}
                for value, status in ((10, "success"), (100, "error"), (1000, "queue_rejected"))]
        result = runtime.latency_summary(rows)
        self.assertEqual(result["n"], 3)
        self.assertEqual(result["maximum_ns"], 1000)
        self.assertFalse(result["tail_stability_claim"])

    def test_mutations_use_real_git_and_do_not_grow_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "project"
            runtime.make_fixture(root, 8)
            head_before = runtime.git(root, "rev-parse", "HEAD")["stdout"]
            sizes = []
            for number in range(24):
                event = runtime.mutate(root, number)
                sizes.append(sum(p.stat().st_size for p in root.glob("*.py")))
                if number == 4:
                    self.assertEqual(event["action"], "real_git_branch_switch")
                    self.assertNotEqual(runtime.git(root, "rev-parse", "HEAD")["stdout"], head_before)
            self.assertLess(max(sizes) - min(sizes), 100)
            self.assertFalse((root / "temporary.py").exists())
            self.assertFalse((root / "renamed.py").exists())

    def test_unowned_git_mutation_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises((FileNotFoundError, AssertionError, RuntimeError, ValueError)):
                runtime.git(Path(tmp), "init")

    def test_raw_is_exclusive_and_preserves_every_row(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "raw.jsonl"
            raw = runtime.Raw(path)
            raw.emit("operation", id=1, status="error")
            raw.close()
            with self.assertRaises(FileExistsError): runtime.Raw(path)
            self.assertEqual(json.loads(path.read_text())["status"], "error")


if __name__ == "__main__":
    unittest.main()
