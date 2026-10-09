"""Synthetic protocol controls only; never measured P8 cold samples."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_cold_matrix as cold
import p8_scale_matrix as full
from test_p8_scale_matrix import raw_fixture, complete_shards

STUDY = cold.study_identity("12345", 1)


def cold_fixture():
    _, old = raw_fixture(skip_fanout=True, capacity_profile=full.CAPACITY_PROFILE)
    end = next(i for i, event in enumerate(old) if event["event"] == "cold_parity")
    events = copy.deepcopy(old[:end + 1])  # Synthetic fixture construction, not historical artifact conversion.
    plan = cold.registered_plan(1000, 0, study=STUDY)
    events[0]["plan"] = plan
    events[0]["cold_environment"] = dict(seed_cache_max_symbols=dict(raw=None, read_status="absent", parsed_usize=None,
        effective=500000), runtime_environment={key: None for key in cold.ENVIRONMENT_KEYS[1:]})
    for event in events:
        if event["event"] == "build_finished":
            event.update(process_snapshot=None, resource_scope="synthetic unavailable snapshot")
    return plan, events


class ColdRawControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "raw.jsonl"
        self.plan, self.events = cold_fixture()

    def replay(self):
        self.path.write_text("".join(json.dumps(event) + "\n" for event in self.events))
        return cold.inspect_raw(self.path, self.plan)

    def test_explicit_cold_scope_retains_two_builds_parity_counts_resources(self):
        result = self.replay()
        self.assertEqual(len(result["measurements"]), 1)
        self.assertEqual(len(result["measurements"][0]["resource_snapshots"]), 2)
        self.assertEqual(set(result["measurements"][0]["counts"]["tables"]), full.TABLES)
        self.assertIsNone(result["measurements"][0]["counts"]["vectors"]["count"])
        with self.assertRaisesRegex(ValueError, "raw stage protocol"):
            full.inspect_raw(self.path, self.plan)

    def test_old_full_protocol_still_rejects_a_cold_prefix(self):
        self.plan.pop("stage_scope")
        self.plan.pop("cold_study")
        self.events[0]["plan"] = self.plan
        self.path.write_text("".join(json.dumps(event) + "\n" for event in self.events))
        with self.assertRaisesRegex(ValueError, "missing registered mutation stages"):
            full.inspect_raw(self.path, self.plan)

    def test_missing_second_build_table_and_failed_parse_are_rejected(self):
        original = copy.deepcopy(self.events)
        changes = (
            lambda: self.events.pop(5),
            lambda: self.events[-1]["parity"]["tables"].pop(),
            lambda: self.events[3]["report"].update(parse_errors=["synthetic failure"]),
            lambda: self.events[-1]["independent_config_fact"].update(passed=False),
        )
        for change in changes:
            self.events = copy.deepcopy(original)
            change()
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.replay()

    def test_extra_stage_failure_truncation_and_extra_build_cannot_be_hidden(self):
        original = copy.deepcopy(self.events)
        for event in (dict(event="mutation", label="scale-1000/repetition-0/body"),
                      dict(event="build_failed", label="failed"),
                      dict(event="stage_not_run")):
            self.events = copy.deepcopy(original) + [event]
            with self.subTest(event=event), self.assertRaises(ValueError):
                self.replay()
        self.events = copy.deepcopy(original)
        self.replay()
        self.path.write_bytes(self.path.read_bytes().rstrip(b"\n"))
        with self.assertRaisesRegex(ValueError, "truncated"):
            cold.inspect_raw(self.path, self.plan)
        self.events = copy.deepcopy(original)
        for row in original[2:4]:
            row = copy.deepcopy(row)
            row["label"] = "unconsumed"
            row["start_us"] += 100000
            if "end_us" in row:
                row["end_us"] += 100000
            self.events.append(row)
        with self.assertRaisesRegex(ValueError, "unconsumed"):
            self.replay()

    def test_native_raw_study_identity_cannot_be_relabelled(self):
        self.events[0]["plan"] = copy.deepcopy(self.plan)
        self.events[0]["plan"]["cold_study"]["run_id"] = "54321"
        with self.assertRaisesRegex(ValueError, "header/plan"):
            self.replay()

    def test_original_full_validator_has_no_cold_opt_in_argument(self):
        import inspect
        self.assertEqual(tuple(inspect.signature(full.validate_shard).parameters),
                         ("directory", "build_record", "binary", "build_receipt_sha256"))
        with mock.patch.object(full, "_validate_shard", return_value={}) as common:
            full.validate_shard("directory", {}, "binary", "sha")
        self.assertIs(common.call_args.kwargs["protocol_plan"], full.registered_plan)
        self.assertIs(common.call_args.kwargs["raw_inspector"], full.inspect_raw)


def cold_shards():
    shards = complete_shards(full.CAPACITY_PROFILE)
    for shard in shards:
        old = shard["plan"]
        shard["plan"] = cold.registered_plan(old["files"][0], old["shard"]["index"], study=STUDY)
        shard["measurements"] = [row for row in shard["measurements"] if row["group"].endswith("/cold")]
        shard.update(study=copy.deepcopy(STUDY), driver_source={"synthetic": True},
                     build_receipt_sha256="b" * 64, measurement_environment={}, cold_environment={})
    return shards


class ColdPopulationControls(unittest.TestCase):
    def test_exact_population_retains_every_original_cold_sample_and_stratum(self):
        shards = cold_shards()
        shards[1]["environment"]["cpu_model"] = "different synthetic CPU"
        result = cold.combine(shards)
        self.assertEqual(result["sample_count"], 150)
        self.assertEqual(len(result["groups"]), 5)
        for group in result["groups"]:
            self.assertEqual(group["n"], 30)
            self.assertEqual(len(group["samples"]), 30)
            self.assertEqual(sum(row["n"] for row in group["environment_strata"]), 30)
            self.assertNotIn("engine_ms", group)
        self.assertEqual(result["release_certification"], "not_run")
        self.assertFalse(result["task_statuses_changed"])

    def test_missing_duplicate_cross_run_binary_and_extra_stage_rejected(self):
        shards = cold_shards()
        for changed in (shards[:-1], shards + [shards[0]]):
            with self.assertRaises(ValueError):
                cold.combine(changed)
        mutations = (
            lambda xs: xs[1]["study"].update(run_id="55555"),
            lambda xs: xs[1]["engine"].update(binary_digest="f" * 64),
            lambda xs: xs[1]["measurements"].append(copy.deepcopy(xs[1]["measurements"][0])),
            lambda xs: xs[1]["plan"].pop("stage_scope"),
            lambda xs: xs[1]["measurements"][0].update(sample="scale-1000/repetition-0"),
        )
        for mutation in mutations:
            changed = copy.deepcopy(shards)
            mutation(changed)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                cold.combine(changed)
        with self.assertRaises(ValueError):
            full.combine(shards, shard_count=30, capacity_profile=full.CAPACITY_PROFILE)

    def test_registered_budget_and_n_cannot_be_lowered_or_inferred(self):
        for changes in (dict(shard_count=6), dict(repetitions=29), dict(deadline_ms=17_000_000),
                        dict(seed=2), dict(capacity_profile=None)):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                cold.registered_plan(1000, 0, study=STUDY, **changes)
        for run, attempt in (("", 1), ("0", 1), ("123", 0), ("123", True)):
            with self.assertRaises(ValueError):
                cold.study_identity(run, attempt)


class ColdEnvelopeControls(unittest.TestCase):
    """Synthetic receipts exercise the actual envelope/inner validators.

    Only the executable hash primitive and checked-out driver snapshot are
    replaced; no native binary is represented as measured by these controls.
    """
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bundle = self.root / "cold-shard"
        self.inner = self.bundle / "native-shard"
        self.native = self.inner / "native"
        self.native.mkdir(parents=True)
        self.build_dir = self.root / "build"
        (self.build_dir / "native-build").mkdir(parents=True)
        full.write_new(self.build_dir / "cold-build.json", {"synthetic": True})
        full.write_new(self.build_dir / "native-build/build.json", {"synthetic": True})
        self.driver = dict(source_commit="e" * 40, inputs={"synthetic": "a" * 64})
        self.outer_build = dict(study=STUDY, driver_source=self.driver)
        inputs = {path: "a" * 64 for path in ("Cargo.toml", "Cargo.lock", "crates/cc-eval/Cargo.toml",
            "crates/cc-eval/src/bin/p8-scale.rs", "crates/cc-eval/src/benchmark/p8_scale.rs",
            "crates/cc-eval/src/benchmark/oracle.rs", "crates/cc-eval/src/benchmark/oracle/streaming.rs")}
        import hashlib
        snapshot = dict(inputs=inputs, input_count=len(inputs), source_commit="e" * 40, source_tree="f" * 40,
                        manifest_sha256=hashlib.sha256(full.json_bytes(inputs)).hexdigest())
        self.built = dict(source_commit="e" * 40, source_manifest_sha256=snapshot["manifest_sha256"],
                          binary_sha256="b" * 64, binary_blake3="c" * 64, driver_source=self.driver)
        for name in ("source-before.json", "source-after.json"):
            full.write_new(self.inner / name, snapshot)
        self.plan, events = cold_fixture()
        environment = {key: None for key in cold.ENVIRONMENT_KEYS}
        environment.update({key: str(self.bundle) for key in ("TMPDIR", "TMP", "TEMP")})
        worker_environment = {key: environment[key] for key in cold.ENVIRONMENT_KEYS[1:]}
        worker_environment.update({key: str(self.bundle / ".synthetic-worker") for key in ("TMPDIR", "TMP", "TEMP")})
        events[0]["cold_environment"]["runtime_environment"] = worker_environment
        raw = "".join(json.dumps(event) + "\n" for event in events).encode()
        (self.native / "raw.jsonl").write_bytes(raw)
        (self.native / "worker.stderr").write_bytes(b"")
        for path in (self.inner / "registered-plan.json", self.native / "plan.json"):
            full.write_new(path, self.plan)
        self.summary = dict(stage_scope=cold.SCOPE, passed=True, sample_count=1, raw_bytes=len(raw),
            shard_only=True, executed_repetition_range=dict(start=0, end=1), release_certification="not_run",
            full_100k_certification="not_run", groups=[dict(group="scale-1000/cold", samples=1, passed=1, failed_or_not_compared=0)])
        full.write_new(self.native / "worker-summary.json", self.summary)
        self.report = dict(profile="release", max_output_bytes=full.MAX_BYTES, exit_code=0, worker_exit_code=0,
            status="measurement_complete", stderr_complete=True, fixture_cleanup=dict(error=None),
            worker_binary_unchanged=True, worker_binary_digest_before="c" * 64, worker_binary_digest_after="c" * 64,
            cold_temporary_environment=dict(parent_root=str(self.bundle), worker_root=str(self.bundle / ".synthetic-worker")),
            plan_digest="f" * 64, raw_digest="f" * 64, summary=self.summary, shard_only=True,
            registered_repetitions=30, executed_repetition_range=dict(start=0, end=1), release_certification="not_run",
            full_100k_certification="not_run")
        full.write_new(self.native / "report.json", self.report)
        minimum = 1000 * 256 * 1024 + 4 * 1024 ** 3
        full.write_new(self.inner / "disk-preflight.json", dict(schema="p8-scale-disk-preflight-v1", passed=True,
            required_free_bytes=minimum, available_bytes=minimum, temporary_root=str(self.bundle)))
        self.inner_record = dict(schema=full.SCHEMA, kind="shard", status="passed", exit_code=0, plan=self.plan,
            build_receipt_sha256=full.file_sha256(self.build_dir / "native-build/build.json"),
            capacity_contract=full.capacity_contract(), environment=dict(host="synthetic-host"),
            temporary_environment={key: str(self.bundle) for key in ("TMPDIR", "TMP", "TEMP")}, **self.built)
        self.outer_record = dict(schema=cold.SCHEMA, kind="cold-shard", stage_scope=cold.SCOPE, passed=True,
            status="passed", study=copy.deepcopy(STUDY), driver_source=self.driver, measurement_environment=environment,
            cold_build_receipt_sha256=full.file_sha256(self.build_dir / "cold-build.json"))
        self.seal()

    def seal(self):
        for directory, name, record in ((self.inner, "shard.json", self.inner_record),
                                        (self.bundle, "cold-shard.json", self.outer_record)):
            record["files"] = full.inventory(directory, (name,))
            (directory / name).write_bytes(full.json_bytes(record))

    def validate(self):
        with mock.patch.object(cold, "driver_snapshot", return_value=self.driver), \
                mock.patch.object(full, "native_digest", return_value="f" * 64):
            return cold.validate_shard(self.bundle, self.outer_build, self.built, Path("synthetic-binary"), self.build_dir, self.root)

    def test_actual_validators_admit_complete_synthetic_cold_and_reject_failed_terminal(self):
        self.assertEqual(len(self.validate()["measurements"]), 1)
        self.report.update(status="deadline_exceeded", exit_code=3, worker_exit_code=None)
        (self.native / "report.json").write_bytes(full.json_bytes(self.report))
        self.seal()
        with self.assertRaisesRegex(ValueError, "supervisor"):
            self.validate()

    def test_worker_temp_is_a_bound_child_not_an_equal_parent_or_another_filesystem(self):
        self.assertEqual(len(self.validate()["measurements"]), 1)
        for worker in (str(self.bundle), str(self.root / "unrelated")):
            self.report["cold_temporary_environment"]["worker_root"] = worker
            (self.native / "report.json").write_bytes(full.json_bytes(self.report))
            self.seal()
            with self.subTest(worker=worker), self.assertRaisesRegex(ValueError, "supervisor-owned child"):
                self.validate()

    def test_other_run_outer_build_and_changed_environment_are_rejected(self):
        original = copy.deepcopy(self.outer_record)
        for change in (
            lambda r: r["study"].update(run_id="98765"),
            lambda r: r.update(cold_build_receipt_sha256="f" * 64),
            lambda r: r["measurement_environment"].update(CODECORTEX_SEED_CACHE_MAX_SYMBOLS="0"),
        ):
            self.outer_record = copy.deepcopy(original)
            change(self.outer_record)
            self.seal()
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.validate()


if __name__ == "__main__":
    unittest.main()
