#!/usr/bin/env python3
"""Separate fresh-history P8-006 study: 1200 mutation + 150 actual N+1 fanout cells.

Every worker includes setup under its original five-hour deadline. The increased
whole-study allowance and fresh history are explicitly different from the old
sequential all-stage study; no failed prefix or prior source can contribute N.
"""
import argparse
from collections import defaultdict
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import p8_scale_matrix as full
from p7_build_identity import file_sha256, json_bytes

SCHEMA = "p8-isolated-profile-study-v1"
SCOPE = "profile_isolated_v1"
DRIVER_FILES = full.DRIVER_FILES + (
    "scripts/p8_profile_matrix.py", "scripts/p8_runner_capacity.py",
    ".github/workflows/p8-profile.yml",
    "docs/roadmap/code-index-v2/P8-PROFILE-ISOLATED.md",
    "docs/roadmap/code-index-v2/p8-profile-isolated-v1.json",
)
require = full.require
ERRORS = (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError)
ENVIRONMENT_KEYS = ("CODECORTEX_SEED_CACHE_MAX_SYMBOLS", "RUSTFLAGS",
                    "CARGO_ENCODED_RUSTFLAGS", "TMPDIR", "TMP", "TEMP")


def driver_snapshot(root):
    root = Path(root).resolve(strict=True)
    old = full.driver_snapshot(root)
    require(Path(__file__).resolve() == root / "scripts/p8_profile_matrix.py",
            "profile observer loaded from another checkout")
    inputs = dict(old["inputs"])
    for relative in DRIVER_FILES[len(full.DRIVER_FILES):]:
        path = root / relative
        require(path.is_file() and not path.is_symlink(), "nonregular profile observer input")
        committed = subprocess.check_output(["git", "show", old["source_commit"] + ":" + relative], cwd=root)
        require(file_sha256(path) == hashlib.sha256(committed).hexdigest(), "profile observer source drift: " + relative)
        inputs[relative] = file_sha256(path)
    require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip() == old["source_commit"],
            "profile observer HEAD changed")
    return dict(source_commit=old["source_commit"], inputs=inputs,
                manifest_sha256=hashlib.sha256(json_bytes(inputs)).hexdigest())


def study_identity(run_id, attempt):
    require(isinstance(run_id, str) and run_id.isascii() and run_id.isdecimal() and 0 < int(run_id) < 2 ** 64,
            "a positive official workflow run ID is required")
    full.integer(attempt, "run attempt", 1, 2 ** 32 - 1)
    return dict(protocol=SCHEMA, run_id=run_id, run_attempt=attempt)


PROFILES = ("no_op", "body", "api", "config", "batch_1", "batch_10", "batch_100", "batch_1000")


def registered_plan(scale, shard_index, shard_count=30, repetitions=30, seed=12648430,
                    deadline_ms=18_000_000, capacity_profile=full.CAPACITY_PROFILE, *, study,
                    mutation_profile, fanout=None):
    require(type(repetitions) is int and repetitions == 30 and type(shard_count) is int and shard_count == 30
            and seed == 12648430 and deadline_ms == 18_000_000 and capacity_profile == full.CAPACITY_PROFILE,
            "isolated profile population/seed/worker budget/capacity differs")
    require(mutation_profile in PROFILES + ("fanout",), "unknown isolated profile")
    if mutation_profile == "fanout":
        require(type(fanout) is int and fanout in full.FANOUTS and type(scale) is int and scale == 1000,
                "fanout requires its actual N+1 fixture and the fixed 1000 capacity-reservation field")
    else:
        require(fanout is None, "mutation cell cannot carry a fanout")
    require(full.exact_equal(study, study_identity(study["run_id"], study["run_attempt"])), "profile study identity differs")
    plan = full.registered_plan(scale, shard_index, shard_count, repetitions, seed, deadline_ms, capacity_profile)
    plan.update(stage_scope=SCOPE, skip_fanout=mutation_profile != "fanout",
                profile_study=dict(run_id=study["run_id"], run_attempt=study["run_attempt"],
                                   mutation_profile=mutation_profile, fanout=fanout))
    return plan


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
    require(record.get("driver_source") == driver_snapshot(root), "profile observer identity differs")
    require(record.get("files") == full.inventory(directory, (filename,)), "profile envelope inventory changed")
    return record


def build(root, output, run_id, attempt, target_directory=None):
    root = Path(root).resolve(strict=True)
    out = full.new_directory(output)
    record = envelope(out, "profile-build", study_identity(run_id, attempt), driver_snapshot(root))
    try:
        native = full.build(root, out / "native-build", target_directory or out.parent / (out.name + "-cargo"))
        require(native["status"] == "passed", "native release build failed")
        require(record["driver_source"] == driver_snapshot(root), "profile build observer changed")
        record.update(status="passed", passed=True, source_commit=native["source_commit"],
                      source_manifest_sha256=native["source_manifest_sha256"],
                      binary_sha256=native["binary_sha256"], binary_blake3=native["binary_blake3"],
                      native_build_receipt_sha256=file_sha256(out / "native-build/build.json"))
    except ERRORS as error:
        record["error"] = str(error)
    return finish(out, "profile-build.json", record)


def validate_build(directory, root=None):
    root = Path(root or Path(__file__).resolve().parents[1]).resolve(strict=True)
    directory = Path(directory).resolve(strict=True)
    outer = verify_envelope(directory, "profile-build.json", "profile-build", root)
    built, binary = full.validate_build(directory / "native-build", root)
    require(outer["native_build_receipt_sha256"] == file_sha256(directory / "native-build/build.json"),
            "profile build inner receipt differs")
    for key in ("source_commit", "source_manifest_sha256", "binary_sha256", "binary_blake3"):
        require(outer[key] == built[key], "profile build " + key + " differs")
    return outer, built, binary


def observed_environment(temporary_root):
    result = {key: os.environ.get(key) for key in ENVIRONMENT_KEYS}
    result.update({key: str(temporary_root) for key in ("TMPDIR", "TMP", "TEMP")})
    return result


def run_shard(root, build_directory, output, scale, shard_index, run_id, attempt, mutation_profile, fanout=None):
    root = Path(root).resolve(strict=True)
    study = study_identity(run_id, attempt)
    built, _, _ = validate_build(build_directory, root)
    require(built["study"] == study, "build belongs to another study/run/attempt")
    out = full.new_directory(output)
    record = envelope(out, "profile-shard", study, driver_snapshot(root))
    record["profile_build_receipt_sha256"] = file_sha256(Path(build_directory) / "profile-build.json")
    record["measurement_environment"] = observed_environment(out)
    try:
        inner = full.run_shard(root, Path(build_directory) / "native-build", out / "native-shard",
                               registered_plan(scale, shard_index, study=study, mutation_profile=mutation_profile, fanout=fanout))
        require(inner["status"] == "passed", "native profile shard failed")
        require(record["driver_source"] == driver_snapshot(root), "profile observer changed during measurement")
        require(record["measurement_environment"] == observed_environment(out), "measurement environment changed")
        record.update(status="passed", passed=True)
    except ERRORS as error:
        record["error"] = str(error)
    return finish(out, "profile-shard.json", record)


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
    inspected = full._inspect_raw(path, plan, isolated_profile=profile)
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
    require(len(inspected["measurements"]) == expected, "isolated native sample count differs")
    if profile != "fanout":
        require(len(snapshots) >= 4, "missing fresh setup, incremental or final full resource observation")
        inspected["resource_snapshots"] = snapshots
    return inspected


def validate_shard(directory, outer_build, built, binary, build_directory, root=None):
    root = Path(root or Path(__file__).resolve().parents[1]).resolve(strict=True)
    directory = Path(directory).resolve(strict=True)
    record = verify_envelope(directory, "profile-shard.json", "profile-shard", root)
    require(record["study"] == outer_build["study"] and record["driver_source"] == outer_build["driver_source"],
            "profile shard belongs to another study/observer")
    require(record["profile_build_receipt_sha256"] == file_sha256(Path(build_directory) / "profile-build.json"),
            "profile shard used another outer build")
    selection = full.read_json(directory / "native-shard/registered-plan.json")["profile_study"]
    inspected = full._validate_shard(directory / "native-shard", built, binary,
        file_sha256(Path(build_directory) / "native-build/build.json"),
        protocol_plan=lambda *args: registered_plan(*args, study=record["study"],
            mutation_profile=selection["mutation_profile"], fanout=selection["fanout"]), raw_inspector=inspect_raw)
    summary = full.read_json(directory / "native-shard/native/worker-summary.json")
    require(summary.get("stage_scope") == SCOPE and summary.get("sample_count") == (1 if selection["mutation_profile"] == "fanout" else 2), "native profile terminal summary differs")
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
    return inspected | dict(directory=str(directory), receipt_sha256=file_sha256(directory / "profile-shard.json"),
                            study=record["study"], driver_source=record["driver_source"],
                            build_receipt_sha256=record["profile_build_receipt_sha256"],
                            measurement_environment=environment)


def slot_key(plan):
    selection = plan["profile_study"]
    return (selection["mutation_profile"], selection["fanout"] if selection["mutation_profile"] == "fanout"
            else plan["files"][0], plan["shard"]["index"])


def expected_slots():
    return ({(profile, scale, index) for profile in PROFILES for scale in full.SCALES for index in range(30)}
            | {("fanout", fanout, index) for fanout in full.FANOUTS for index in range(30)})


def combine(shards):
    expected = expected_slots()
    seen, groups, identities, environments, inputs, setup = set(), defaultdict(list), set(), {}, {}, []
    for shard in shards:
        plan = shard["plan"]
        key = slot_key(plan)
        require(key in expected and key not in seen, "duplicate/unexpected isolated slot")
        fanout = key[0] == "fanout"
        require(full.exact_equal(plan, registered_plan(plan["files"][0], key[2], study=shard["study"],
            mutation_profile=key[0], fanout=key[1] if fanout else None)), "isolated registered plan differs")
        seen.add(key)
        engine = shard["engine"]
        identity = {field: engine.get(field) for field in ("engine_head_observed", "source_files_digest", "cargo_lock_digest",
                    "binary_digest", "eval_version", "os", "arch", "rustflags", "eval_debug_assertions")}
        identity.update(study=shard["study"], driver_source=shard["driver_source"],
                        build_receipt_sha256=shard["build_receipt_sha256"])
        identities.add(json.dumps(identity, sort_keys=True))
        require(len(shard["measurements"]) == (1 if fanout else 2), "missing setup or selected profile")
        sample = shard["measurements"][-1]
        group = f"fanout-{key[1]}" if fanout else f"scale-{key[1]}/{key[0]}"
        sample_id = f"fanout-{key[1]}/repetition-{key[2]}" if fanout else f"scale-{key[1]}/repetition-{key[2]}"
        require(sample["group"] == group and sample["sample"] == sample_id, "profile sample identity differs")
        if not fanout:
            cold = shard["measurements"][0]
            require(cold["group"] == f"scale-{key[1]}/cold" and cold["sample"] == sample_id,
                    "missing pristine setup pair")
            setup.append(dict(slot=list(key), sample=cold, resource_snapshots=shard["resource_snapshots"]))
        environment = dict(shard["environment"], engine_cpu_parallelism=engine.get("cpu_parallelism"),
                           measurement_environment=shard["measurement_environment"],
                           profile_environment=shard["profile_environment"])
        require(isinstance(environment.get("host"), str) and environment["host"], "missing measured host")
        full.integer(environment["engine_cpu_parallelism"], "native CPU parallelism", 1)
        environment_id = hashlib.sha256(json_bytes(environment)).hexdigest()
        environments[environment_id] = environment
        input_key = f"fanout-{key[1]}" if fanout else f"scale-{key[1]}"
        if input_key in inputs:
            require(inputs[input_key] == shard["input_digest"], "pristine corpus differs across profiles/repetitions")
        inputs[input_key] = shard["input_digest"]
        groups[group].append(sample | dict(environment_id=environment_id))
    require(seen == expected and len(identities) == 1, "missing isolated slots or mixed source/binary/study")
    results = []
    for name, samples in sorted(groups.items()):
        require(len(samples) == 30, "profile group must retain all N30")
        strata = defaultdict(list)
        samples.sort(key=lambda sample: int(sample["sample"].rsplit("-", 1)[1]))
        for sample in samples:
            strata[sample["environment_id"]].append(sample)
        results.append(dict(group=name, n=30, samples=samples,
            environment_strata=[dict(environment_id=identity, **full.summarize_group(name, values))
                                for identity, values in sorted(strata.items())],
            pooled_latency_statistics="not_computed_heterogeneous_coverage"))
    require(any(sample.get("first_build_incomplete") is True for name, samples in groups.items()
                if name.startswith("fanout-") for sample in samples), "no actual over-budget closure observed")
    return dict(schema=SCHEMA, stage_scope=SCOPE, status="complete_isolated_profile_coverage", passed=True,
                study=shards[0]["study"], engine=json.loads(next(iter(identities))), registered_repetitions=30,
                sample_count=1350, mutation_sample_count=1200, fanout_sample_count=150,
                setup_pair_count=1200, setup_pairs=setup, groups=results, environments=environments,
                capacity_contract=full.capacity_contract(), input_digests=inputs,
                evidence=[dict(directory=s["directory"], receipt_sha256=s["receipt_sha256"]) for s in shards],
                history="fresh pristine A/B per profile; not the old sequential warm/history distribution",
                deadline_scope="five hours per worker INCLUDING setup and both complete parity comparisons; 1350 workers",
                total_time_allowance="6750 worker-hours; not equivalent splitting of old 150-worker/750-hour study",
                scope="P8-006 isolated mutation/fanout coverage; setup is not new P8-005 or query/P8-008 credit",
                resources_scope="original worker snapshots, not peaks or process-tree proof",
                statistics_scope="N30 per group with environment strata; no pooled stable tails",
                release_certification="not_run", G8="not_evaluated", task_statuses_changed=False)


def aggregate(build_directory, directories, output, root=None):
    out = full.new_directory(output)
    result = dict(schema=SCHEMA, stage_scope=SCOPE, status="failed", passed=False, release_certification="not_run")
    try:
        outer, built, binary = validate_build(build_directory, root)
        shards = [validate_shard(path, outer, built, binary, build_directory, root) for path in directories]
        result = combine(shards)
        result["driver_source"] = outer["driver_source"]
        result["profile_build_receipt_sha256"] = file_sha256(Path(build_directory) / "profile-build.json")
    except ERRORS as error:
        result["error"] = str(error)
    full.write_new(out / "profile-matrix.json", result)
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
