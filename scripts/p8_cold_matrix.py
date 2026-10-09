#!/usr/bin/env python3
"""Separate P8-005 cold-only study: 5 scales x 30 original cold pairs.

This protocol cannot admit old full-stage excerpts or certify incremental/query
work. It shares the unchanged build/parity/resource primitives, with its own
scope, sealed observer closure, study identity and artifact envelope.
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

SCHEMA = "p8-cold-only-study-v1"
SCOPE = "cold_only_v1"
DRIVER_FILES = full.DRIVER_FILES + (
    "scripts/p8_cold_matrix.py", "scripts/p8_runner_capacity.py",
    ".github/workflows/p8-cold.yml",
)
require = full.require
ERRORS = (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError)
ENVIRONMENT_KEYS = ("CODECORTEX_SEED_CACHE_MAX_SYMBOLS", "RUSTFLAGS",
                    "CARGO_ENCODED_RUSTFLAGS", "TMPDIR", "TMP", "TEMP")


def driver_snapshot(root):
    root = Path(root).resolve(strict=True)
    old = full.driver_snapshot(root)
    require(Path(__file__).resolve() == root / "scripts/p8_cold_matrix.py",
            "cold observer loaded from another checkout")
    inputs = dict(old["inputs"])
    for relative in DRIVER_FILES[len(full.DRIVER_FILES):]:
        path = root / relative
        require(path.is_file() and not path.is_symlink(), "nonregular cold observer input")
        committed = subprocess.check_output(["git", "show", old["source_commit"] + ":" + relative], cwd=root)
        require(file_sha256(path) == hashlib.sha256(committed).hexdigest(), "cold observer source drift: " + relative)
        inputs[relative] = file_sha256(path)
    require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip() == old["source_commit"],
            "cold observer HEAD changed")
    return dict(source_commit=old["source_commit"], inputs=inputs,
                manifest_sha256=hashlib.sha256(json_bytes(inputs)).hexdigest())


def study_identity(run_id, attempt):
    require(isinstance(run_id, str) and run_id.isascii() and run_id.isdecimal() and 0 < int(run_id) < 2 ** 64,
            "a positive official workflow run ID is required")
    full.integer(attempt, "run attempt", 1, 2 ** 32 - 1)
    return dict(protocol=SCHEMA, run_id=run_id, run_attempt=attempt)


def registered_plan(scale, shard_index, shard_count=30, repetitions=30, seed=12648430,
                    deadline_ms=18_000_000, capacity_profile=full.CAPACITY_PROFILE, *, study):
    require(type(repetitions) is int and repetitions == 30 and type(shard_count) is int and shard_count == 30
            and seed == 12648430 and deadline_ms == 18_000_000 and capacity_profile == full.CAPACITY_PROFILE,
            "cold-only registered population/seed/budget/capacity differs")
    plan = full.registered_plan(scale, shard_index, shard_count, repetitions, seed, deadline_ms, capacity_profile)
    require(full.exact_equal(study, study_identity(study["run_id"], study["run_attempt"])), "cold study identity differs")
    plan.update(stage_scope=SCOPE, skip_fanout=True,
                cold_study={key: study[key] for key in ("run_id", "run_attempt")})
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
            "cold " + kind + " failed or belongs to another protocol")
    study = record["study"]
    require(full.exact_equal(study, study_identity(study["run_id"], study["run_attempt"])), "study identity differs")
    require(record.get("driver_source") == driver_snapshot(root), "cold observer identity differs")
    require(record.get("files") == full.inventory(directory, (filename,)), "cold envelope inventory changed")
    return record


def build(root, output, run_id, attempt, target_directory=None):
    root = Path(root).resolve(strict=True)
    out = full.new_directory(output)
    record = envelope(out, "cold-build", study_identity(run_id, attempt), driver_snapshot(root))
    try:
        native = full.build(root, out / "native-build", target_directory or out.parent / (out.name + "-cargo"))
        require(native["status"] == "passed", "native release build failed")
        require(record["driver_source"] == driver_snapshot(root), "cold build observer changed")
        record.update(status="passed", passed=True, source_commit=native["source_commit"],
                      source_manifest_sha256=native["source_manifest_sha256"],
                      binary_sha256=native["binary_sha256"], binary_blake3=native["binary_blake3"],
                      native_build_receipt_sha256=file_sha256(out / "native-build/build.json"))
    except ERRORS as error:
        record["error"] = str(error)
    return finish(out, "cold-build.json", record)


def validate_build(directory, root=None):
    root = Path(root or Path(__file__).resolve().parents[1]).resolve(strict=True)
    directory = Path(directory).resolve(strict=True)
    outer = verify_envelope(directory, "cold-build.json", "cold-build", root)
    built, binary = full.validate_build(directory / "native-build", root)
    require(outer["native_build_receipt_sha256"] == file_sha256(directory / "native-build/build.json"),
            "cold build inner receipt differs")
    for key in ("source_commit", "source_manifest_sha256", "binary_sha256", "binary_blake3"):
        require(outer[key] == built[key], "cold build " + key + " differs")
    return outer, built, binary


def observed_environment(temporary_root):
    result = {key: os.environ.get(key) for key in ENVIRONMENT_KEYS}
    result.update({key: str(temporary_root) for key in ("TMPDIR", "TMP", "TEMP")})
    return result


def run_shard(root, build_directory, output, scale, shard_index, run_id, attempt):
    root = Path(root).resolve(strict=True)
    study = study_identity(run_id, attempt)
    built, _, _ = validate_build(build_directory, root)
    require(built["study"] == study, "build belongs to another study/run/attempt")
    out = full.new_directory(output)
    record = envelope(out, "cold-shard", study, driver_snapshot(root))
    record["cold_build_receipt_sha256"] = file_sha256(Path(build_directory) / "cold-build.json")
    record["measurement_environment"] = observed_environment(out)
    try:
        inner = full.run_shard(root, Path(build_directory) / "native-build", out / "native-shard",
                               registered_plan(scale, shard_index, study=study))
        require(inner["status"] == "passed", "native cold shard failed")
        require(record["driver_source"] == driver_snapshot(root), "cold observer changed during measurement")
        require(record["measurement_environment"] == observed_environment(out), "measurement environment changed")
        record.update(status="passed", passed=True)
    except ERRORS as error:
        record["error"] = str(error)
    return finish(out, "cold-shard.json", record)


def inspect_raw(path, plan):
    inspected = full.inspect_raw(path, plan, cold_only=True)
    snapshots = {}
    with Path(path).open("rb") as stream:
        for line in stream:
            event = full.decode(line)
            if event["event"] == "run_started":
                inspected["cold_environment"] = event["cold_environment"]
            elif event["event"] == "build_finished":
                require("process_snapshot" in event and isinstance(event.get("resource_scope"), str),
                        "original worker resource observation missing")
                snapshots[event["label"]] = dict(process_snapshot=event["process_snapshot"],
                                                 resource_scope=event["resource_scope"])
    require(len(snapshots) == 2 and len(inspected["measurements"]) == 1, "cold pair/sample count differs")
    inspected["measurements"][0]["resource_snapshots"] = snapshots
    return inspected


def validate_shard(directory, outer_build, built, binary, build_directory, root=None):
    root = Path(root or Path(__file__).resolve().parents[1]).resolve(strict=True)
    directory = Path(directory).resolve(strict=True)
    record = verify_envelope(directory, "cold-shard.json", "cold-shard", root)
    require(record["study"] == outer_build["study"] and record["driver_source"] == outer_build["driver_source"],
            "cold shard belongs to another study/observer")
    require(record["cold_build_receipt_sha256"] == file_sha256(Path(build_directory) / "cold-build.json"),
            "cold shard used another outer build")
    inspected = full._validate_shard(directory / "native-shard", built, binary,
        file_sha256(Path(build_directory) / "native-build/build.json"),
        protocol_plan=lambda *args: registered_plan(*args, study=record["study"]), raw_inspector=inspect_raw)
    summary = full.read_json(directory / "native-shard/native/worker-summary.json")
    require(summary.get("stage_scope") == SCOPE and summary.get("sample_count") == 1, "native cold terminal summary differs")
    measured = inspected["cold_environment"]
    seed = measured["seed_cache_max_symbols"]
    full.integer(seed.get("effective"), "effective seed cache capacity")
    parsed = seed.get("parsed_usize")
    require(parsed is None or type(parsed) is int and parsed >= 0, "invalid native seed cap parse result")
    require(seed["effective"] == (500_000 if parsed is None else parsed), "seed cache effective capacity differs")
    environment = record["measurement_environment"]
    require(set(environment) == set(ENVIRONMENT_KEYS), "cold environment keys differ")
    require(seed.get("raw") == environment["CODECORTEX_SEED_CACHE_MAX_SYMBOLS"] and
            seed.get("read_status") == ("absent" if seed["raw"] is None else "unicode"),
            "worker seed cache environment differs")
    report = full.read_json(directory / "native-shard/native/report.json")
    temporary = report.get("cold_temporary_environment", {})
    parent = full.producer_path(temporary.get("parent_root"), "supervisor temporary root")
    worker = full.producer_path(temporary.get("worker_root"), "worker temporary root")
    require(worker.parent == parent and str(parent) == environment["TMPDIR"],
            "worker temporary root is not the supervisor-owned child on the registered filesystem")
    worker_environment = {key: environment[key] for key in ENVIRONMENT_KEYS[1:]}
    worker_environment.update({key: str(worker) for key in ("TMPDIR", "TMP", "TEMP")})
    require(measured.get("runtime_environment") == worker_environment, "worker runtime environment differs")
    inner = full.read_json(directory / "native-shard/shard.json")
    require({key: environment[key] for key in ("TMPDIR", "TMP", "TEMP")} == inner["temporary_environment"],
            "cold temporary filesystem differs from capacity preflight")
    return inspected | dict(directory=str(directory), receipt_sha256=file_sha256(directory / "cold-shard.json"),
                            study=record["study"], driver_source=record["driver_source"],
                            build_receipt_sha256=record["cold_build_receipt_sha256"],
                            measurement_environment=environment)


def combine(shards):
    expected = {(scale, index) for scale in full.SCALES for index in range(30)}
    seen, groups, identities, environments, inputs = set(), defaultdict(list), set(), {}, {}
    for shard in shards:
        plan = shard["plan"]
        key = (plan["files"][0], plan["shard"]["index"])
        require(key in expected and key not in seen, "duplicate/unexpected cold slot")
        require(full.exact_equal(plan, registered_plan(*key, study=shard["study"])), "cold registered plan differs")
        seen.add(key)
        engine = shard["engine"]
        identity = {key: engine.get(key) for key in ("engine_head_observed", "source_files_digest", "cargo_lock_digest",
                    "binary_digest", "eval_version", "os", "arch", "rustflags", "eval_debug_assertions")}
        identity.update(study=shard["study"], driver_source=shard["driver_source"],
                        build_receipt_sha256=shard["build_receipt_sha256"])
        identities.add(json.dumps(identity, sort_keys=True))
        require(len(shard["measurements"]) == 1, "cold shard must retain exactly one original cold pair")
        sample = shard["measurements"][0]
        require(sample["group"] == f"scale-{key[0]}/cold" and sample["sample"] == f"scale-{key[0]}/repetition-{key[1]}",
                "cold sample/slot identity differs")
        environment = dict(shard["environment"], engine_cpu_parallelism=engine.get("cpu_parallelism"),
                           measurement_environment=shard["measurement_environment"],
                           cold_environment=shard["cold_environment"])
        require(isinstance(environment.get("host"), str) and environment["host"], "missing cold host")
        full.integer(environment["engine_cpu_parallelism"], "native CPU parallelism", 1)
        environment_id = hashlib.sha256(json_bytes(environment)).hexdigest()
        environments[environment_id] = environment
        if key[0] in inputs:
            require(inputs[key[0]] == shard["input_digest"], "same-scale cold corpus changed")
        inputs[key[0]] = shard["input_digest"]
        groups[sample["group"]].append(sample | dict(environment_id=environment_id))
    require(seen == expected and len(identities) == 1, "missing cold slots or mixed source/binary/study")
    results = []
    for name, samples in sorted(groups.items()):
        require(len(samples) == 30, "cold scale does not have all 30 repetitions")
        strata = defaultdict(list)
        samples.sort(key=lambda sample: int(sample["sample"].rsplit("-", 1)[1]))
        for sample in samples:
            strata[sample["environment_id"]].append(sample)
        results.append(dict(group=name, n=30, samples=samples,
            environment_strata=[dict(environment_id=identity, **full.summarize_group(name, values))
                                for identity, values in sorted(strata.items())],
            pooled_latency_statistics="not_computed_heterogeneous_coverage"))
    return dict(schema=SCHEMA, stage_scope=SCOPE, status="complete_cold_measurement_coverage", passed=True,
                study=shards[0]["study"], engine=json.loads(next(iter(identities))), registered_repetitions=30, shard_count_per_scale=30,
                scales=list(full.SCALES), sample_count=150, groups=results, environments=environments,
                capacity_contract=full.capacity_contract(), input_digests=inputs,
                evidence=[dict(directory=s["directory"], receipt_sha256=s["receipt_sha256"]) for s in shards],
                scope="P8-005 cold pairs only; no incremental/fanout/query measurement credit",
                resources_scope="original worker snapshots retained; not peaks or process-tree proof",
                cold_definition="fresh projects, empty indexes/parse caches; OS page cache retained",
                statistics_scope="all 30 samples per scale; host/CPU/kernel/environment strata, no pooled stable tails",
                release_certification="not_run", G8="not_evaluated", task_statuses_changed=False)


def aggregate(build_directory, directories, output, root=None):
    out = full.new_directory(output)
    result = dict(schema=SCHEMA, stage_scope=SCOPE, status="failed", passed=False, release_certification="not_run")
    try:
        outer, built, binary = validate_build(build_directory, root)
        shards = [validate_shard(path, outer, built, binary, build_directory, root) for path in directories]
        result = combine(shards)
        result["driver_source"] = outer["driver_source"]
        result["cold_build_receipt_sha256"] = file_sha256(Path(build_directory) / "cold-build.json")
    except ERRORS as error:
        result["error"] = str(error)
    full.write_new(out / "cold-matrix.json", result)
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
        if mode == "aggregate":
            command.add_argument("--inputs", nargs="+", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.mode == "build":
            result = build(args.root, args.output, args.run_id, args.attempt, args.target_directory)
        elif args.mode == "run":
            result = run_shard(args.root, args.build, args.output, args.scale, args.shard_index, args.run_id, args.attempt)
        else:
            result = aggregate(args.build, args.inputs, args.output, args.root)
        print(json.dumps({key: result.get(key) for key in ("status", "passed", "error", "sample_count")}))
        return 0 if result.get("passed") is True else 1
    except ERRORS as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
