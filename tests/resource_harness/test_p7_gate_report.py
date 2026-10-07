"""Dossier integrity controls; temporary source and observations are synthetic."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import p7_gate_report as report


class GateReportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "crates/example/src/lib.rs"
        self.source.parent.mkdir(parents=True)
        self.source.write_text("// synthetic fixed source; not a build\n")
        (self.root / "Cargo.toml").write_text("[workspace]\n")
        (self.root / "Cargo.lock").write_text("version = 4\n")
        self.task_path = self.root / report.TASKS
        self.task_path.parent.mkdir(parents=True)
        tasks = []
        for number in range(1, 21):
            task_id = f"P7-{number:03}"
            deps = [] if number == 1 else [f"P7-{number - 1:03}"]
            if number == 20:
                deps = sorted(report.HARD_G7)
            tasks.append({"id": task_id, "status": "done" if number <= 10 else "todo",
                          "depends_on": deps, "acceptance": ["original synthetic condition"],
                          "validations": ["V15"], "evidence": [{"fixture": True}]})
        tasks[17]["status"] = "blocked"
        self.tasks = {"task_count": 20, "tasks": tasks}
        self.write(self.task_path, self.tasks)
        decision = self.root / report.DECISION
        decision.parent.mkdir(parents=True)
        self.write(decision, {"decisions": [{"id": "D1+D2", "affected_tasks": ["P7-018"],
                                            "choice": "synthetic no-live record"}]})
        self.evidence = self.root / "artifacts/current-synthetic-receipt.json"
        self.write(self.evidence, {"profile": "synthetic", "observation": "not a real test result"})
        self.git("init", "--quiet")
        self.commit()
        self.baseline = self.git("rev-parse", "HEAD").decode().strip()
        ref = self.ref(self.evidence)
        self.index = {"schema_version": 1, "baseline_sha": self.baseline,
                      "tasks_sha256": self.sha(self.task_path), "decision": self.ref(decision),
                      "entries": [{"task_id": "P7-011", "kind": "current_validation",
                                   "scope": "synthetic integrity control", "source_commit": self.baseline,
                                   "artifacts": [ref], "limitations": ["not a product validation"]}],
                      "wiring": [{"id": number, "task_id": "P7-014", "status": "not_reviewed",
                                  "note": "synthetic unresolved responsibility", "artifacts": [ref]}
                                 for number in range(1, 14)]}
        self.index_path = self.root / "index.json"

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, stderr=subprocess.PIPE)

    def commit(self):
        self.git("add", ".")
        self.git("-c", "user.name=G7 control", "-c", "user.email=fixture@example.invalid",
                 "commit", "--quiet", "-m", "synthetic fixed inputs")

    @staticmethod
    def sha(path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    @staticmethod
    def write(path, value):
        path.write_bytes(report.json_bytes(value))

    def ref(self, path):
        return {"path": path.relative_to(self.root).as_posix(), "sha256": self.sha(path)}

    def run_report(self):
        self.write(self.index_path, self.index)
        return report.collect(self.root, self.index_path)

    def save_tasks(self):
        self.write(self.task_path, self.tasks)
        self.index["tasks_sha256"] = self.sha(self.task_path)

    def test_incomplete_dossier_is_never_gate_or_live_approval(self):
        result = self.run_report()
        self.assertEqual(result["unfinished_count"], 10)
        self.assertEqual(result["engineering"]["status"], "not_accepted")
        self.assertEqual(result["live"]["status"], "blocked")
        self.assertIsNone(result["live"]["monetary_cost"])
        self.assertTrue(all(row["status"] == "blocked" and row["run_id"] is None
                            for row in result["live"]["validations"]))
        self.assertEqual(result["engineering"]["wiring_rows_requiring_review"], list(range(1, 14)))
        self.assertIn("P7-017", result["engineering"]["unfinished_hard_dependencies"])
        self.assertEqual(self.sha(self.task_path), self.index["tasks_sha256"])

    def test_progress_separates_acceptance_audit_and_blocked_disposition(self):
        self.tasks["tasks"][10]["status"] = "done"
        self.save_tasks()
        extra = copy.deepcopy(self.index["entries"][0])
        extra.update(task_id="P7-018", kind="blocked_disposition")
        self.index["entries"].append(extra)
        extra = copy.deepcopy(extra)
        extra.update(task_id="P7-019", kind="scope_audit")
        self.index["entries"].append(extra)
        result = self.run_report()
        self.assertEqual(result["unfinished_count"], 9)
        self.assertEqual(result["progress"]["accepted_since_baseline"], ["P7-011"])
        self.assertEqual(result["progress"]["implemented_or_currently_validated"], ["P7-011"])
        self.assertEqual(result["progress"]["blocked_dispositions"], ["P7-018"])
        self.assertEqual(result["progress"]["scope_audits_not_counted_as_implementation"], ["P7-019"])
        self.assertEqual(result["engineering"]["status"], "not_accepted")

    def test_missing_current_entries_do_not_reopen_accepted_tasks(self):
        result = self.run_report()
        self.assertEqual(result["engineering"]["accepted_tasks_without_current_submission"],
                         [f"P7-{number:03}" for number in range(1, 11)])
        self.assertEqual(result["engineering"]["missing_unfinished_current_submissions"],
                         ["P7-012", "P7-013", "P7-014", "P7-015", "P7-016", "P7-017", "P7-019"])
        self.assertEqual(result["unfinished_count"], 10)
        self.assertEqual(result["counts"]["done"], 10)
        self.assertIn("缺少本轮条目不会重开这些任务", report.markdown(result))
        self.assertEqual(self.sha(self.task_path), self.index["tasks_sha256"])

    def test_wrong_artifact_bytes_and_symlink_or_traversal_cannot_enter_report(self):
        self.evidence.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "digest drift"):
            self.run_report()
        self.index["entries"][0]["artifacts"][0]["sha256"] = self.sha(self.evidence)
        other = self.root / "same-evidence"
        self.evidence.rename(other)
        self.evidence.symlink_to(other)
        with self.assertRaisesRegex(ValueError, "symlink"):
            self.run_report()
        for value in [".", "../outside", "/etc/passwd", "artifacts/../Cargo.toml", "artifacts//file"]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                report.regular_path(self.root, value)

    def test_authority_drift_and_changed_original_acceptance_are_rejected(self):
        self.tasks["tasks"][10]["status"] = "in_progress"
        self.write(self.task_path, self.tasks)
        with self.assertRaisesRegex(ValueError, "task authority changed"):
            self.run_report()
        self.save_tasks()
        self.tasks["tasks"][10]["acceptance"] = ["weakened condition"]
        self.save_tasks()
        with self.assertRaisesRegex(ValueError, "original task condition changed"):
            self.run_report()

    def test_done_cannot_skip_hard_dependencies_or_reclassify_live_as_implementation(self):
        self.tasks["tasks"][11]["status"] = "done"
        self.save_tasks()
        with self.assertRaisesRegex(ValueError, "unfinished hard dependency"):
            self.run_report()
        self.tasks["tasks"][11]["status"] = "todo"
        self.save_tasks()
        self.index["entries"][0].update(task_id="P7-018", kind="implementation")
        with self.assertRaisesRegex(ValueError, "cannot be counted as implementation"):
            self.run_report()

    def test_old_source_remains_mixed_and_dirty_current_source_is_rejected(self):
        self.source.write_text("// later synthetic source\n")
        with self.assertRaisesRegex(ValueError, "uncommitted build input"):
            self.run_report()
        self.commit()
        result = self.run_report()
        self.assertEqual(result["engineering"]["mixed_source_submissions"], ["P7-011"])
        self.assertFalse(result["task_submissions"][0]["source_inputs_equal_to_current"])

    def test_duplicate_or_nonfinite_json_and_incomplete_wiring_are_rejected(self):
        for raw in [b'{"a":1,"a":2}', b'{"value":NaN}', b'{"value":Infinity}']:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                report.parse(raw)
        self.index["wiring"].pop()
        with self.assertRaisesRegex(ValueError, "all 13 wiring"):
            self.run_report()
        self.index_path.write_bytes(b" " * (256 * 1024 + 1))
        with self.assertRaisesRegex(ValueError, "exceeds 256 KiB"):
            report.collect(self.root, self.index_path)

    def test_cli_refuses_to_overwrite_previous_review_dossier(self):
        self.write(self.index_path, self.index)
        output = self.root / "published"
        args = [sys.executable, str(ROOT / "scripts/p7_gate_report.py"), "--root", str(self.root),
                "--evidence-index", str(self.index_path), "--output", str(output)]
        first = subprocess.run(args, capture_output=True, text=True)
        self.assertEqual(first.returncode, 0, first.stderr)
        original = (output / "report.json").read_bytes()
        self.assertIn("not_accepted", first.stdout)
        second = subprocess.run(args, capture_output=True, text=True)
        self.assertEqual(second.returncode, 2)
        self.assertEqual((output / "report.json").read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
