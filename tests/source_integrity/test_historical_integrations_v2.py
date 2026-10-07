"""Controls for historical snapshot fixity and independently frozen task definitions."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import code_index_plan as plan
import verify_historical_integrations_v2 as verifier


class HistoricalIntegrationsV2Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.git = verifier.GitObjects(ROOT)
        cls.git.ensure([verifier.TASK_BASE, verifier.PACKING_INTEGRATION,
                        verifier.E3_INTEGRATION])
        cls.baseline_raw = cls.git.blob(verifier.TASK_BASE, verifier.TASK_PATH)
        cls.baseline = verifier.parse_json(cls.baseline_raw)
        cls.current = verifier.parse_json(verifier.read_regular(ROOT, verifier.TASK_PATH))
        cls.packing_raw = cls.git.blob(verifier.PACKING_INTEGRATION,
                                     verifier.PACKING_REPORT + "/source-manifest.json")
        cls.packing = verifier.parse_json(cls.packing_raw)
        cls.e3_imports = verifier.parse_json(cls.git.blob(
            verifier.E3_INTEGRATION, verifier.E3_REPORT + "/imported-identity.json"))

    def task(self, data, identifier):
        return next(row for row in data["tasks"] if row["id"] == identifier)

    def test_fixed_historical_snapshots_match_their_own_manifests(self):
        self.assertEqual(verifier.sha(self.baseline_raw), verifier.TASK_BASE_SHA256)
        for ref, preserved, label, complete in [
            (verifier.PACKING_INTEGRATION, self.packing["preserved_task_states"], "packing", True),
            (verifier.E3_INTEGRATION, self.e3_imports["preserved_task_states"], "E3", False),
        ]:
            with self.subTest(label=label):
                snapshot = verifier.parse_json(self.git.blob(ref, verifier.TASK_PATH))
                verifier.historical_states(snapshot, preserved, label, complete)
                changed = copy.deepcopy(snapshot)
                self.task(changed, "P7-018")["status"] = "done"
                with self.assertRaisesRegex(verifier.VerificationError, "historical task states"):
                    verifier.historical_states(changed, preserved, label, complete)
        # These are the real four changes which caused the original CI failure.
        with self.assertRaisesRegex(verifier.VerificationError, "historical task states"):
            verifier.historical_states(self.current, self.packing["preserved_task_states"],
                                       "packing", complete=True)

    def test_actual_current_definitions_and_legal_progress_are_accepted(self):
        verifier.current_task_definitions(self.current, self.baseline)
        changed = copy.deepcopy(self.baseline)
        row = self.task(changed, "P7-011")
        row["status"] = "done"
        row["evidence"].append({"scope": "unit fixture only; no new acceptance claim"})
        row["implementation_notes"] = "Synthetic progress record for this control."
        changed["next_task"] = "P7-012"
        changed["execution_note"] += " Unit fixture progress."
        result = verifier.current_task_definitions(changed, self.baseline)
        self.assertEqual(result, [{"task_id": "P7-011", "baseline": "todo", "current": "done"}])
        plan.validate(changed, ROOT / "docs/roadmap/code-index-v2")

    def test_acceptance_dependencies_conditions_and_subgates_cannot_change(self):
        changes = [
            ("P7-012", "acceptance", ["weakened condition"]),
            ("P7-012", "depends_on", []),
            ("P7-018", "conditional", None),
            ("P7-018", "conditional_dependencies", [{"task": "P0-001", "when": "altered"}]),
            ("P7-012", "validations", []),
            ("P8-005", "acceptance_subgates", {}),
        ]
        for identifier, field, value in changes:
            with self.subTest(task=identifier, field=field):
                changed = copy.deepcopy(self.baseline)
                self.assertNotEqual(self.task(changed, identifier).get(field), value)
                self.task(changed, identifier)[field] = value
                with self.assertRaisesRegex(verifier.VerificationError, "task definition"):
                    verifier.current_task_definitions(changed, self.baseline)

    def test_definition_comparison_distinguishes_json_boolean_from_integer(self):
        changed = copy.deepcopy(self.baseline)
        self.assertEqual(changed["schema_version"], 1)
        changed["schema_version"] = True
        with self.assertRaisesRegex(verifier.VerificationError, "plan definition"):
            verifier.current_task_definitions(changed, self.baseline)

    def test_task_inventory_unknown_and_missing_fields_are_rejected(self):
        changes = [
            lambda data: data["tasks"].pop(),
            lambda data: data["tasks"].reverse(),
            lambda data: data["tasks"].__setitem__(1, copy.deepcopy(data["tasks"][0])),
            lambda data: data["tasks"][0].update(acceptance_override=True),
            lambda data: data["tasks"][0].pop("acceptance"),
            lambda data: data["tasks"][0].pop("evidence"),
            lambda data: data.update(definition_override="HEAD"),
            lambda data: data.pop("execution_note"),
        ]
        for number, change in enumerate(changes):
            with self.subTest(case=number):
                changed = copy.deepcopy(self.baseline)
                change(changed)
                with self.assertRaises(verifier.VerificationError):
                    verifier.current_task_definitions(changed, self.baseline)

    def test_progress_shape_and_current_completion_gate_remain_enforced(self):
        for field, value in [("status", "accepted_without_evidence"),
                             ("evidence", {"not": "a list"}),
                             ("implementation_notes", ["not a string"])]:
            with self.subTest(field=field):
                changed = copy.deepcopy(self.baseline)
                self.task(changed, "P7-011")[field] = value
                with self.assertRaises(verifier.VerificationError):
                    verifier.current_task_definitions(changed, self.baseline)
        for identifier, evidence, reason in [
            ("P7-011", [], "Done task lacks evidence"),
            ("P8-001", ["unit fixture"], "unfinished hard dependency"),
        ]:
            with self.subTest(task=identifier):
                changed = copy.deepcopy(self.baseline)
                self.task(changed, identifier).update(status="done", evidence=evidence)
                verifier.current_task_definitions(changed, self.baseline)
                with self.assertRaisesRegex(ValueError, reason):
                    plan.validate(changed, ROOT / "docs/roadmap/code-index-v2")

    def test_frozen_manifest_and_missing_or_changed_evidence_are_rejected(self):
        row = self.packing["evidence_files"][0]
        self.git.ensure([row["source_commit"]])
        original = self.git.blob(row["source_commit"], row["path"])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path = verifier.PACKING_REPORT + "/source-manifest.json"
            target = root / manifest_path
            target.parent.mkdir(parents=True)
            target.write_bytes(self.packing_raw)
            verifier.fixed_file(root, manifest_path, self.packing_raw)
            changed = copy.deepcopy(self.packing)
            changed["preserved_task_states"]["P7-018"] = "done"
            target.write_text(json.dumps(changed))
            with self.assertRaisesRegex(verifier.VerificationError, "frozen file changed"):
                verifier.fixed_file(root, manifest_path, self.packing_raw)
            target = root / row["path"]
            target.parent.mkdir(parents=True)
            target.write_bytes(original)
            self.assertEqual(verifier.sha(verifier.fixed_file(root, row["path"], original)),
                             row["sha256"])
            target.write_bytes(original + b"\nchanged evidence\n")
            with self.assertRaisesRegex(verifier.VerificationError, "frozen file changed"):
                verifier.fixed_file(root, row["path"], original)
            target.unlink()
            with self.assertRaises(FileNotFoundError):
                verifier.fixed_file(root, row["path"], original)

    @unittest.skipUnless(os.name == "posix", "symlink component control requires POSIX")
    def test_symlink_file_or_parent_cannot_substitute_for_fixed_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "real").mkdir()
            (root / "real/file").write_bytes(b"same bytes")
            (root / "linked").symlink_to(root / "real", target_is_directory=True)
            (root / "file-link").symlink_to(root / "real/file")
            for path in ["linked/file", "file-link"]:
                with self.subTest(path=path):
                    with self.assertRaisesRegex(verifier.VerificationError, "non-regular path"):
                        verifier.fixed_file(root, path, b"same bytes")

    def test_original_current_gates_and_excluded_directory_remain_enforced(self):
        gates = verifier.parse_json(verifier.read_regular(ROOT, verifier.GATES_PATH))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            verifier.current_gate_checks(root, gates)
            changes = [
                lambda data: data["rows"].pop(),
                lambda data: data["full_gate_status"].update(V19="passed"),
                lambda data: data["fixed_e3_local_attempt_width4_acceptance"].update(strict_causal_speedup=True),
                lambda data: data["fixed_e3_local_attempt_width4_acceptance"].update(statistical_significance=0),
            ]
            for change in changes:
                changed = copy.deepcopy(gates)
                change(changed)
                with self.assertRaises(verifier.VerificationError):
                    verifier.current_gate_checks(root, changed)
            (root / verifier.FORBIDDEN).mkdir(parents=True)
            with self.assertRaisesRegex(verifier.VerificationError, "excluded historical"):
                verifier.current_gate_checks(root, gates)

    def test_cli_rejects_optimized_python_and_pin_overrides_before_verification(self):
        script = ROOT / "scripts/verify_historical_integrations_v2.py"
        for flags, code, message in [
            (["-O", "-B", str(script)], 1, "optimized Python"),
            (["-B", str(script), "--task-base", "HEAD"], 2, "unrecognized arguments"),
        ]:
            with self.subTest(flags=flags):
                result = subprocess.run([sys.executable, *flags], cwd=ROOT,
                                        text=True, capture_output=True,
                                        env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
                self.assertEqual(result.returncode, code)
                self.assertIn(message, result.stderr)


if __name__ == "__main__":
    unittest.main()
