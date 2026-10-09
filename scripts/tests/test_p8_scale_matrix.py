"""Synthetic evidence protocol controls; these are never product scale results."""
import copy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_scale_matrix as matrix


def engine():
    return dict(engine_head_observed="e" * 40, source_files_digest="a" * 64,
                cargo_lock_digest="b" * 64, binary_digest="c" * 64,
                eval_version="contract-only", os="linux", arch="x86_64", rustflags=None,
                cpu_parallelism=2, eval_debug_assertions=False)


def build_timing(full=False):
    return dict(schema_version=1, prepare_us=2000, commit_write_us=2000,
                postprocess_compute_us=1000, postprocess_apply_us=1000,
                between_stages_us=0, total_us=6000, prepare_snapshot_us=100,
                full_staging_us=100 if full else None)


def build_report(complete=True, full=False):
    return dict(files_scanned=1001, files_added=0, files_updated=0, files_removed=0,
                files_skipped=1001, files_parsed=0, symbols_total=1000, chunks_total=1000,
                elapsed_ms=6, parse_errors=[], dirty_plan={}, project_model={}, document_changes={},
                phase_timing={key: 1 for key in matrix.PHASES}, resolution_freshness=dict(complete=complete),
                build_timing=build_timing(full))


def counts():
    tables = {name: 1 for name in matrix.TABLES}
    return dict(tables=tables, files=1, symbols=1, chunks=1,
                edges={name: 1 for name in ("call_edges", "semantic_edges", "test_edges")},
                vectors=dict(state="disabled", count=None), index_db_with_wal_shm_bytes=4096)


def parity():
    return dict(equal=True, status="equal", different_tables=[], incremental_counts=counts(), full_counts=counts(),
                tables=[dict(table=name, equal=True, different_row_count=0, incremental_rows=1, full_rows=1,
                             incremental_digest="a" * 64, full_digest="a" * 64) for name in sorted(matrix.TABLES)])


def raw_fixture(skip_fanout=False, capacity_profile=None):
    plan = matrix.registered_plan(1000, 0, 30, capacity_profile=capacity_profile)
    plan["skip_fanout"] = skip_fanout
    prefix = "scale-1000/repetition-0"
    config = ["f0000.ts", "f0001.ts", "f0002.ts"]
    paths = [f"f{n:04}.ts" for n in range(1000)] + ["tsconfig.json"]
    events = [dict(event="run_started", plan=plan, shard_only=True, release_certification="not_run", engine=engine()),
              dict(event="input", sample=prefix, seed=plan["seed"], synthetic_files_requested=1000,
                   synthetic_files_written=1000, auxiliary_visible_config_files=1,
                   files=[dict(path=p, bytes=1, digest="d" * 64) for p in paths], input_digest="e" * 64,
                   config_fixture_augmented_code_files=config,
                   batch_target_policy="generated_code_then_existing_route_and_yaml_v2",
                   runtime_config={"auto_index": {"enabled": False}, "indexing": {
                       "dirty_propagation": True, "dirty_propagation_max_files": plan["dirty_budget"],
                       "db_read_pool_size": 1, "max_concurrent_parse": 2}, "semantic": {
                           "enabled": False, "network_opt_in": False, "allow_query_network": False}})]

    clock = 0

    def add_build(label, full):
        nonlocal clock
        start = clock + 100
        clock = start + 10000
        events.append(dict(event="build_started", label=label, full=full, start_us=start))
        events.append(dict(event="build_finished", label=label, full=full, start_us=start,
                           end_us=clock, wall_us=10000, report=build_report(full=full)))

    def fact(changed=False):
        target = config[1 if changed else 0]
        return dict(consumer=config[2], expected_target=target, actual_targets=[target], passed=True)

    add_build(prefix + "/cold_incremental", True)
    add_build(prefix + "/cold_full_control", True)
    events.append(dict(event="cold_parity", sample=prefix, parity=parity(), parity_wall_us=10,
                       independent_config_fact=fact()))
    for stage in ("no_op", "body", "api", "config", "batch_1", "batch_10", "batch_100", "batch_1000"):
        count = 0 if stage == "no_op" else int(stage[6:]) if stage.startswith("batch_") else 1
        label = prefix + "/" + stage
        if stage == "config":
            content = json.dumps({"compilerOptions": {"baseUrl": ".", "paths": {"@p8/config": ["./f0001.ts"]}}})
            ops = [dict(path="tsconfig.json", content=content)]
        else:
            content = (f"x\n// p8 batch {count} repetition 0\n" if stage.startswith("batch_") else
                       "p8_offset: number = 1" if stage == "api" else "p8_body_0")
            ops = [dict(path=paths[n], content=content) for n in range(count)]
        receipts = [dict(path=op["path"], before_digest="1" * 64, after_digest="2" * 64,
                         bytes=len(op["content"].encode())) for op in ops]
        events.append(dict(event="mutation", label=label, requested_files=count, operations=ops, receipts=receipts))
        add_build(label + "/incremental-0", False)
        add_build(label + "/full_control", True)
        events.append(dict(event="stage_finished", label=label, incremental_builds=1, passed=True,
                           complete=True, full_complete=True, parity=parity(), parity_wall_us=10,
                           incremental_wall_us=11000, no_op_unchanged=True,
                           independent_config_fact=fact(stage == "config" or stage.startswith("batch_"))))
    if not skip_fanout:
        for fanout in matrix.FANOUTS:
            initial = {"api.ts": "contract fake"} | {f"use_{i:03}.ts": "contract fake" for i in range(fanout)}
            case = dict(initial=initial, dirty_budget=8, max_resume_builds=128)
            events.append(dict(event="fanout_started", fanout=fanout, repetition=0, case=case))
            facts = {table: [] for table in matrix.TABLES}
            facts["call_edges"] = [dict(file_path=f"use_{i:03}.ts", callee_symbol="scale_ping", target_file_path="api.ts")
                                   for i in range(fanout)]
            reports = [build_report(False), build_report(True)] if fanout > 8 else [build_report(True)]
            point = dict(status="compared", settle_required=True, different_tables=[], reports=reports,
                         incremental=facts, full=copy.deepcopy(facts), truth=[
                             dict(id=f"caller_{i}", expected=1, actual=1, passed=True) for i in range(fanout)])
            result = dict(passed=True, failure_signature=None, tables=sorted(matrix.TABLES), checkpoints=[point])
            events.append(dict(event="fanout_finished", fanout=fanout, repetition=0, fixture_files=fanout + 1,
                               passed=True, result=result, incremental_builds=len(reports),
                               incremental_engine_elapsed_ms=6 * len(reports), first_build_incomplete=fanout > 8, wall_us=100000))
    if capacity_profile is not None:
        for event in events:
            if "parity" in event:
                event["parity"]["capacity_profile"] = capacity_profile
    return plan, events


class RawControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="p8-scale-raw-contract-")
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "raw.jsonl"
        self.plan, self.events = raw_fixture()

    def replay(self):
        self.path.write_text("".join(json.dumps(event, ensure_ascii=False) + "\n" for event in self.events))
        return matrix.inspect_raw(self.path, self.plan)

    def event(self, kind, suffix=None):
        return next(event for event in self.events if event["event"] == kind and
                    (suffix is None or event.get("label", "").endswith(suffix)))

    def test_complete_original_raw_population_replays(self):
        actual = self.replay()
        self.assertEqual(len(actual["measurements"]), 14)
        self.assertEqual(len({row["group"] for row in actual["measurements"]}), 14)
        self.assertEqual(actual["input_digest"], "e" * 64)

    def test_green_summary_cannot_replace_missing_canonical_table(self):
        self.event("cold_parity")["parity"]["tables"].pop()
        with self.assertRaisesRegex(ValueError, "fifteen"):
            self.replay()

    def test_incomplete_original_report_cannot_be_masked_by_passed_stage(self):
        self.event("build_finished", "body/incremental-0")["report"]["resolution_freshness"]["complete"] = False
        with self.assertRaisesRegex(ValueError, "incomplete closure"):
            self.replay()

    def test_noop_must_retain_zero_actual_file_changes(self):
        self.event("build_finished", "no_op/incremental-0")["report"]["files_updated"] = 1
        with self.assertRaisesRegex(ValueError, "no-op changed"):
            self.replay()

    def test_exact_batch_cannot_be_shrunk(self):
        self.event("mutation", "batch_1000")["operations"].pop()
        with self.assertRaisesRegex(ValueError, "silently shrunk"):
            self.replay()

    def test_config_mutation_and_independent_target_are_both_verified(self):
        self.event("mutation", "config")["operations"][0]["path"] = "f0000.ts"
        self.event("mutation", "config")["receipts"][0]["path"] = "f0000.ts"
        with self.assertRaisesRegex(ValueError, "registered target"):
            self.replay()

    def test_config_fact_cannot_pass_with_a_stale_target(self):
        self.event("stage_finished", "config")["independent_config_fact"]["actual_targets"] = ["f0000.ts"]
        with self.assertRaisesRegex(ValueError, "config target"):
            self.replay()

    def test_duplicate_and_missing_builds_are_rejected(self):
        original = copy.deepcopy(self.events)
        self.events.append(copy.deepcopy(self.event("build_finished")))
        with self.assertRaisesRegex(ValueError, "duplicate build"):
            self.replay()
        self.events = [event for event in original if not (event["event"] == "build_finished" and event["label"].endswith("api/full_control"))]
        with self.assertRaisesRegex(ValueError, "missing native"):
            self.replay()

    def test_partial_last_json_line_is_never_accepted(self):
        self.replay()
        self.path.write_bytes(self.path.read_bytes().rstrip(b"\n"))
        with self.assertRaisesRegex(ValueError, "truncated"):
            matrix.inspect_raw(self.path, self.plan)

    def test_raw_budget_exhaustion_cannot_certify_a_prefix(self):
        self.replay()
        self.plan["max_output_bytes"] = 100
        with self.assertRaisesRegex(ValueError, "budget"):
            matrix.inspect_raw(self.path, self.plan)

    def test_unknown_failure_event_is_rejected(self):
        self.events.append(dict(event="stage_not_run"))
        with self.assertRaisesRegex(ValueError, "not-run raw event"):
            self.replay()

    def test_fanout_full_facts_and_hand_assertions_are_replayed(self):
        event = self.event("fanout_finished")
        point = event["result"]["checkpoints"][0]
        point["incremental"]["call_edges"] = []
        point["full"]["call_edges"] = []
        with self.assertRaisesRegex(ValueError, "call edge missing"):
            self.replay()

    def test_false_budget_claim_is_rejected(self):
        self.event("fanout_finished")["first_build_incomplete"] = True
        with self.assertRaisesRegex(ValueError, "budget claim"):
            self.replay()

    def test_separate_fanout_can_only_be_omitted_by_explicit_partial_plan(self):
        self.plan, self.events = raw_fixture(skip_fanout=True)
        self.assertEqual(len(self.replay()["measurements"]), 9)
        self.plan["skip_fanout"] = False
        with self.assertRaisesRegex(ValueError, "missing registered fanout"):
            self.replay()

    def test_signed_zero_value_equality_does_not_require_same_digest(self):
        self.event("cold_parity")["parity"]["tables"][0]["full_digest"] = "f" * 64
        self.assertEqual(len(self.replay()["measurements"]), 14)
        self.assertTrue(matrix.exact_equal(-0.0, 0.0))
        self.assertFalse(matrix.exact_equal(True, 1))

    def test_json_duplicate_keys_and_nonfinite_numbers_fail_closed(self):
        for value in ('{"x": 1, "x": 2}', '{"x": NaN}', '{"x": 1e999}'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                matrix.decode(value)

    def test_boolean_is_not_an_observed_zero_latency(self):
        self.event("build_finished")["report"]["elapsed_ms"] = False
        with self.assertRaisesRegex(ValueError, "invalid integer"):
            self.replay()

    def test_missing_staging_and_unaccounted_stage_time_cannot_be_certified(self):
        original = copy.deepcopy(self.events)
        self.event("build_finished")["report"]["build_timing"]["total_us"] += 100000
        with self.assertRaisesRegex(ValueError, "conserve wall time"):
            self.replay()
        self.events = original
        self.event("build_finished")["report"]["build_timing"]["full_staging_us"] = None
        with self.assertRaisesRegex(ValueError, "actual build mode"):
            self.replay()


def complete_shards(capacity_profile=None):
    shards = []
    stages = ("cold", "no_op", "body", "api", "config") + tuple(f"batch_{n}" for n in matrix.BATCHES)
    for scale in matrix.SCALES:
        count = 30 if capacity_profile else 6
        for index in range(count):
            plan = matrix.registered_plan(scale, index, count, capacity_profile=capacity_profile)
            samples = []
            for repetition in matrix.repetition_range(plan):
                for stage in stages:
                    row = dict(group=f"scale-{scale}/{stage}", sample=f"scale-{scale}/repetition-{repetition}",
                               engine_ms=6, phases={phase: 1 for phase in matrix.PHASES},
                               build_timing=build_timing(stage == "cold"),
                               full_control_build_timing=build_timing(True))
                    if stage == "cold":
                        row["counts"] = counts()
                    samples.append(row)
                if scale == 1000:
                    for fanout in matrix.FANOUTS:
                        samples.append(dict(group=f"fanout-{fanout}", sample=f"fanout-{fanout}/repetition-{repetition}",
                            engine_ms=6, phases={phase: 1 for phase in matrix.PHASES}, first_build_incomplete=fanout > 8))
                        samples[-1]["build_timing"] = build_timing()
            shards.append(dict(directory=f"CONTRACT_FAKE/{scale}/{index}", receipt_sha256="d" * 64,
                               plan=plan, engine=engine(), environment=dict(cpu_model="CONTRACT_FAKE"),
                               input_digest="e" * 64, measurements=samples))
            if capacity_profile:
                shards[-1]["environment"].update(host=f"CONTRACT_FAKE-host-{index}", release="test-kernel")
    return shards


class MatrixControls(unittest.TestCase):
    def setUp(self):
        self.shards = complete_shards()

    def test_all_five_scales_and_thirty_repetitions_are_required(self):
        result = matrix.combine(self.shards)
        self.assertEqual(result["sample_count"], 1500)
        self.assertEqual(len(result["groups"]), 50)
        self.assertTrue(all(group["n"] == 30 for group in result["groups"]))
        self.assertEqual(result["release_certification"], "not_run")
        self.assertFalse(result["task_statuses_changed"])

    def test_missing_or_duplicated_shards_cannot_be_complete(self):
        with self.assertRaisesRegex(ValueError, "missing matrix shards"):
            matrix.combine(self.shards[:-1])
        with self.assertRaisesRegex(ValueError, "duplicate"):
            matrix.combine(self.shards + [self.shards[0]])

    def test_duplicate_global_repetition_cannot_hide_a_missing_sample(self):
        self.shards[1]["measurements"][0]["sample"] = "scale-1000/repetition-0"
        with self.assertRaisesRegex(ValueError, "global repetition"):
            matrix.combine(self.shards)

    def test_source_binary_environment_and_profile_drift_are_rejected(self):
        for target, key, value in (("engine", "binary_digest", "f" * 64),
                                   ("engine", "source_files_digest", "f" * 64),
                                   ("environment", "cpu_model", "other CPU"),
                                   ("plan", "dirty_budget", 9)):
            with self.subTest(key=key):
                shards = copy.deepcopy(self.shards)
                shards[1][target][key] = value
                with self.assertRaisesRegex(ValueError, "mismatch"):
                    matrix.combine(shards)

    def test_same_seed_input_drift_is_rejected(self):
        self.shards[1]["input_digest"] = "f" * 64
        with self.assertRaisesRegex(ValueError, "input changed"):
            matrix.combine(self.shards)

    def test_unobserved_budget_overflow_cannot_be_inferred_from_requested_fanout(self):
        for shard in self.shards:
            for sample in shard["measurements"]:
                if "first_build_incomplete" in sample:
                    sample["first_build_incomplete"] = False
        with self.assertRaisesRegex(ValueError, "no actual over-budget"):
            matrix.combine(self.shards)

    def test_fanout_population_is_counted_once_not_per_scale(self):
        self.shards[6]["measurements"].extend(copy.deepcopy([
            row for row in self.shards[0]["measurements"] if row["group"].startswith("fanout-")]))
        with self.assertRaisesRegex(ValueError, "global repetition"):
            matrix.combine(self.shards)

    def test_registered_population_and_native_limits_are_not_weakened(self):
        for args in ((1000, 0, 6, 29), (1000, 6, 6, 30), (1000, 0, 31, 30), (999, 0, 6, 30)):
            with self.subTest(args=args), self.assertRaises(ValueError):
                matrix.registered_plan(*args)
        plan = matrix.registered_plan(1000, 0, 6)
        self.assertEqual(plan["max_output_bytes"], 512 * 1024 * 1024)
        self.assertEqual(plan["batch_sizes"], [1, 10, 100, 1000])
        self.assertEqual(plan["fanouts"], [1, 4, 16, 64, 128])

    def test_cargo_release_profile_and_semantic_feature_lies_are_rejected(self):
        artifact = dict(reason="compiler-artifact", target=dict(name="p8-scale", kind=["bin"],
                        src_path="/example/crates/cc-eval/src/bin/p8-scale.rs"),
                        manifest_path="/example/crates/cc-eval/Cargo.toml", features=["default"],
                        profile=dict(opt_level="3", debug_assertions=False, test=False))
        matrix.verify_artifact(artifact)
        artifact["features"] = ["semantic"]
        with self.assertRaisesRegex(ValueError, "default p8-scale"):
            matrix.verify_artifact(artifact)
        artifact["features"] = []
        artifact["profile"]["debug_assertions"] = True
        with self.assertRaisesRegex(ValueError, "not optimized"):
            matrix.verify_artifact(artifact)

    def test_preserved_source_manifest_is_recomputed_not_only_compared_by_name(self):
        inputs = {path: "a" * 64 for path in ("Cargo.toml", "Cargo.lock", "crates/cc-eval/Cargo.toml",
            "crates/cc-eval/src/bin/p8-scale.rs", "crates/cc-eval/src/benchmark/p8_scale.rs",
            "crates/cc-eval/src/benchmark/oracle.rs", "crates/cc-eval/src/benchmark/oracle/streaming.rs")}
        snapshot = dict(inputs=inputs, input_count=len(inputs), source_commit="e" * 40,
                        source_tree="f" * 40, manifest_sha256=hashlib.sha256(matrix.json_bytes(inputs)).hexdigest())
        matrix.validate_snapshot(snapshot)
        inputs["Cargo.lock"] = "b" * 64
        with self.assertRaisesRegex(ValueError, "manifest digest"):
            matrix.validate_snapshot(snapshot)


class CapacityControls(unittest.TestCase):
    def test_new_capacity_is_explicit_and_retains_original_population_and_fanout(self):
        legacy = matrix.registered_plan(1000, 0, 6)
        capacity = matrix.registered_plan(1000, 0, 30, capacity_profile=matrix.CAPACITY_PROFILE)
        self.assertNotIn("capacity_profile", legacy)
        self.assertEqual((legacy["dirty_budget"], legacy["max_resume_builds"]), (8, 128))
        self.assertEqual((capacity["dirty_budget"], capacity["max_resume_builds"]), (200, 1024))
        for key in ("seed", "batch_sizes", "fanouts", "max_output_bytes", "deadline_ms", "repetitions"):
            self.assertEqual(capacity[key], legacy[key])
        for count, n in ((6, 30), (29, 29)):
            with self.assertRaises(ValueError):
                matrix.registered_plan(1000, 0, count, repetitions=n, capacity_profile=matrix.CAPACITY_PROFILE)

    def test_heterogeneous_hosts_retain_every_sample_in_explicit_strata_without_pooling(self):
        shards = complete_shards(matrix.CAPACITY_PROFILE)
        shards[1]["environment"]["cpu_model"] = "different CPU"
        shards[1]["environment"]["release"] = "different kernel"
        shards[1]["engine"]["cpu_parallelism"] = 8
        shards[1]["measurements"][0]["engine_ms"] = 10_000
        result = matrix.combine(shards, shard_count=30, capacity_profile=matrix.CAPACITY_PROFILE)
        self.assertEqual(result["sample_count"], 1500)
        self.assertEqual(result["environment_policy"], matrix.STRATIFIED_ENVIRONMENT)
        self.assertEqual(len(result["groups"]), 50)
        self.assertNotIn("cpu_parallelism", result["engine"])
        for group in result["groups"]:
            self.assertEqual(group["n"], 30)
            self.assertNotIn("engine_ms", group)
            self.assertEqual(sum(stratum["n"] for stratum in group["environment_strata"]), 30)
            self.assertEqual(len(group["sample_environments"]), 30)
            self.assertEqual({row["sample"] for row in group["sample_environments"]},
                             {sample for stratum in group["environment_strata"] for sample in stratum["sample_ids"]})
        cold = next(group for group in result["groups"] if group["group"] == "scale-1000/cold")
        self.assertIn(10_000, [sample for stratum in cold["environment_strata"] for sample in stratum["engine_ms"]["samples"]])

    def test_new_profile_cannot_mix_sources_infer_hosts_or_be_selected_implicitly(self):
        shards = complete_shards(matrix.CAPACITY_PROFILE)
        with self.assertRaisesRegex(ValueError, "profile mismatch"):
            matrix.combine(shards, shard_count=30)
        for target, key, value in (("engine", "binary_digest", "f" * 64),
                                   ("engine", "source_files_digest", "f" * 64),
                                   ("plan", "dirty_budget", 201),
                                   ("environment", "host", "")):
            changed = copy.deepcopy(shards)
            changed[1][target][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                matrix.combine(changed, shard_count=30, capacity_profile=matrix.CAPACITY_PROFILE)
        with self.assertRaisesRegex(ValueError, "missing matrix shards"):
            matrix.combine(shards[:-1], shard_count=30, capacity_profile=matrix.CAPACITY_PROFILE)

    def test_capacity_raw_still_requires_original_fanout_pressure_and_exact_oracle_profile(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "raw.jsonl"
            plan, events = raw_fixture(capacity_profile=matrix.CAPACITY_PROFILE)
            def replay():
                path.write_text("".join(json.dumps(event) + "\n" for event in events))
                return matrix.inspect_raw(path, plan)
            self.assertEqual(len(replay()["measurements"]), 14)
            started = next(event for event in events if event["event"] == "fanout_started")
            started["case"]["dirty_budget"] = 200
            with self.assertRaisesRegex(ValueError, "fanout closure budget"):
                replay()
            started["case"]["dirty_budget"] = 8
            next(event for event in events if event["event"] == "cold_parity")["parity"]["capacity_profile"] = None
            with self.assertRaisesRegex(ValueError, "parity capacity profile"):
                replay()

    def test_capacity_streaming_limits_cannot_silently_change(self):
        value = parity()
        value["capacity_profile"] = matrix.CAPACITY_PROFILE
        for side in ("incremental_counts", "full_counts"):
            value[side]["tables"]["chunks"] = value[side]["chunks"] = 100_001
        row = next(row for row in value["tables"] if row["table"] == "chunks")
        row["incremental_rows"] = row["full_rows"] = 100_001
        value.update(oracle_mode="disk_backed_exact_v1", limits=matrix.capacity_contract()["oracle_limits"],
                     sorting_cache_kib=2048, storage_layout=matrix.capacity_contract()["storage_layout"],
                     canonical_bytes=1_000_000, scratch_peak_bytes=4096)
        plan = matrix.registered_plan(1000, 0, 30, capacity_profile=matrix.CAPACITY_PROFILE)
        matrix.check_parity(value, plan)
        for key in ("max_canonical_bytes", "max_scratch_bytes", "max_rows_per_table", "max_row_bytes"):
            changed = copy.deepcopy(value)
            changed["limits"][key] += 1
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "limits/layout"):
                matrix.check_parity(changed, plan)

    def test_insufficient_disk_is_sealed_not_run_and_never_launches_native(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build_directory = root / "build"
            build_directory.mkdir()
            (build_directory / "build.json").write_text("{}")
            built = {key: "f" * 64 for key in ("source_commit", "source_manifest_sha256", "binary_sha256", "binary_blake3")}
            built["driver_source"] = {"contract": "synthetic"}
            plan = matrix.registered_plan(100000, 0, 30, capacity_profile=matrix.CAPACITY_PROFILE)
            required = 100000 * 256 * 1024 + 4 * 1024 ** 3
            with mock.patch.object(matrix, "validate_build", return_value=(built, root / "binary")), \
                 mock.patch.object(matrix, "source_snapshot", return_value={"contract": "synthetic"}), \
                 mock.patch.object(matrix.os, "statvfs", return_value=SimpleNamespace(f_bavail=required - 1, f_frsize=1)), \
                 mock.patch.object(matrix.subprocess, "run") as execute:
                actual = matrix.run_shard(root, build_directory, root / "failed", plan)
            self.assertEqual(actual["status"], "not_run")
            self.assertIsNone(actual["exit_code"])
            execute.assert_not_called()
            self.assertIn("disk-preflight.json", actual["files"])
            self.assertEqual(matrix.read_json(root / "failed/disk-preflight.json")["required_free_bytes"], required)
            self.assertEqual(actual["files"], matrix.inventory(root / "failed", ("shard.json",)))


class WideDirtyControls(unittest.TestCase):
    def test_named_work_budget_keeps_the_old_plan_and_physical_contract(self):
        old = matrix.registered_plan(100000, 0, 30, capacity_profile=matrix.CAPACITY_PROFILE)
        wide = matrix.registered_plan(100000, 0, 30, capacity_profile=matrix.WIDE_DIRTY_PROFILE)
        self.assertEqual(old["dirty_budget"], 200)
        self.assertEqual(wide, old | {"capacity_profile": matrix.WIDE_DIRTY_PROFILE, "dirty_budget": 4096})
        self.assertEqual(matrix.registered_plan(1000, 0, 6)["dirty_budget"], 8)
        physical = matrix.capacity_contract()
        self.assertEqual(physical["id"], matrix.CAPACITY_PROFILE)
        self.assertNotIn("oracle_capacity_profile", physical)
        self.assertEqual(matrix.capacity_contract(matrix.WIDE_DIRTY_PROFILE), physical | {
            "id": matrix.WIDE_DIRTY_PROFILE, "scale_dirty_budget": 4096,
            "oracle_capacity_profile": matrix.CAPACITY_PROFILE})
        for profile in matrix.CAPACITY_PROFILES:
            for parameters in ({"shard_count": 6}, {"repetitions": 29, "shard_count": 29},
                               {"deadline_ms": 18_000_001}):
                args = {"scale": 1000, "shard_index": 0, "shard_count": 30,
                        "capacity_profile": profile} | parameters
                with self.subTest(profile=profile, parameters=parameters), self.assertRaises(ValueError):
                    matrix.registered_plan(**args)
        with self.assertRaisesRegex(ValueError, "unknown capacity profile"):
            matrix.registered_plan(1000, 0, 30, capacity_profile="unregistered")

    def test_complete_wide_population_cannot_mix_old_work_profiles_or_limits(self):
        shards = complete_shards(matrix.WIDE_DIRTY_PROFILE)
        result = matrix.combine(shards, shard_count=30, capacity_profile=matrix.WIDE_DIRTY_PROFILE)
        self.assertEqual((result["sample_count"], len(result["groups"])), (1500, 50))
        self.assertTrue(all(group["n"] == 30 for group in result["groups"]))
        self.assertEqual(result["capacity_contract"], matrix.capacity_contract(matrix.WIDE_DIRTY_PROFILE))
        for selected in (None, matrix.CAPACITY_PROFILE):
            with self.subTest(selected=selected), self.assertRaisesRegex(ValueError, "profile mismatch"):
                matrix.combine(shards, shard_count=30, capacity_profile=selected)
        old = complete_shards(matrix.CAPACITY_PROFILE)
        with self.assertRaisesRegex(ValueError, "profile mismatch"):
            matrix.combine(old, shard_count=30, capacity_profile=matrix.WIDE_DIRTY_PROFILE)
        changed = copy.deepcopy(shards)
        changed[1] = old[1]
        with self.assertRaisesRegex(ValueError, "profile mismatch"):
            matrix.combine(changed, shard_count=30, capacity_profile=matrix.WIDE_DIRTY_PROFILE)
        for key, value in (("dirty_budget", 200), ("max_resume_builds", 1025),
                           ("max_output_bytes", matrix.MAX_BYTES + 1)):
            changed = copy.deepcopy(shards)
            changed[1]["plan"][key] = value
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "parameters mismatch"):
                matrix.combine(changed, shard_count=30, capacity_profile=matrix.WIDE_DIRTY_PROFILE)
        with self.assertRaisesRegex(ValueError, "missing matrix shards"):
            matrix.combine(shards[:-1], shard_count=30, capacity_profile=matrix.WIDE_DIRTY_PROFILE)

    def test_wide_raw_keeps_nine_stages_full_parity_and_original_fanout_pressure(self):
        plan, events = raw_fixture(capacity_profile=matrix.WIDE_DIRTY_PROFILE)
        for event in events:
            if "parity" in event:
                event["parity"]["capacity_profile"] = matrix.CAPACITY_PROFILE
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "raw.jsonl"
            def replay():
                path.write_text("".join(json.dumps(event) + "\n" for event in events))
                return matrix.inspect_raw(path, plan)
            rows = replay()["measurements"]
            self.assertEqual(len(rows), 14)
            self.assertEqual(sum(row["group"].startswith("scale-") for row in rows), 9)
            physical = next(event for event in events if event["event"] == "cold_parity")["parity"]
            physical["capacity_profile"] = matrix.WIDE_DIRTY_PROFILE
            with self.assertRaisesRegex(ValueError, "parity capacity profile"):
                replay()
            physical["capacity_profile"] = matrix.CAPACITY_PROFILE
            started = next(event for event in events if event["event"] == "fanout_started")
            started["case"]["dirty_budget"] = 4096
            with self.assertRaisesRegex(ValueError, "fanout closure budget"):
                replay()
            started["case"]["dirty_budget"] = 8
            physical["tables"].pop()
            with self.assertRaisesRegex(ValueError, "fifteen unique tables"):
                replay()

    def test_wide_disk_oracle_uses_only_the_existing_physical_limits(self):
        plan = matrix.registered_plan(1000, 0, 30, capacity_profile=matrix.WIDE_DIRTY_PROFILE)
        value = parity()
        value["capacity_profile"] = matrix.CAPACITY_PROFILE
        for side in ("incremental_counts", "full_counts"):
            value[side]["tables"]["chunks"] = value[side]["chunks"] = 100_001
        row = next(row for row in value["tables"] if row["table"] == "chunks")
        row["incremental_rows"] = row["full_rows"] = 100_001
        contract = matrix.capacity_contract()
        value.update(oracle_mode="disk_backed_exact_v1", limits=contract["oracle_limits"],
                     sorting_cache_kib=2048, storage_layout=contract["storage_layout"],
                     canonical_bytes=1_000_000, scratch_peak_bytes=4096)
        matrix.check_parity(value, plan)
        for key in ("max_canonical_bytes", "max_scratch_bytes", "max_rows_per_table", "max_row_bytes"):
            changed = copy.deepcopy(value)
            changed["limits"][key] += 1
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "limits/layout"):
                matrix.check_parity(changed, plan)

    def test_wide_insufficient_disk_retains_its_work_identity_without_launching(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build_directory = root / "build"
            build_directory.mkdir()
            (build_directory / "build.json").write_text("{}")
            built = {key: "f" * 64 for key in ("source_commit", "source_manifest_sha256", "binary_sha256", "binary_blake3")}
            built["driver_source"] = {"contract": "synthetic"}
            plan = matrix.registered_plan(100000, 0, 30, capacity_profile=matrix.WIDE_DIRTY_PROFILE)
            with mock.patch.object(matrix, "validate_build", return_value=(built, root / "binary")), \
                 mock.patch.object(matrix, "source_snapshot", return_value={"contract": "synthetic"}), \
                 mock.patch.object(matrix.os, "statvfs", return_value=SimpleNamespace(f_bavail=0, f_frsize=1)), \
                 mock.patch.object(matrix.subprocess, "run") as execute:
                record = matrix.run_shard(root, build_directory, root / "failed", plan)
            self.assertEqual(record["status"], "not_run")
            self.assertIsNone(record["exit_code"])
            execute.assert_not_called()
            self.assertEqual(record["capacity_contract"], matrix.capacity_contract(matrix.WIDE_DIRTY_PROFILE))
            self.assertEqual(record["command"][-2:], ["--capacity-profile", matrix.WIDE_DIRTY_PROFILE])
            self.assertEqual(record["files"], matrix.inventory(root / "failed", ("shard.json",)))


class PortableBuildOriginControls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="p8-portable-build-contract-")
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        self.build = root / "retained-build"
        self.build.mkdir()
        self.producer = root / "previous-vm-checkout"
        self.target = root / "previous-vm-target"
        self.producer.mkdir()
        self.target.mkdir()
        (self.target / "p8-scale").write_bytes(b"contract fixture bytes; not a compiled executable")
        shutil.copyfile(self.target / "p8-scale", self.build / "p8-scale")
        inputs = {path: "a" * 64 for path in ("Cargo.toml", "Cargo.lock", "crates/cc-eval/Cargo.toml",
            "crates/cc-eval/src/bin/p8-scale.rs", "crates/cc-eval/src/benchmark/p8_scale.rs",
            "crates/cc-eval/src/benchmark/oracle.rs", "crates/cc-eval/src/benchmark/oracle/streaming.rs")}
        snapshot = dict(inputs=inputs, input_count=len(inputs), source_commit="e" * 40,
                        source_tree="f" * 40, manifest_sha256=hashlib.sha256(matrix.json_bytes(inputs)).hexdigest())
        driver_inputs = {path: "b" * 64 for path in matrix.DRIVER_FILES}
        self.driver = dict(source_commit=snapshot["source_commit"], inputs=driver_inputs,
                           manifest_sha256=hashlib.sha256(matrix.json_bytes(driver_inputs)).hexdigest())
        artifact = dict(reason="compiler-artifact", features=[], executable=str(self.target / "p8-scale"),
                        manifest_path=str(self.producer / "crates/cc-eval/Cargo.toml"),
                        target=dict(name="p8-scale", kind=["bin"], src_path=str(self.producer / "crates/cc-eval/src/bin/p8-scale.rs")),
                        profile=dict(opt_level="3", debug_assertions=False, test=False))
        self.cargo = [artifact, {"reason": "build-finished", "success": True}]
        binary = self.build / "p8-scale"
        self.record = dict(schema=matrix.SCHEMA, kind="build", status="passed", exit_code=0,
            source_commit=snapshot["source_commit"], source_manifest_sha256=snapshot["manifest_sha256"],
            build_root=str(self.producer), target_directory=str(self.target), command=matrix.build_command(self.target),
            driver_source=self.driver, driver_sha256=self.driver["inputs"][matrix.DRIVER_FILES[0]], cargo_artifact=artifact,
            binary_sha256=matrix.file_sha256(binary), binary_bytes=binary.stat().st_size, binary_blake3="c" * 64,
            copy_source=dict(path=artifact["executable"], sha256=matrix.file_sha256(binary), bytes=binary.stat().st_size))
        for when in ("before", "after"):
            matrix.write_new(self.build / f"source-{when}.json", snapshot)
        (self.build / "cargo.stderr").write_text("synthetic protocol fixture; no compiler executed\n")

    def resign_and_verify(self):
        # Re-seal each adversarial fixture so content-inventory checks alone
        # cannot hide a missing semantic producer/copy binding.
        (self.build / "cargo.jsonl").write_text("".join(json.dumps(row) + "\n" for row in self.cargo))
        self.record["files"] = matrix.inventory(self.build, ("build.json",))
        (self.build / "build.json").write_text(json.dumps(self.record))
        with mock.patch.object(matrix, "driver_snapshot", return_value=self.driver), \
             mock.patch.object(matrix, "native_digest", return_value="c" * 64):
            return matrix.validate_build(self.build)

    def test_portable_archive_retains_copy_identity_after_the_producer_vm_is_gone(self):
        self.resign_and_verify()
        shutil.rmtree(self.producer)
        shutil.rmtree(self.target)
        record, binary = self.resign_and_verify()
        self.assertEqual(record["copy_source"]["sha256"], matrix.file_sha256(binary))

    def test_matching_suffix_cannot_hide_a_foreign_producer_checkout(self):
        artifact = self.record["cargo_artifact"]
        for owner, key in ((artifact, "manifest_path"), (artifact["target"], "src_path")):
            original = owner[key]
            owner[key] = original.replace("previous-vm-checkout", "foreign-checkout")
            with self.assertRaisesRegex(ValueError, "exact producer checkout"):
                self.resign_and_verify()
            owner[key] = original
        self.record["build_root"] = str(self.producer / "foreign")
        with self.assertRaisesRegex(ValueError, "exact producer checkout"):
            self.resign_and_verify()

    def test_original_command_target_and_cargo_executable_must_agree(self):
        original = copy.deepcopy(self.record)
        self.record["command"][-1] = str(self.target / "other")
        with self.assertRaisesRegex(ValueError, "command/target"):
            self.resign_and_verify()
        self.record["command"] = original["command"]
        self.record["cargo_artifact"]["executable"] = str(self.producer / "p8-scale")
        with self.assertRaisesRegex(ValueError, "selected Cargo target"):
            self.resign_and_verify()

    def test_resealed_copy_claims_cannot_replace_original_producer_bytes_or_path(self):
        original = copy.deepcopy(self.record["copy_source"])
        for key, value in (("sha256", "f" * 64), ("bytes", original["bytes"] + 1),
                           ("path", str(self.target / "another-executable"))):
            self.record["copy_source"][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.resign_and_verify()
            self.record["copy_source"] = dict(original)
        (self.build / "p8-scale").write_bytes(b"a replaced retained executable")
        self.record["binary_sha256"] = matrix.file_sha256(self.build / "p8-scale")
        self.record["binary_bytes"] = (self.build / "p8-scale").stat().st_size
        with self.assertRaisesRegex(ValueError, "original executable/copy"):
            self.resign_and_verify()


class DriverIdentityControls(unittest.TestCase):
    def test_loaded_driver_and_helper_must_equal_their_committed_git_blobs(self):
        with tempfile.TemporaryDirectory(prefix="p8-driver-source-control-") as temp:
            root = Path(temp)
            (root / "scripts").mkdir()
            source = Path(matrix.__file__).resolve().parents[1]
            for relative in matrix.DRIVER_FILES:
                shutil.copyfile(source / relative, root / relative)
            for command in (("init", "--quiet"), ("add", "scripts"),
                            ("-c", "user.name=P8 Protocol Test", "-c", "user.email=p8-test@example.invalid",
                             "commit", "--quiet", "-m", "Synthetic driver admission control")):
                subprocess.run(["git", *command], cwd=root, check=True, capture_output=True)
            command = [sys.executable, "-c", "import sys; sys.path.insert(0, 'scripts'); "
                       "import p8_scale_matrix as matrix; matrix.driver_snapshot('.')"]
            passed = subprocess.run(command, cwd=root, capture_output=True, text=True)
            self.assertEqual(passed.returncode, 0, passed.stderr)
            for relative in matrix.DRIVER_FILES:
                target = root / relative
                original = target.read_bytes()
                target.write_bytes(original + b"\n# Changed protocol code after source freeze\n")
                failed = subprocess.run(command, cwd=root, capture_output=True, text=True)
                self.assertNotEqual(failed.returncode, 0)
                self.assertIn("differs from committed source: " + relative, failed.stderr)
                target.write_bytes(original)


if __name__ == "__main__":
    unittest.main()
