"""Exercise the public plan CLI against independently authored entry-point drift."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[2] / "scripts/code_index_plan.py"
START = "<!-- code-index-progress:start -->"
END = "<!-- code-index-progress:end -->"


class PlanProgressTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        self.roadmap = self.repo / "docs/roadmap/code-index-v2"
        self.roadmap.mkdir(parents=True)
        self.source = self.roadmap / "tasks.json"
        self.todo = self.roadmap / "05-TODO.md"
        self.entries = [self.repo / "README.md", self.roadmap / "README.md",
                        self.roadmap / "08-HANDOFF.md"]
        self.data = {
            "status": "in_progress", "current_phase": "P7", "next_task": "P7-011",
            "last_implementation_date": "2026-10-07", "task_count": 3,
            "phase_order": ["P0", "P7"],
            "phase_names": {"P0": "基础", "P7": "语义接线"},
            "phase_task_counts": {"P0": 1, "P7": 2},
            "tasks": [
                self.task("P0-001", "done", [], ["frozen historical receipt"]),
                self.task("P7-011", "todo", ["P0-001"], []),
                self.task("P7-014", "in_progress", ["P7-011"], []),
            ],
        }
        self.write_source(self.data)
        (self.roadmap / "06-VALIDATION.md").write_text("# V01\n", encoding="utf-8")
        self.todo.write_text("old TODO\n", encoding="utf-8")
        for index, entry in enumerate(self.entries):
            entry.write_text("# Entry {}\n\n{}\n118 done / P5 stale\n{}\n\n"
                             "## Historical receipt\n原始失败记录 {} — unchanged.\n".format(
                                 index, START, END, index), encoding="utf-8")

    @staticmethod
    def task(identifier, status, dependencies, evidence):
        return {
            "id": identifier, "phase": identifier.split("-")[0], "batch": "fixture",
            "title": "Fixture " + identifier, "status": status, "scope": ["owned/path"],
            "depends_on": dependencies, "steps": ["inspect current source"],
            "acceptance": ["keep evidence"], "validations": ["V01"],
            "rollback": "restore the prior view", "evidence": evidence,
        }

    def write_source(self, data):
        self.source.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    def run_plan(self, *arguments):
        return subprocess.run([sys.executable, str(SCRIPT), "--root", str(self.roadmap),
                               *arguments], capture_output=True, text=True, check=False)

    def assert_passes(self, *arguments):
        result = self.run_plan(*arguments)
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)

    def snapshots(self):
        return {path: path.read_bytes() for path in [self.source, self.todo, *self.entries]}

    def test_write_updates_four_views_without_mutating_source_or_history(self):
        original_source = self.source.read_bytes()
        outside = {}
        for entry in self.entries:
            text = entry.read_text(encoding="utf-8")
            outside[entry] = (text.split(START)[0], text.split(END)[1])
        result = self.assert_passes("--write")
        self.assertEqual(result["states"], {"done": 1, "todo": 1, "in_progress": 1})
        self.assertEqual(result["task_count"], 3)
        self.assertEqual(result["views"], 4)
        self.assertEqual(self.source.read_bytes(), original_source)
        digest = hashlib.sha256(original_source).hexdigest()
        for entry in self.entries:
            with self.subTest(entry=entry):
                text = entry.read_text(encoding="utf-8")
                self.assertIn("3 项任务：1 done / 1 in_progress / 1 todo", text)
                self.assertIn("下一任务：**P7-011｜Fixture P7-011**", text)
                self.assertIn("| P7-014｜Fixture P7-014 | `in_progress` | P7-011 (todo) |", text)
                self.assertIn(digest, text)
                self.assertNotIn("118 done / P5 stale", text)
                self.assertEqual((text.split(START)[0], text.split(END)[1]), outside[entry])
        self.assertIn(digest, self.todo.read_text(encoding="utf-8"))
        self.assertIn("(docs/roadmap/code-index-v2/tasks.json)", self.entries[0].read_text())
        self.assertIn("(tasks.json)", self.entries[1].read_text())
        self.assert_passes()
        saved = self.snapshots()
        self.assert_passes("--write")
        self.assertEqual(self.snapshots(), saved)

    def test_each_view_drift_is_rejected_without_writing(self):
        self.assert_passes("--write")
        for target in [self.todo, *self.entries]:
            with self.subTest(target=target):
                original = target.read_bytes()
                if target == self.todo:
                    target.write_bytes(original + b"manual edit\n")
                else:
                    target.write_bytes(original.replace(b"1 done", b"118 done", 1))
                drifted = self.snapshots()
                result = self.run_plan()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Plan view drift", result.stderr)
                self.assertEqual(self.snapshots(), drifted)
                target.write_bytes(original)

    def test_markers_fail_before_any_write_even_in_last_entry(self):
        broken = [
            "no markers\n", START + "\nmissing end\n", END + "\nmissing start\n",
            START + "\n" + START + "\n" + END + "\n",
            START + "\n" + END + "\n" + END + "\n",
            END + "\n" + START + "\n", "inline " + START + "\n" + END + "\n",
        ]
        for text in broken:
            with self.subTest(text=text):
                self.entries[-1].write_text(text, encoding="utf-8")
                before = self.snapshots()
                result = self.run_plan("--write")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Progress markers", result.stderr)
                self.assertEqual(self.snapshots(), before)

    def test_invalid_navigation_metadata_is_rejected_without_repair(self):
        mutations = [
            {"next_task": "missing"}, {"next_task": []},
            {"next_task": "P0-001", "current_phase": "P0"},
            {"next_task": "P7-014"}, {"current_phase": "P0"}, {"current_phase": "P99"},
            {"status": "legacy_cloud_status"}, {"status": []}, {"status": "done"},
            {"last_implementation_date": "2026-02-30"},
            {"last_implementation_date": "20261007"}, {"last_implementation_date": None},
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.write_source(dict(self.data, **mutation))
                before = self.snapshots()
                result = self.run_plan("--write")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("ValueError:", result.stderr)
                self.assertEqual(self.snapshots(), before)

    def test_completed_plan_renders_without_next_task_or_dependency_table(self):
        for statuses in [("done", "done", "done"), ("done", "done", "deferred"),
                         ("deferred", "deferred", "deferred")]:
            with self.subTest(statuses=statuses):
                data = copy.deepcopy(self.data)
                data.update(status="done", current_phase=None, next_task=None)
                for task, status in zip(data["tasks"], statuses):
                    task.update(status=status, evidence=["recorded completion or deferral"])
                self.write_source(data)
                source = self.source.read_bytes()
                result = self.assert_passes("--write")
                self.assertEqual(result["states"], {state: statuses.count(state) for state in set(statuses)})
                self.assertEqual(self.source.read_bytes(), source)
                for entry in self.entries:
                    block = entry.read_text().split(START)[1].split(END)[0]
                    self.assertIn("计划状态：`done`（本轮计划已结束）", block)
                    self.assertIn("当前阶段：无；下一任务：无。", block)
                    self.assertNotIn("|", block)
                    if "deferred" in statuses:
                        self.assertIn("{} deferred".format(statuses.count("deferred")), block)
                self.assert_passes()
                before = self.snapshots()
                self.assert_passes("--write")
                self.assertEqual(self.snapshots(), before)

    def test_completed_plan_requires_finished_tasks_and_explicit_null_navigation(self):
        complete = copy.deepcopy(self.data)
        complete.update(status="done", current_phase=None, next_task=None)
        for task in complete["tasks"]:
            task.update(status="done", evidence=["completion receipt"])
        cases = []
        for field, value in [("current_phase", "P7"), ("next_task", "P7-011"),
                             ("status", "in_progress"), ("last_implementation_date", None)]:
            cases.append((field + "=" + str(value), dict(complete, **{field: value})))
        for field in ("current_phase", "next_task"):
            missing = copy.deepcopy(complete)
            del missing[field]
            cases.append(("missing " + field, missing))
        for status in ("todo", "in_progress", "blocked"):
            unfinished = copy.deepcopy(complete)
            unfinished["tasks"][-1]["status"] = status
            cases.append(("remaining " + status, unfinished))
        for label, data in cases:
            with self.subTest(case=label):
                self.write_source(data)
                before = self.snapshots()
                result = self.run_plan("--write")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("ValueError:", result.stderr)
                self.assertEqual(self.snapshots(), before)

    def test_deferred_next_task_is_rejected_without_repair(self):
        data = copy.deepcopy(self.data)
        data["tasks"][1]["status"] = "deferred"
        self.write_source(data)
        before = self.snapshots()
        result = self.run_plan("--write")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("next_task must not be done or deferred", result.stderr)
        self.assertEqual(self.snapshots(), before)

    def test_progress_keeps_transitive_unfinished_dependencies_visible(self):
        data = copy.deepcopy(self.data)
        data["tasks"][-1]["depends_on"] = ["P7-013"]
        data["tasks"][2:2] = [self.task("P7-012", "todo", ["P7-011"], []),
                                self.task("P7-013", "todo", ["P7-012"], [])]
        data["task_count"] = 5
        data["phase_task_counts"]["P7"] = 4
        self.write_source(data)
        self.assert_passes("--write")
        view = self.entries[0].read_text()
        self.assertIn("| P7-012｜Fixture P7-012 | `todo` | P7-011 (todo) |", view)
        self.assertIn("| P7-013｜Fixture P7-013 | `todo` | P7-012 (todo) |", view)
        self.assertIn("| P7-014｜Fixture P7-014 | `in_progress` | P7-013 (todo) |", view)
        self.assertEqual(json.loads(self.source.read_bytes()), data)

    def test_valid_chosen_next_task_is_not_replaced_by_first_unfinished(self):
        data = copy.deepcopy(self.data)
        data["tasks"].append(self.task("P8-001", "todo", ["P0-001"], []))
        data.update({"task_count": 4, "current_phase": "P8", "next_task": "P8-001"})
        data["phase_order"].append("P8")
        data["phase_names"]["P8"] = "选择的并行范围"
        data["phase_task_counts"]["P8"] = 1
        self.write_source(data)
        before = self.source.read_bytes()
        self.assert_passes("--write")
        self.assertEqual(self.source.read_bytes(), before)
        self.assertIn("下一任务：**P8-001｜Fixture P8-001**", self.entries[0].read_text())
        self.assert_passes()

    def test_source_only_change_invalidates_all_derived_hashes(self):
        self.assert_passes("--write")
        changed = dict(self.data, execution_note="new historical note, no task state change")
        self.write_source(changed)
        self.assertNotEqual(self.run_plan().returncode, 0)
        self.assert_passes("--write")
        digest = hashlib.sha256(self.source.read_bytes()).hexdigest()
        for target in [self.todo, *self.entries]:
            self.assertIn(digest, target.read_text())
        self.assertEqual(json.loads(self.source.read_bytes())["tasks"], self.data["tasks"])

    def test_done_dependency_guard_is_still_enforced(self):
        changed = copy.deepcopy(self.data)
        changed["tasks"][-1]["status"] = "done"
        changed["tasks"][-1]["evidence"] = ["insufficient receipt with unfinished prerequisite"]
        self.write_source(changed)
        result = self.run_plan("--write")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Done task has unfinished hard dependency", result.stderr)


if __name__ == "__main__":
    unittest.main()
