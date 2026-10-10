#!/usr/bin/env python3
"""Independent N=1 descriptive task population: 45 cells and 85 retained records.

Fresh history per mutation, complete original native closure and fifteen-table
parity, no tail statistics, no automatic task status or release certification.
The existing N30 isolated profile study and sequential study remain unchanged.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import p8_scale_matrix as full
from p7_build_identity import file_sha256, json_bytes

SCHEMA = "p8-profile-task-descriptive-v1"
SCOPE = "profile_task_descriptive_v1"
DRIVER_FILES = full.DRIVER_FILES + (
    "scripts/p8_task_profile_matrix.py", "scripts/p8_runner_capacity.py",
    "scripts/p8_task_profile_capture.py",
    "scripts/p8_scale_diagnostic_capture.py",
    "scripts/tests/test_p8_task_profile_matrix.py",
    "scripts/tests/test_p8_task_profile_capture.py",
    ".github/workflows/p8-task-profile.yml",
    "docs/roadmap/code-index-v2/P8-TASK-PROFILE-DESCRIPTIVE.md",
    "docs/roadmap/code-index-v2/p8-profile-task-descriptive-v1.json",
)
require = full.require
ERRORS = (OSError, ValueError, KeyError, IndexError, TypeError, subprocess.SubprocessError)
ENVIRONMENT_KEYS = ("CODECORTEX_SEED_CACHE_MAX_SYMBOLS", "RUSTFLAGS",
                    "CARGO_ENCODED_RUSTFLAGS", "TMPDIR", "TMP", "TEMP")


def driver_snapshot(root):
    root = Path(root).resolve(strict=True)
    old = full.driver_snapshot(root)
    require(Path(__file__).resolve() == root / "scripts/p8_task_profile_matrix.py",
            "task observer loaded from another checkout")
    inputs = dict(old["inputs"])
    for relative in DRIVER_FILES[len(full.DRIVER_FILES):]:
        path = root / relative
        require(path.is_file() and not path.is_symlink(), "nonregular task observer input")
        committed = subprocess.check_output(["git", "show", old["source_commit"] + ":" + relative], cwd=root)
        require(file_sha256(path) == hashlib.sha256(committed).hexdigest(), "task observer source drift: " + relative)
        inputs[relative] = file_sha256(path)
    require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip() == old["source_commit"],
            "task observer HEAD changed")
    return dict(source_commit=old["source_commit"], inputs=inputs,
                manifest_sha256=hashlib.sha256(json_bytes(inputs)).hexdigest())


def study_identity(run_id, attempt):
    require(isinstance(run_id, str) and run_id.isascii() and run_id.isdecimal() and 0 < int(run_id) < 2 ** 64,
            "a positive official workflow run ID is required")
    full.integer(attempt, "run attempt", 1, 2 ** 32 - 1)
    return dict(protocol=SCHEMA, run_id=run_id, run_attempt=attempt)


PROFILES = ("no_op", "body", "api", "config", "batch_1", "batch_10", "batch_100", "batch_1000")


def registered_plan(scale, shard_index, shard_count=1, repetitions=1, seed=12648430,
                    deadline_ms=18_000_000, capacity_profile=full.CAPACITY_PROFILE, *, study,
                    mutation_profile, fanout=None):
    require(type(repetitions) is int and repetitions == 1 and type(shard_count) is int and shard_count == 1
            and type(shard_index) is int and shard_index == 0 and type(seed) is int and seed == 12648430
            and type(deadline_ms) is int and deadline_ms == 18_000_000
            and capacity_profile == full.CAPACITY_PROFILE,
            "descriptive task N1/seed/worker budget/capacity differs")
    require(type(scale) is int and scale in full.SCALES, "noncanonical task scale")
    require(mutation_profile in PROFILES + ("fanout",), "unknown task profile")
    if mutation_profile == "fanout":
        require(type(fanout) is int and fanout in full.FANOUTS and scale == 1000,
                "fanout requires actual N+1 files; 1000 is only the capacity field")
    else:
        require(fanout is None, "mutation cell cannot carry a fanout")
    require(full.exact_equal(study, study_identity(study["run_id"], study["run_attempt"])),
            "task study identity differs")
    contract = full.capacity_contract(capacity_profile)
    return dict(schema_version=1, profile="release", files=[scale], seed=seed,
                repetitions=1, shard=dict(index=0, count=1),
                skip_fanout=mutation_profile != "fanout", capacity_profile=capacity_profile,
                dirty_budget=contract["scale_dirty_budget"],
                max_resume_builds=contract["scale_max_resume_builds"],
                batch_sizes=list(full.BATCHES), fanouts=list(full.FANOUTS),
                deadline_ms=deadline_ms, max_output_bytes=full.MAX_BYTES, stage_scope=SCOPE,
                profile_study=dict(run_id=study["run_id"], run_attempt=study["run_attempt"],
                                   mutation_profile=mutation_profile, fanout=fanout))


def registration(study, driver, built, native_receipt_sha256):
    """Written by build before any cell; every consumer checks this exact object."""
    return dict(schema=SCHEMA, kind="task-registration", stage_scope=SCOPE,
                study=study, driver_source=driver,
                source_commit=built["source_commit"],
                source_manifest_sha256=built["source_manifest_sha256"],
                binary_sha256=built["binary_sha256"], binary_blake3=built["binary_blake3"],
                native_build_receipt_sha256=native_receipt_sha256,
                repetitions=1, cell_count=45, record_count=85,
                cold_curve_selection="no_op setup only: five scales; included in forty setup records",
                plans=[registered_plan(1000 if profile == "fanout" else amount, index,
                         study=study, mutation_profile=profile,
                         fanout=amount if profile == "fanout" else None)
                       for profile, amount, index in sorted(expected_slots())],
                statistics="not_computed_N1_descriptive", task_statuses_changed=False,
                release_certification="not_run")

def envelope(out, kind, study, driver):
    return dict(schema=SCHEMA, kind=kind, stage_scope=SCOPE, study=study,
                driver_source=driver, started_utc=full.utc(), status="failed", passed=False,
                release_certification="not_run", task_statuses_changed=False)


def finish(out, filename, record):
    record["finished_utc"] = full.utc()
    record["files"] = full.inventory(out)
    full.write_new(out / filename, record)
    return record


def verify_envelope(directory, filename, kind, root):
    record = full.read_json(directory / filename)
    require(record.get("schema") == SCHEMA and record.get("kind") == kind and record.get("stage_scope") == SCOPE
            and record.get("status") == "passed" and record.get("passed") is True,
            "profile " + kind + " failed or belongs to another protocol")
    study = record["study"]
    require(full.exact_equal(study, study_identity(study["run_id"], study["run_attempt"])), "study identity differs")
    require(record.get("driver_source") == driver_snapshot(root), "task observer identity differs")
    require(record.get("files") == full.inventory(directory, (filename,)), "task envelope inventory changed")
    return record


def build(root, output, run_id, attempt, target_directory=None):
    root = Path(root).resolve(strict=True)
    out = full.new_directory(output)
    record = envelope(out, "task-build", study_identity(run_id, attempt), driver_snapshot(root))
    try:
        native = full.build(root, out / "native-build", target_directory or out.parent / (out.name + "-cargo"))
        require(native["status"] == "passed", "native release build failed")
        require(record["driver_source"] == driver_snapshot(root), "profile build observer changed")
        registry = registration(record["study"], record["driver_source"], native,
                                file_sha256(out / "native-build/build.json"))
        full.write_new(out / "task-registry.json", registry)
        record["registration_sha256"] = file_sha256(out / "task-registry.json")
        record.update(status="passed", passed=True, source_commit=native["source_commit"],
                      source_manifest_sha256=native["source_manifest_sha256"],
                      binary_sha256=native["binary_sha256"], binary_blake3=native["binary_blake3"],
                      native_build_receipt_sha256=file_sha256(out / "native-build/build.json"))
    except ERRORS as error:
        record["error"] = str(error)
    return finish(out, "task-build.json", record)


def validate_build(directory, root=None):
    root = Path(root or Path(__file__).resolve().parents[1]).resolve(strict=True)
    directory = Path(directory).resolve(strict=True)
    outer = verify_envelope(directory, "task-build.json", "task-build", root)
    built, binary = full.validate_build(directory / "native-build", root)
    require(outer["native_build_receipt_sha256"] == file_sha256(directory / "native-build/build.json"),
            "profile build inner receipt differs")
    for key in ("source_commit", "source_manifest_sha256", "binary_sha256", "binary_blake3"):
        require(outer[key] == built[key], "profile build " + key + " differs")
    require(outer["registration_sha256"] == file_sha256(directory / "task-registry.json"),
            "task registration bytes changed")
    require(full.exact_equal(full.read_json(directory / "task-registry.json"),
            registration(outer["study"], outer["driver_source"], built,
                         outer["native_build_receipt_sha256"])), "task registration differs")
    return outer, built, binary


def observed_environment(temporary_root):
    result = {key: os.environ.get(key) for key in ENVIRONMENT_KEYS}
    result.update({key: str(temporary_root) for key in ("TMPDIR", "TMP", "TEMP")})
    return result


def run_shard(root, build_directory, output, scale, shard_index, run_id, attempt, mutation_profile, fanout=None):
    root = Path(root).resolve(strict=True)
    study = study_identity(run_id, attempt)
    out = full.new_directory(output)
    record = envelope(out, "task-shard", study, driver_snapshot(root))
    record["requested_slot"] = dict(scale=scale, shard_index=shard_index,
                                    mutation_profile=mutation_profile, fanout=fanout)
    try:
        built, _, _ = validate_build(build_directory, root)
        require(built["study"] == study, "build belongs to another task run/attempt")
        record["task_build_receipt_sha256"] = file_sha256(Path(build_directory) / "task-build.json")
        record["registration_sha256"] = built["registration_sha256"]
        record["measurement_environment"] = observed_environment(out)
        plan = registered_plan(scale, shard_index, study=study, mutation_profile=mutation_profile, fanout=fanout)
        record["plan"] = plan
        inner = full.run_shard(root, Path(build_directory) / "native-build", out / "native-shard", plan)
        require(inner["status"] == "passed", "native task shard failed")
        require(record["driver_source"] == driver_snapshot(root), "task observer changed during measurement")
        require(record["measurement_environment"] == observed_environment(out), "measurement environment changed")
        record.update(status="passed", passed=True)
    except ERRORS as error:
        record["error"] = str(error)
    return finish(out, "task-shard.json", record)

def expected_fanout_case(n, seed):
    initial = {"api.ts": "export function scale_ping(x:number):number{return x;}\n"}
    assertions = []
    for i in range(n):
        path = f"use_{i:03}.ts"
        initial[path] = ("import {scale_ping} from './api';\n"
                         f"export function run_{i}(x:number):number{{return scale_ping(x);}}\n")
        assertions.append(dict(id=f"caller_{i}", table="call_edges", count=1,
            matches=dict(file_path=path, callee_symbol="scale_ping", target_file_path="api.ts")))
    return dict(schema_version=1, name=f"p8-fanout-{n}-{seed}", initial=initial,
        stages=[dict(mutation=dict(kind="write", path="api.ts", content=
            f"export function scale_ping(x:number,offset:number={seed % 1000 + 1}):number{{return x+offset;}}\n"),
            reopen=False, settle=True, assertions=assertions)], dirty_budget=8, max_resume_builds=128)


def inspect_raw(path, plan):
    selection = plan["profile_study"]
    profile = selection["mutation_profile"]
    inspected = full._inspect_raw(path, plan, isolated_profile=profile, profile_scope=SCOPE)
    snapshots = {}
    with Path(path).open("rb") as stream:
        for line in stream:
            event = full.decode(line)
            if event["event"] == "run_started":
                inspected["profile_environment"] = event["profile_environment"]
            elif event["event"] == "build_finished":
                require("process_snapshot" in event and isinstance(event.get("resource_scope"), str),
                        "original worker resource observation missing")
                snapshots[event["label"]] = dict(process_snapshot=event["process_snapshot"],
                                                 resource_scope=event["resource_scope"])
            elif event["event"] == "fanout_started":
                require(full.exact_equal(event["case"], expected_fanout_case(selection["fanout"], plan["seed"])),
                        "registered fanout fixture/mutation/independent witnesses differ")
            elif event["event"] == "fanout_finished":
                initial = event["result"].get("initial_evidence", {})
                require(initial.get("equal") is True and set(initial.get("tables", [])) == full.TABLES
                        and len(initial["tables"]) == 15
                        and set(initial.get("incremental", {})) == set(initial.get("full", {})) == full.TABLES
                        and full.exact_equal(initial["incremental"], initial["full"]),
                        "fanout initial full fifteen-table parity missing")
                require(len(initial["incremental"]["files"]) == selection["fanout"] + 1,
                        "fanout initial indexed file count differs from actual N+1")
                initial_reports = [full.check_build_report(initial.get(key))
                                   for key in ("incremental_report", "full_report")]
                point = event["result"]["checkpoints"][0]
                final_full = full.check_build_report(point.get("full_report"))
                require(all(item["complete"] and item["build_timing"]["full_staging_us"] is not None
                            for item in initial_reports + [final_full]), "fanout full build incomplete or not full")
                require(initial.get("runtime_config") == {"auto_index":{"enabled":False},
                        "indexing":{"dirty_propagation_max_files":8,"db_read_pool_size":1}},
                        "fanout original runtime config differs")
                require("process_snapshot" in initial and "process_snapshot" in point
                        and isinstance(initial.get("resource_scope"), str), "fanout resource observations missing")
                sample = inspected["measurements"][0]
                sample["initial_evidence"] = initial
                sample["full_control_build_timing"] = full.sum_build_timing([final_full])
                sample["full_control_ms"] = final_full["elapsed_ms"]
                sample["process_snapshot"] = point["process_snapshot"]
    expected = 1 if profile == "fanout" else 2
    require(len(inspected["measurements"]) == expected, "task native sample count differs")
    if profile != "fanout":
        require(len(snapshots) >= 4, "missing fresh setup, incremental or final full resource observation")
        inspected["resource_snapshots"] = snapshots
    return inspected


def validate_shard(directory, outer_build, built, binary, build_directory, root=None):
    root = Path(root or Path(__file__).resolve().parents[1]).resolve(strict=True)
    directory = Path(directory).resolve(strict=True)
    record = verify_envelope(directory, "task-shard.json", "task-shard", root)
    require(record["study"] == outer_build["study"] and record["driver_source"] == outer_build["driver_source"],
            "profile shard belongs to another study/observer")
    require(record["task_build_receipt_sha256"] == file_sha256(Path(build_directory) / "task-build.json"),
            "profile shard used another outer build")
    require(record["registration_sha256"] == outer_build["registration_sha256"],
            "task shard used another registration")
    original_plan = full.read_json(directory / "native-shard/registered-plan.json")
    require(full.exact_equal(record["plan"], original_plan), "outer/native task plan differs")
    selection = original_plan["profile_study"]
    inspected = full._validate_shard(directory / "native-shard", built, binary,
        file_sha256(Path(build_directory) / "native-build/build.json"),
        protocol_plan=lambda *args: registered_plan(*args, study=record["study"],
            mutation_profile=selection["mutation_profile"], fanout=selection["fanout"]), raw_inspector=inspect_raw)
    summary = full.read_json(directory / "native-shard/native/worker-summary.json")
    require(summary.get("stage_scope") == SCOPE and summary.get("sample_count") == (1 if selection["mutation_profile"] == "fanout" else 2), "native task terminal summary differs")
    measured = inspected["profile_environment"]
    seed = measured["seed_cache_max_symbols"]
    full.integer(seed.get("effective"), "effective seed cache capacity")
    parsed = seed.get("parsed_usize")
    require(parsed is None or type(parsed) is int and parsed >= 0, "invalid native seed cap parse result")
    require(seed["effective"] == (500_000 if parsed is None else parsed), "seed cache effective capacity differs")
    environment = record["measurement_environment"]
    require(set(environment) == set(ENVIRONMENT_KEYS), "profile environment keys differ")
    require(seed.get("raw") == environment["CODECORTEX_SEED_CACHE_MAX_SYMBOLS"] and
            seed.get("read_status") == ("absent" if seed["raw"] is None else "unicode"),
            "worker seed cache environment differs")
    report = full.read_json(directory / "native-shard/native/report.json")
    temporary = report.get("profile_temporary_environment", {})
    parent = full.producer_path(temporary.get("parent_root"), "supervisor temporary root")
    worker = full.producer_path(temporary.get("worker_root"), "worker temporary root")
    require(worker.parent == parent and str(parent) == environment["TMPDIR"],
            "worker temporary root is not the supervisor-owned child on the registered filesystem")
    worker_environment = {key: environment[key] for key in ENVIRONMENT_KEYS[1:]}
    worker_environment.update({key: str(worker) for key in ("TMPDIR", "TMP", "TEMP")})
    require(measured.get("runtime_environment") == worker_environment, "worker runtime environment differs")
    inner = full.read_json(directory / "native-shard/shard.json")
    require({key: environment[key] for key in ("TMPDIR", "TMP", "TEMP")} == inner["temporary_environment"],
            "profile temporary filesystem differs from capacity preflight")
    return inspected | dict(directory=str(directory), receipt_sha256=file_sha256(directory / "task-shard.json"),
                            study=record["study"], driver_source=record["driver_source"],
                            registration_sha256=record["registration_sha256"],
                            build_receipt_sha256=record["task_build_receipt_sha256"],
                            measurement_environment=environment)


def slot_key(plan):
    selection = plan["profile_study"]
    return (selection["mutation_profile"], selection["fanout"] if selection["mutation_profile"] == "fanout"
            else plan["files"][0], plan["shard"]["index"])


def expected_slots():
    return ({(profile, scale, 0) for profile in PROFILES for scale in full.SCALES}
            | {("fanout", fanout, 0) for fanout in full.FANOUTS})


def combine(shards):
    expected = expected_slots()
    seen, identities, environments, inputs = set(), set(), {}, {}
    cells, setup, mutations, fanouts, cold_curve = [], [], [], [], []
    for shard in shards:
        plan = shard["plan"]
        key = slot_key(plan)
        require(key in expected and key not in seen, "duplicate/unexpected task slot")
        fanout = key[0] == "fanout"
        require(full.exact_equal(plan, registered_plan(plan["files"][0], key[2], study=shard["study"],
            mutation_profile=key[0], fanout=key[1] if fanout else None)), "task registered plan differs")
        seen.add(key)
        engine = shard["engine"]
        identity = {field: engine.get(field) for field in ("engine_head_observed", "source_files_digest", "cargo_lock_digest",
                    "binary_digest", "eval_version", "os", "arch", "rustflags", "eval_debug_assertions")}
        identity.update(study=shard["study"], driver_source=shard["driver_source"],
                        build_receipt_sha256=shard["build_receipt_sha256"],
                        registration_sha256=shard["registration_sha256"])
        identities.add(json.dumps(identity, sort_keys=True))
        measurements = shard["measurements"]
        require(len(measurements) == (1 if fanout else 2), "missing setup or selected task profile")
        sample = measurements[-1]
        group = f"fanout-{key[1]}" if fanout else f"scale-{key[1]}/{key[0]}"
        sample_id = f"fanout-{key[1]}/repetition-0" if fanout else f"scale-{key[1]}/repetition-0"
        require(sample["group"] == group and sample["sample"] == sample_id, "task sample identity differs")
        environment = dict(shard["environment"], engine_cpu_parallelism=engine.get("cpu_parallelism"),
                           measurement_environment=shard["measurement_environment"],
                           profile_environment=shard["profile_environment"])
        require(isinstance(environment.get("host"), str) and environment["host"], "missing measured host")
        full.integer(environment["engine_cpu_parallelism"], "native CPU parallelism", 1)
        environment_id = hashlib.sha256(json_bytes(environment)).hexdigest()
        environments[environment_id] = environment
        input_key = f"fanout-{key[1]}" if fanout else f"scale-{key[1]}"
        if input_key in inputs:
            require(inputs[input_key] == shard["input_digest"], "pristine corpus differs across task profiles")
        inputs[input_key] = shard["input_digest"]
        selected = dict(slot=list(key), n=1, environment_id=environment_id, sample=sample)
        if fanout:
            fanouts.append(selected)
        else:
            cold = measurements[0]
            require(cold["group"] == f"scale-{key[1]}/cold" and cold["sample"] == sample_id,
                    "missing pristine setup pair")
            initial = dict(slot=list(key), n=1, environment_id=environment_id,
                           sample=cold, resource_snapshots=shard["resource_snapshots"])
            setup.append(initial)
            if key[0] == "no_op":
                cold_curve.append(initial)
            mutations.append(selected)
        cells.append(dict(slot=list(key), environment_id=environment_id,
                          directory=shard["directory"], receipt_sha256=shard["receipt_sha256"],
                          measurements=measurements))
    require(seen == expected and len(identities) == 1, "missing task slots or mixed source/binary/driver/build/run/attempt")
    require(len(cells) == 45 and len(setup) == len(mutations) == 40 and len(fanouts) == len(cold_curve) == 5,
            "task record coverage differs")
    require(any(item["sample"].get("first_build_incomplete") is True for item in fanouts),
            "no actual over-budget closure observed")
    for values in (cells, setup, mutations, fanouts, cold_curve):
        values.sort(key=lambda item: item["slot"])
    return dict(schema=SCHEMA, stage_scope=SCOPE, status="complete_task_descriptive_coverage", passed=True,
                study=shards[0]["study"], engine=json.loads(next(iter(identities))), registered_repetitions=1,
                cell_count=45, sample_count=85, record_count=85,
                mutation_sample_count=40, fanout_sample_count=5, setup_pair_count=40,
                setup_pairs=setup, mutation_samples=mutations, fanout_samples=fanouts,
                cold_curve_from_no_op=cold_curve, cold_curve_is_subset_of_setup=True,
                cells=cells, environments=environments, capacity_contract=full.capacity_contract(),
                input_digests=inputs,
                evidence=[dict(directory=s["directory"], receipt_sha256=s["receipt_sha256"]) for s in shards],
                history="fresh pristine A/B per profile; distinct from sequential warm/history distribution",
                deadline_scope="five hours per cell INCLUDING setup, full control and complete parity",
                total_time_allowance="225 worker-hours across 45 cells; neither old 150 nor isolated N30 population",
                scope="P8-006 descriptive task evidence only; no automatic task completion",
                resources_scope="original worker snapshots, not peaks or process-tree proof",
                statistics_scope="N1 per cell, each environment retained; no pooled or tail statistics",
                release_certification="not_run", G8="not_evaluated", task_statuses_changed=False)


def aggregate(build_directory, directories, output, root=None):
    out = full.new_directory(output)
    attempts, shards, errors = [], [], []
    result = dict(schema=SCHEMA, stage_scope=SCOPE, status="failed", passed=False,
                  release_certification="not_run", task_statuses_changed=False)
    try:
        outer, built, binary = validate_build(build_directory, root)
        result.update(study=outer["study"], driver_source=outer["driver_source"],
                      registration_sha256=outer["registration_sha256"],
                      task_build_receipt_sha256=file_sha256(Path(build_directory) / "task-build.json"))
        # Attempt every supplied cell once, retaining failures; never substitute a prefix.
        for directory in directories:
            entry = dict(directory=str(directory), status="failed", passed=False)
            try:
                path = Path(directory)
                entry["files"] = full.inventory(path)
                receipt = path / "task-shard.json"
                if receipt.is_file():
                    entry["original_task_receipt"] = full.read_json(receipt)
                    entry["receipt_sha256"] = file_sha256(receipt)
                inspected = validate_shard(path, outer, built, binary, build_directory, root)
                shards.append(inspected)
                entry.update(status="passed", passed=True, slot=list(slot_key(inspected["plan"])))
            except ERRORS as error:
                entry["error"] = str(error)
                errors.append(dict(directory=str(directory), error=str(error)))
            attempts.append(entry)
        require(not errors, "one or more task cells failed original validation")
        result.update(combine(shards))
    except ERRORS as error:
        result["error"] = str(error)
    result["input_results"] = attempts
    result["input_errors"] = errors
    result["missing_slots"] = [list(slot) for slot in sorted(expected_slots() - {slot_key(s["plan"]) for s in shards})]
    result["validated_input_count"] = len(shards)
    result["accepted_cell_count"] = len({slot_key(shard["plan"]) for shard in shards})
    full.write_new(out / "task-matrix.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    for mode in ("build", "run", "aggregate"):
        command = sub.add_parser(mode)
        command.add_argument("--root", type=Path, default=Path.cwd())
        command.add_argument("--output", type=Path, required=True)
        if mode != "aggregate":
            command.add_argument("--run-id", required=True)
            command.add_argument("--attempt", type=int, required=True)
        if mode == "build":
            command.add_argument("--target-directory", type=Path)
        else:
            command.add_argument("--build", type=Path, required=True)
        if mode == "run":
            command.add_argument("--scale", type=int, choices=full.SCALES, required=True)
            command.add_argument("--shard-index", type=int, required=True)
            command.add_argument("--mutation-profile", choices=PROFILES + ("fanout",), required=True)
            command.add_argument("--fanout", type=int, choices=full.FANOUTS)
        if mode == "aggregate":
            command.add_argument("--inputs", nargs="+", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.mode == "build":
            result = build(args.root, args.output, args.run_id, args.attempt, args.target_directory)
        elif args.mode == "run":
            result = run_shard(args.root, args.build, args.output, args.scale, args.shard_index, args.run_id, args.attempt, args.mutation_profile, args.fanout)
        else:
            result = aggregate(args.build, args.inputs, args.output, args.root)
        print(json.dumps({key: result.get(key) for key in ("status", "passed", "error", "sample_count")}))
        return 0 if result.get("passed") is True else 1
    except ERRORS as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
