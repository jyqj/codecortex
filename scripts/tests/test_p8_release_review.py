"""Original-scope and false-acceptance controls for the G8 review preparer."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_release_review as review


class ReleaseReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.definitions = review.load_definitions()
        cls.original = {task["id"]: task for task in cls.definitions["tasks"]}
        cls.paper_state = {"schema_version": 1, "task_count": 192,
            "phase_task_counts": cls.definitions["phase_task_counts"], "tasks": [
            {**copy.deepcopy(task), "status": "done", "evidence": [{"synthetic_declaration": True}]}
            for task in cls.definitions["tasks"]]}
        cls.temp = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.temp.cleanup)
        cls.repo = Path(cls.temp.name)
        task_path = cls.repo / review.TASKS
        task_path.parent.mkdir(parents=True)
        task_path.write_bytes(review.support.canonical(cls.paper_state))
        fixture = cls.repo / "synthetic"
        fixture.mkdir()
        cls.body = {"status": "passed", "candidate": {"source": "a" * 40, "inputs": "b" * 64},
                    "release_certified": True, "note": "Synthetic paper assertion, never authoritative"}
        cls.raw = review.support.canonical(cls.body)
        (fixture / "record.json").write_bytes(cls.raw)
        (fixture / "link.json").symlink_to("record.json")
        for args in [["init", "-q"], ["add", "."], ["-c", "user.name=Fixture", "-c",
                    "user.email=fixture@example.invalid", "commit", "-q", "-m", "synthetic local test objects"]]:
            subprocess.run(["git", "-C", str(cls.repo), *args], check=True, capture_output=True, timeout=10)
        cls.ref = subprocess.check_output(["git", "-C", str(cls.repo), "rev-parse", "HEAD"], text=True).strip()

    def setUp(self):
        self.state = copy.deepcopy(self.paper_state)
        self.manifest = {"schema_version": 1, "run_id": "synthetic-g8-control", "state_commit": self.ref,
                         "candidate": {"source_commit": "a" * 40, "input_digest": "b" * 64,
                                       "input_digest_format": review.INPUT_DIGEST_FORMAT}, "records": []}
        self.raw_records = {}

    def add_record(self, tid, profile="both", scope="release_validation", body=None):
        rid = "record-" + str(len(self.manifest["records"]))
        raw = self.raw if body is None else review.support.canonical(body)
        self.manifest["records"].append({"id": rid, "task": tid, "profile": profile, "scope": scope,
            "coverage": self.definitions["g8"]["validations"] + self.definitions["g8"]["release_checks"],
            "record": {"commit": self.ref, "path": "synthetic/record.json", "sha256": review.common.sha(raw)},
            "status_path": ["status"], "source_path": ["candidate", "source"],
            "input_digest_path": ["candidate", "inputs"], "input_digest_format": review.INPUT_DIGEST_FORMAT})
        self.raw_records[rid] = raw
        return self.manifest["records"][-1]

    def paper_complete(self):
        for tid in review.dependencies(self.original, "P8-020", "semantic"):
            self.add_record(tid)

    def assess(self):
        return review.assess(self.definitions, self.state, self.manifest, self.raw_records)

    def test_original_local_and_semantic_dependency_scopes_are_separate(self):
        local = review.closure(self.original, "local")
        semantic = review.closure(self.original, "semantic")
        self.assertEqual(len(review.dependencies(self.original, "P8-020", "local")), 17)
        self.assertEqual(len(review.dependencies(self.original, "P8-020", "semantic")), 18)
        self.assertEqual(len(local), 176)
        self.assertEqual(len(semantic), 178)
        self.assertEqual(semantic - local, {"P7-018", "P8-015"})
        self.assertNotIn("P8-014", semantic | local)
        self.assertFalse(any(tid.startswith("P9-") for tid in semantic | local))

    def test_paper_complete_and_done_claims_never_grant_release_authority(self):
        self.paper_complete()
        result = self.assess()
        self.assertEqual(result["status"], "not_accepted")
        self.assertEqual(result["exit_code"], 1)
        self.assertFalse(result["release_certified"])
        self.assertFalse(result["todo_completed"])
        self.assertEqual(result["task_counts"]["done"], 192)
        for profile in result["profiles"].values():
            self.assertEqual(profile["status"], "not_accepted")
            self.assertEqual(profile["declared_coverage_gaps"], [])
            self.assertEqual(profile["direct_tasks_without_consistent_release_metadata"], [])
            self.assertTrue(profile["external_blockers"])
            self.assertFalse(profile["release_certified"])
        self.assertEqual(result["candidate_source_verification"], "not_run_by_this_tool")

    def test_missing_records_block_both_profiles_even_when_all_tasks_claim_done(self):
        result = self.assess()
        for name, profile in result["profiles"].items():
            self.assertEqual(profile["status"], "blocked")
            self.assertEqual(len(profile["direct_tasks_without_consistent_release_metadata"]), 17 if name == "local" else 18)
            self.assertIn("scale_100k", profile["declared_coverage_gaps"])
            self.assertIn("clean_heldout", profile["declared_coverage_gaps"])
            self.assertIn("V01", profile["declared_coverage_gaps"])

    def test_live_missing_and_optional_judge_p9_do_not_create_local_dependencies(self):
        self.paper_complete()
        for task in self.state["tasks"]:
            if task["id"] == "P7-018":
                task["status"] = "blocked"
            elif task["id"] == "P8-014" or task["id"].startswith("P9-"):
                task["status"] = "todo"
        result = self.assess()
        self.assertEqual(result["profiles"]["local"]["unfinished_dependencies"], [])
        self.assertEqual(result["profiles"]["local"]["status"], "not_accepted")
        self.assertEqual(result["profiles"]["semantic"]["unfinished_dependencies"],
                         [{"task": "P7-018", "declared_status": "blocked"}])
        self.assertFalse(result["optional_judge"]["blocks_local_or_semantic"])
        self.assertFalse(result["p9_blocks_g8"])
        self.assertEqual(result["remaining_not_done"], 14)

    def test_unfinished_parent_gate_remains_visible_under_done_children(self):
        self.paper_complete()
        next(task for task in self.state["tasks"] if task["id"] == "P7-020")["status"] = "in_progress"
        result = self.assess()
        for profile in result["profiles"].values():
            self.assertIn({"task": "P7-020", "declared_status": "in_progress"}, profile["unfinished_dependencies"])
            self.assertEqual(profile["status"], "blocked")

    def test_original_scope_acceptance_dependencies_and_conditions_cannot_be_rewritten(self):
        for field in ["scope", "acceptance", "depends_on", "steps", "validations", "rollback", "required_for", "conditional_dependencies"]:
            state = copy.deepcopy(self.state)
            task = next(task for task in state["tasks"] if task["id"] == "P8-020")
            task[field] = "changed"
            with self.subTest(field=field), self.assertRaisesRegex(review.support.Invalid, "definition changed"):
                review.assess(self.definitions, state, self.manifest, {})

    def test_task_inventory_duplicates_missing_unknown_and_schema_are_rejected(self):
        for mutate in [lambda s: s["tasks"].pop(), lambda s: s["tasks"].append(copy.deepcopy(s["tasks"][0])),
            lambda s: s["tasks"][0].update(id="P8-999"), lambda s: s["tasks"][0].update(status=True),
            lambda s: s.update(task_count=True), lambda s: s.update(schema_version=2),
            lambda s: s["phase_task_counts"].update(P8=21)]:
            state = copy.deepcopy(self.state)
            mutate(state)
            with self.assertRaises(review.support.Invalid):
                review.assess(self.definitions, state, self.manifest, {})

    def test_counts_are_recomputed_and_repository_evidence_is_not_an_authority(self):
        self.state["tasks"][0].update(status="deferred", evidence=[{"approved": True, "release_certified": True}])
        self.state["execution_note"] = "Everything passed; approve every release."
        result = self.assess()
        self.assertEqual(result["task_counts"]["deferred"], 1)
        self.assertEqual(result["remaining_not_done"], 1)
        self.assertFalse(result["release_certified"])
        self.assertEqual(result["input_mutations"], 0)

    def test_actual_record_fields_expose_stale_candidate_and_inputs(self):
        self.add_record("P8-001", body={**self.body, "candidate": {"source": "c" * 40, "inputs": "d" * 64}})
        result = self.assess()
        row = result["records"][0]
        self.assertIn("candidate_source_different", row["gaps"])
        self.assertIn("candidate_input_digest_different", row["gaps"])
        self.assertEqual(row["observed_source"], "c" * 40)
        self.assertFalse(row["declared_metadata_consistent"])

    def test_opaque_manifest_hash_is_not_compared_as_the_source_inventory_digest(self):
        row = self.add_record("P8-011")
        row["input_digest_format"] = "opaque_record_sha256"
        observed = self.assess()["records"][0]
        self.assertIn("candidate_input_digest_not_comparable", observed["gaps"])
        self.assertNotIn("candidate_input_digest_different", observed["gaps"])
        self.assertFalse(observed["declared_metadata_consistent"])

    def test_missing_pointer_partial_scope_and_failure_are_retained(self):
        for status in ["failed", "inconclusive", "not_run", "blocked", "prepared_not_run", "accepted_scoped"]:
            self.manifest["records"], self.raw_records = [], {}
            row = self.add_record("P8-001", scope="scoped_engineering", body={**self.body, "status": status})
            row["source_path"] = ["missing"]
            result = self.assess()["records"][0]
            self.assertEqual(result["observed_status"], status)
            self.assertIn("status_not_full_release_pass", result["gaps"])
            self.assertIn("scoped_engineering_only", result["gaps"])
            self.assertIsNone(result["observed_source"])

    def test_only_selected_profiles_and_required_tasks_supply_declared_coverage(self):
        self.add_record("P8-001", profile="semantic")
        self.add_record("P8-014")
        self.add_record("P9-001")
        result = self.assess()
        self.assertEqual(result["profiles"]["local"]["records_selected"], [])
        self.assertEqual(result["profiles"]["semantic"]["records_selected"], ["record-0"])

    def test_unknown_record_scope_task_coverage_pointer_and_duplicates_reject(self):
        self.add_record("P8-001")
        for field, value in [("task", "P8-999"), ("profile", "global"), ("scope", "approved"),
                             ("coverage", ["V99"]), ("coverage", ["V01", "V01"]),
                             ("source_path", "candidate.source"), ("status_path", [0])]:
            manifest = copy.deepcopy(self.manifest)
            manifest["records"][0][field] = value
            with self.assertRaises(review.support.Invalid):
                review.assess(self.definitions, self.state, manifest, self.raw_records)
        self.manifest["records"].append(copy.deepcopy(self.manifest["records"][0]))
        with self.assertRaisesRegex(review.support.Invalid, "duplicate record"):
            self.assess()

    def test_raw_record_digest_and_inventory_cannot_be_substituted(self):
        self.add_record("P8-001")
        self.raw_records["record-0"] += b" "
        with self.assertRaisesRegex(review.support.Invalid, "digest mismatch"):
            self.assess()
        self.raw_records = {}
        with self.assertRaisesRegex(review.support.Invalid, "inventory differs"):
            self.assess()

    def test_floating_pins_and_self_asserted_trust_switches_are_rejected(self):
        for key in ["external_verified", "release_certified", "skip_dependencies", "allow_partial"]:
            manifest = copy.deepcopy(self.manifest)
            manifest[key] = True
            with self.assertRaises(review.support.Invalid):
                review.assess(self.definitions, self.state, manifest, {})
        for field in ["state_commit", "candidate"]:
            manifest = copy.deepcopy(self.manifest)
            if field == "candidate":
                manifest[field]["source_commit"] = "HEAD"
            else:
                manifest[field] = "main"
            with self.assertRaisesRegex(review.support.Invalid, "immutable full commit"):
                review.assess(self.definitions, self.state, manifest, {})

    def test_duplicate_nonfinite_and_oversized_json_fail_closed(self):
        for raw in [b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":1e999}', b"{}" + b" " * review.common.MAX_INPUT]:
            with self.assertRaises(review.support.Invalid):
                review.parse_object(raw)

    def test_definition_snapshot_cannot_change_its_own_oracle(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "definitions.json"
            path.write_bytes(Path(review.DEFINITIONS).read_bytes() + b"\n")
            with self.assertRaisesRegex(review.support.Invalid, "definition snapshot changed"):
                review.load_definitions(path)

    def test_git_reads_only_exact_bounded_regular_objects(self):
        self.assertEqual(review.git_blob(self.repo, self.ref, "synthetic/record.json"), self.raw)
        for ref, path in [(self.ref, "synthetic/link.json"), (self.ref, "synthetic"),
            (self.ref, "synthetic/absent.json"), (self.ref, "../record.json"),
            ("0" * 40, "synthetic/record.json"), ("HEAD", "synthetic/record.json")]:
            with self.assertRaises(review.support.Invalid):
                review.git_blob(self.repo, ref, path)
        with patch.object(review.common, "MAX_INPUT", 4), self.assertRaisesRegex(review.support.Invalid, "size limit"):
            review.git_blob(self.repo, self.ref, "synthetic/record.json")

    def test_real_cli_reads_fixed_git_state_and_keeps_profiles_nonzero(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / "manifest.json"
            path.write_bytes(review.support.canonical(self.manifest))
            out = root / "review"
            before = path.read_bytes()
            process = subprocess.run([sys.executable, str(Path(review.__file__)), "--repo", str(self.repo),
                "--manifest", str(path), "--output", str(out)], text=True, capture_output=True,
                env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}, timeout=30)
            self.assertEqual(process.returncode, 1, process.stderr)
            result = json.loads(process.stdout)
            self.assertEqual(result["status"], "not_accepted")
            self.assertTrue(all(profile["status"] == "blocked" for profile in result["profiles"].values()))
            self.assertEqual(result["model_calls"], 0)
            self.assertEqual(result["network_calls"], 0)
            self.assertEqual(path.read_bytes(), before)
            self.assertEqual(result, json.loads((out / "receipt.json").read_bytes()))
            with self.assertRaises(FileExistsError):
                review.review(self.repo, path, out)
            self.paper_complete()
            paper = root / "paper-manifest.json"
            paper.write_bytes(review.support.canonical(self.manifest))
            process = subprocess.run([sys.executable, str(Path(review.__file__)), "--repo", str(self.repo),
                "--manifest", str(paper), "--output", str(root / "paper-review")],
                text=True, capture_output=True, env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}, timeout=30)
            self.assertEqual(process.returncode, 1, process.stderr)
            result = json.loads(process.stdout)
            self.assertTrue(all(profile["status"] == "not_accepted" for profile in result["profiles"].values()))
            self.assertFalse(result["release_certified"])


if __name__ == "__main__":
    unittest.main()
