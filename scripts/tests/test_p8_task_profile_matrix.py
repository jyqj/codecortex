"""Harmless synthetic task protocol controls; these are not native observations."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_task_profile_matrix as task
import p8_profile_matrix as isolated
import p8_cold_matrix as cold
import p8_scale_matrix as full
from test_p8_profile_matrix import fixture as isolated_fixture

STUDY = task.study_identity("12345", 1)


def fixture(name="batch_1", fanout=None):
    _, events = isolated_fixture(name, fanout)
    plan = task.registered_plan(1000, 0, study=STUDY, mutation_profile=name, fanout=fanout)
    events[0]["plan"] = plan
    return plan, events


def write_raw(path, events):
    path.write_text("".join(json.dumps(event) + "\n" for event in events))


def synthetic_population():
    parsed = {}
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "raw.jsonl"
        for name, n in [(p, None) for p in task.PROFILES] + [("fanout", n) for n in full.FANOUTS]:
            plan, events = fixture(name, n)
            write_raw(path, events)
            parsed[(name, n)] = task.inspect_raw(path, plan)
    shards = []
    for name, amount, rep in sorted(task.expected_slots()):
        fanout = name == "fanout"
        item = copy.deepcopy(parsed[(name, amount if fanout else None)])
        plan = task.registered_plan(1000 if fanout else amount, rep, study=STUDY,
                                    mutation_profile=name, fanout=amount if fanout else None)
        for sample in item["measurements"]:
            if fanout:
                sample["sample"] = f"fanout-{amount}/repetition-0"
            else:
                sample["sample"] = f"scale-{amount}/repetition-0"
                sample["group"] = f"scale-{amount}/" + sample["group"].split('/')[-1]
        item.update(plan=plan, study=STUDY, driver_source={"synthetic": True},
                    build_receipt_sha256="a" * 64, registration_sha256="c" * 64,
                    directory=f"synthetic/{name}/{amount}/0", receipt_sha256="b" * 64,
                    environment={"host": f"synthetic-{name}-{amount}", "kernel": "synthetic"},
                    measurement_environment={key: None for key in task.ENVIRONMENT_KEYS})
        shards.append(item)
    return shards


class TaskRawControls(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "raw.jsonl"

    def replay(self, plan, events):
        write_raw(self.path, events)
        return task.inspect_raw(self.path, plan)

    def test_all_profiles_and_actual_fanouts_use_explicit_new_scope(self):
        for name, fanout in [(p, None) for p in task.PROFILES] + [("fanout", n) for n in full.FANOUTS]:
            with self.subTest(profile=name, fanout=fanout):
                plan, events = fixture(name, fanout)
                result = self.replay(plan, events)
                self.assertEqual(len(result["measurements"]), 1 if fanout else 2)
                for inspector in (full.inspect_raw, cold.inspect_raw, isolated.inspect_raw):
                    with self.assertRaisesRegex(ValueError, "raw stage protocol"):
                        inspector(self.path, plan)

    def test_n1_plan_rejects_old_population_bool_budget_and_wrong_scope_identity(self):
        invalid = (dict(repetitions=30), dict(shard_count=30), dict(shard_index=1),
                   dict(repetitions=True), dict(shard_index=False), dict(seed=True),
                   dict(deadline_ms=18000001), dict(capacity_profile=full.WIDE_DIRTY_PROFILE),
                   dict(scale=True), dict(mutation_profile="cold"), dict(fanout=1))
        for update in invalid:
            args = dict(scale=1000, shard_index=0, study=STUDY, mutation_profile="body")
            args.update(update)
            with self.subTest(update=update), self.assertRaises(ValueError):
                task.registered_plan(**args)
        for identity in ({"protocol": isolated.SCHEMA, "run_id": "12345", "run_attempt": 1},
                         {"protocol": task.SCHEMA, "run_id": "12345", "run_attempt": True}):
            with self.assertRaises(ValueError):
                task.registered_plan(1000, 0, study=identity, mutation_profile="body")

    def test_missing_setup_or_parity_and_incomplete_or_truncated_native_are_rejected(self):
        plan, original = fixture("body")
        changes = [lambda e: e.pop(5), lambda e: e[-1]["parity"]["tables"].pop(),
                   lambda e: e[-1].update(incremental_builds=2),
                   lambda e: e[-2]["report"].update(parse_errors=["synthetic error"]),
                   lambda e: e.append(dict(event="mutation", label="scale-1000/repetition-0/api")),
                   lambda e: e[0]["plan"]["profile_study"].update(run_attempt=2)]
        for change in changes:
            events = copy.deepcopy(original)
            change(events)
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.replay(plan, events)
        self.replay(plan, original)
        self.path.write_bytes(self.path.read_bytes().rstrip(b"\n"))
        with self.assertRaisesRegex(ValueError, "truncated"):
            task.inspect_raw(self.path, plan)

    def test_fresh_batch_target_and_real_fanout_initial_full_evidence_are_preserved(self):
        plan, events = fixture()
        events[-1]["independent_config_fact"].update(expected_target="f0001.ts", actual_targets=["f0001.ts"])
        with self.assertRaises(ValueError):
            self.replay(plan, events)
        plan, original = fixture("fanout", 16)
        for change in (lambda e: e[-1]["result"]["initial_evidence"].pop("full"),
                       lambda e: e[-1]["result"]["initial_evidence"]["incremental"]["files"].pop(),
                       lambda e: e[-1]["result"]["checkpoints"][0].pop("full_report"),
                       lambda e: e[-1]["result"]["checkpoints"][0]["truth"].pop()):
            events = copy.deepcopy(original)
            change(events)
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.replay(plan, events)


class TaskPopulationControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.population = synthetic_population()

    def test_exact_45_cells_85_records_with_prespecified_five_cold_subset(self):
        result = task.combine(self.population)
        self.assertTrue(result["passed"])
        self.assertEqual((result["cell_count"], result["record_count"], result["sample_count"]), (45, 85, 85))
        self.assertEqual((result["mutation_sample_count"], result["fanout_sample_count"], result["setup_pair_count"]), (40, 5, 40))
        self.assertEqual([row["slot"] for row in result["cold_curve_from_no_op"]],
                         [["no_op", scale, 0] for scale in full.SCALES])
        self.assertTrue(all(row in result["setup_pairs"] for row in result["cold_curve_from_no_op"]))
        self.assertEqual(len(result["environments"]), 45)
        self.assertNotIn("groups", result)
        self.assertNotIn("p95", json.dumps(result))
        self.assertFalse(result["task_statuses_changed"])
        self.assertEqual(result["release_certification"], "not_run")

    def test_missing_duplicate_extra_cells_and_missing_over_budget_closure_rejected(self):
        for rows in (self.population[:-1], self.population + [self.population[0]]):
            with self.assertRaises(ValueError):
                task.combine(rows)
        rows = copy.deepcopy(self.population)
        rows[0]["plan"]["shard"]["index"] = 1
        with self.assertRaises(ValueError):
            task.combine(rows)
        rows = copy.deepcopy(self.population)
        for row in rows:
            for sample in row["measurements"]:
                if "first_build_incomplete" in sample:
                    sample["first_build_incomplete"] = False
        with self.assertRaisesRegex(ValueError, "over-budget"):
            task.combine(rows)

    def test_each_identity_dimension_and_same_scale_corpus_must_be_one_registered_population(self):
        changes = (lambda row: row["engine"].update(binary_digest="f" * 64),
                   lambda row: row["engine"].update(engine_head_observed="f" * 40),
                   lambda row: row["engine"].update(source_files_digest="f" * 64),
                   lambda row: row.update(driver_source={"different": True}),
                   lambda row: row.update(build_receipt_sha256="f" * 64),
                   lambda row: row.update(registration_sha256="f" * 64),
                   lambda row: row["study"].update(run_attempt=2),
                   lambda row: row["study"].update(run_id="54321"),
                   lambda row: row.update(input_digest="f" * 64))
        for change in changes:
            rows = copy.deepcopy(self.population)
            change(rows[0])
            with self.subTest(change=change), self.assertRaises(ValueError):
                task.combine(rows)

    def test_no_fastest_setup_selection_or_suppression_of_other_35_setups(self):
        rows = copy.deepcopy(self.population)
        for row in rows:
            if row["plan"]["profile_study"]["mutation_profile"] != "fanout":
                row["measurements"][0]["test_marker"] = row["plan"]["profile_study"]["mutation_profile"]
        result = task.combine(rows)
        self.assertEqual({r["sample"]["test_marker"] for r in result["cold_curve_from_no_op"]}, {"no_op"})
        self.assertEqual(len([r for r in result["setup_pairs"] if r["sample"]["test_marker"] != "no_op"]), 35)

    def test_aggregate_continues_after_failure_and_retains_every_supplied_original_receipt(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            built = root / "build"
            built.mkdir()
            (built / "task-build.json").write_text('{}')
            inputs = []
            for index in range(3):
                directory = root / f"cell{index}"
                directory.mkdir()
                (directory / "task-shard.json").write_text(json.dumps({"synthetic": index, "passed": index != 1}))
                inputs.append(directory)
            outer = dict(study=STUDY, driver_source={}, registration_sha256="c" * 64)
            with mock.patch.object(task, "validate_build", return_value=(outer, {}, Path("unused"))), \
                 mock.patch.object(task, "validate_shard", side_effect=[self.population[0], ValueError("original native failed"), self.population[1]]) as validator:
                result = task.aggregate(built, inputs, root / "matrix", root)
            self.assertFalse(result["passed"])
            self.assertEqual(validator.call_count, 3)
            self.assertEqual([r["original_task_receipt"]["synthetic"] for r in result["input_results"]], [0, 1, 2])
            self.assertEqual(result["accepted_cell_count"], 2)
            self.assertEqual(len(result["missing_slots"]), 43)
            self.assertEqual(len(result["input_errors"]), 1)

    def test_run_build_failure_still_seals_failed_task_receipt_without_native_call(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with mock.patch.object(task, "driver_snapshot", return_value={"synthetic": True}), \
                 mock.patch.object(task, "validate_build", side_effect=ValueError("wrong original build")), \
                 mock.patch.object(task.full, "run_shard") as native:
                result = task.run_shard(root, root / "build", root / "out", 1000, 0, "12345", 1, "body")
            native.assert_not_called()
            self.assertFalse(result["passed"])
            self.assertEqual(result["error"], "wrong original build")
            self.assertTrue((root / "out/task-shard.json").is_file())

    def test_registration_contains_exact_plans_and_all_immutable_run_build_identities(self):
        built = dict(source_commit="a" * 40, source_manifest_sha256="b" * 64,
                     binary_sha256="c" * 64, binary_blake3="d" * 64)
        original = task.registration(STUDY, {"inputs": {"synthetic": "e" * 64}}, built, "f" * 64)
        self.assertEqual(len(original["plans"]), 45)
        self.assertEqual({task.slot_key(plan) for plan in original["plans"]}, task.expected_slots())
        self.assertEqual(original["record_count"], 85)
        for key in built:
            changed = dict(built)
            changed[key] = "different"
            self.assertNotEqual(original, task.registration(STUDY, {"inputs": {"synthetic": "e" * 64}}, changed, "f" * 64))

    def test_build_seals_registration_before_cells_and_rehash_cannot_change_registered_plans(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "build"
            native = dict(status="passed", source_commit="a" * 40, source_manifest_sha256="b" * 64,
                          binary_sha256="c" * 64, binary_blake3="d" * 64)
            observer = dict(source_commit="a" * 40, inputs={"synthetic": "e" * 64})

            def fake_build(_root, directory, _target):
                directory.mkdir()
                (directory / "build.json").write_text(json.dumps(native))
                return native

            with mock.patch.object(task, "driver_snapshot", return_value=observer), \
                 mock.patch.object(task.full, "build", side_effect=fake_build) as producer, \
                 mock.patch.object(task.full, "validate_build", return_value=(native, Path("unused"))):
                original = task.build(root, output, "12345", 1)
                producer.assert_called_once()
                self.assertTrue(original["passed"])
                self.assertEqual(task.validate_build(output, root)[0], original)
                registry_path = output / "task-registry.json"
                registry = json.loads(registry_path.read_text())
                registry["plans"][0]["deadline_ms"] = 1
                registry_path.write_text(json.dumps(registry))
                altered = dict(original)
                altered["registration_sha256"] = task.file_sha256(registry_path)
                altered["files"] = full.inventory(output, ("task-build.json",))
                (output / "task-build.json").write_text(json.dumps(altered))
                with self.assertRaisesRegex(ValueError, "task registration differs"):
                    task.validate_build(output, root)


if __name__ == "__main__":
    unittest.main()
