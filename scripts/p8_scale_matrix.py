#!/usr/bin/env python3
"""Build, execute and replay an immutable, completely covered P8 scale matrix.

Each native shard retains its original 512 MiB maximum and global N >= 30.
The separate fanout workload belongs to the 1k scale's shards, exactly once.
Aggregation consumes original raw records, not a collection of green summaries.
This certifies measurement coverage only, never G8, quality or a release.
"""
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys

from p7_build_identity import file_sha256, json_bytes, source_snapshot, verify_release_profile


SCALES = (1000, 5000, 10000, 50000, 100000)
BATCHES = (1, 10, 100, 1000)
FANOUTS = (1, 4, 16, 64, 128)
TABLES = frozenset(("document_manifest", "resolution_frontier", "semantic_edges",
                   "dispatch_sites", "resolution_manifests", "resolution_dependencies",
                   "public_surfaces", "files", "symbols", "imports", "symbol_refs",
                   "call_edges", "chunks", "test_edges", "routes"))
PHASES = ("scan_diff_ms", "parse_ms", "resolve_ms", "write_ms", "postprocess_ms", "analysis_ms")
BUILD_STAGES = ("prepare_us", "commit_write_us", "postprocess_compute_us", "postprocess_apply_us", "between_stages_us")
BUILD_TIMING = BUILD_STAGES + ("total_us", "prepare_snapshot_us", "full_staging_us")
HEX = re.compile(r"[0-9a-f]{64}\Z")
MAX_BYTES = 512 * 1024 * 1024
MAX_LINE_BYTES = 64 * 1024 * 1024
SCHEMA = "p8-scale-matrix-v1"
DRIVER_FILES = ("scripts/p8_scale_matrix.py", "scripts/p7_build_identity.py")
CAPACITY_PROFILE = "scale_capacity_v1"
WIDE_DIRTY_PROFILE = "scale_wide_dirty_v1"
CAPACITY_PROFILES = (CAPACITY_PROFILE, WIDE_DIRTY_PROFILE)
STRATIFIED_ENVIRONMENT = "heterogeneous_stratified_coverage_v1"


def capacity_contract(profile=CAPACITY_PROFILE):
    require(profile in CAPACITY_PROFILES, "unknown capacity profile")
    if profile == WIDE_DIRTY_PROFILE:
        # The work configuration changes; the oracle's physical limits do not.
        return capacity_contract() | {"id": WIDE_DIRTY_PROFILE,
                "scale_dirty_budget": 4096, "oracle_capacity_profile": CAPACITY_PROFILE}
    return {"id": CAPACITY_PROFILE, "environment_policy": STRATIFIED_ENVIRONMENT,
            "scale_dirty_budget": 200, "scale_max_resume_builds": 1024,
            "fanout_dirty_budget": 8, "fanout_max_resume_builds": 128,
            "oracle_limits": {"max_rows_per_table": 5_000_000,
                "max_canonical_bytes": 16 * 1024 ** 3, "max_scratch_bytes": 8 * 1024 ** 3,
                "max_row_bytes": 1024 ** 2}, "sorting_cache_kib": 2048,
            "storage_layout": "without_rowid_value_ordinal_v1",
            "one_repetition_per_shard": True, "deadline_ms": 18_000_000,
            "max_output_bytes": MAX_BYTES,
            "minimum_free_bytes_per_file": 256 * 1024,
            "minimum_free_fixed_bytes": 4 * 1024 ** 3,
            "disk_requirement_scope": "preregistered capacity estimate, not an observed peak or sufficiency proof"}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value, name, minimum=0, maximum=None):
    require(type(value) is int and value >= minimum and
            (maximum is None or value <= maximum), "invalid integer: " + name)
    return value


def exact_equal(a, b):
    """Retain JSON number kinds; Python's True == 1 is not Value equality."""
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(exact_equal(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(exact_equal(x, y) for x, y in zip(a, b))
    return a == b


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON object key: " + key)
        result[key] = value
    return result


def decode(data):
    def finite_float(text):
        value = float(text)
        require(math.isfinite(value), "nonfinite JSON number")
        return value
    return json.loads(data, object_pairs_hook=unique_object,
                      parse_float=finite_float,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError("nonfinite JSON: " + value)))


def read_json(path, limit=8 * 1024 * 1024):
    path = Path(path)
    require(path.is_file() and not path.is_symlink(), "missing/nonregular JSON: " + str(path))
    require(path.stat().st_size <= limit, "JSON byte bound: " + str(path))
    return decode(path.read_bytes())


def write_new(path, value):
    with Path(path).open("xb") as stream:
        stream.write(json_bytes(value))


def new_directory(path):
    path = Path(path).absolute()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.mkdir()
    return path


def utc():
    return datetime.now(timezone.utc).isoformat()


def digest_ok(value):
    return isinstance(value, str) and HEX.fullmatch(value) is not None


def native_digest(binary, path):
    result = subprocess.run([str(binary), "--hash-file", str(path)], check=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120)
    value = result.stdout.decode().strip()
    require(digest_ok(value), "native evidence hash response is invalid")
    return value


def inventory(directory, exclude=()):
    result = {}
    for path in sorted(Path(directory).rglob("*")):
        require(not path.is_symlink(), "symlink in evidence inventory")
        if path.is_file():
            relative = path.relative_to(directory).as_posix()
            if relative not in exclude:
                result[relative] = {"bytes": path.stat().st_size, "sha256": file_sha256(path)}
        else:
            require(path.is_dir(), "nonregular evidence input")
    return result


def runtime_environment(include_host=False):
    cpu = None
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                cpu = line.partition(":")[2].strip()
                break
    except OSError:
        pass
    result = {"system": platform.system(), "release": platform.release(),
            "machine": platform.machine(), "cpu_model": cpu,
            "cpu_count": os.cpu_count(), "rustflags": os.environ.get("RUSTFLAGS")}
    if include_host:
        result["host"] = platform.node()
    return result


def verify_artifact(artifact, root=None):
    target = artifact.get("target", {})
    require(artifact.get("reason") == "compiler-artifact" and target.get("name") == "p8-scale"
            and target.get("kind") == ["bin"] and artifact.get("features") in ([], ["default"]),
            "Cargo record is not the default p8-scale executable")
    verify_release_profile(artifact.get("profile"))
    for key, suffix in (("manifest_path", "/crates/cc-eval/Cargo.toml"),):
        require(isinstance(artifact.get(key), str) and artifact[key].endswith(suffix),
                "unexpected Cargo manifest")
    require(isinstance(target.get("src_path"), str) and
            target["src_path"].endswith("/crates/cc-eval/src/bin/p8-scale.rs"), "unexpected Cargo source")
    if root is not None:
        require(Path(artifact["manifest_path"]).resolve() == root / "crates/cc-eval/Cargo.toml" and
                Path(target["src_path"]).resolve() == root / "crates/cc-eval/src/bin/p8-scale.rs",
                "Cargo artifact belongs to another source checkout")


def validate_snapshot(snapshot):
    inputs = snapshot.get("inputs")
    require(isinstance(inputs, dict) and {"Cargo.toml", "Cargo.lock",
            "crates/cc-eval/Cargo.toml", "crates/cc-eval/src/bin/p8-scale.rs",
            "crates/cc-eval/src/benchmark/p8_scale.rs", "crates/cc-eval/src/benchmark/oracle.rs",
            "crates/cc-eval/src/benchmark/oracle/streaming.rs"} <= set(inputs),
            "incomplete preserved build source inventory")
    require(snapshot.get("input_count") == len(inputs) and all(digest_ok(value) for value in inputs.values()),
            "invalid preserved source count or file digest")
    require(snapshot.get("manifest_sha256") == hashlib.sha256(json_bytes(inputs)).hexdigest(),
            "preserved source manifest digest differs")
    for field in ("source_commit", "source_tree"):
        require(isinstance(snapshot.get(field), str) and re.fullmatch(r"[0-9a-f]{40}", snapshot[field]),
                "invalid preserved " + field)


def driver_snapshot(root):
    """Bind the code doing admission/replay to its exact committed Git blobs."""
    root = Path(root).resolve(strict=True)
    loaded = (Path(__file__).resolve(), Path(source_snapshot.__code__.co_filename).resolve())
    require(loaded == tuple(root / path for path in DRIVER_FILES),
            "matrix driver/helper loaded from a different checkout")
    commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    inputs = {}
    for relative, path in zip(DRIVER_FILES, loaded):
        require(path.is_file() and not path.is_symlink(), "matrix driver/helper is not regular")
        current = file_sha256(path)
        committed = subprocess.check_output(["git", "show", f"{commit}:{relative}"], cwd=root)
        require(current == hashlib.sha256(committed).hexdigest(),
                "matrix driver/helper differs from committed source: " + relative)
        inputs[relative] = current
    require(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip() == commit,
            "matrix driver source changed during snapshot")
    return {"source_commit": commit, "inputs": inputs,
            "manifest_sha256": hashlib.sha256(json_bytes(inputs)).hexdigest()}


def build_command(target):
    return ["cargo", "build", "--release", "--locked", "-p", "cc-eval", "--bin", "p8-scale",
            "--message-format=json", "--target-dir", str(target)]


def producer_path(value, name):
    """Check a preserved absolute producer path without touching a prior VM."""
    require(isinstance(value, str) and value, "missing producer " + name)
    path = Path(value)
    require(path.is_absolute() and str(path) == value and ".." not in path.parts,
            "noncanonical producer " + name)
    return path


def verify_build_origin(record, cargo):
    root = producer_path(record.get("build_root"), "checkout")
    target = producer_path(record.get("target_directory"), "target directory")
    require(record.get("command") == build_command(target), "original Cargo command/target differs")
    artifact = record.get("cargo_artifact", {})
    verify_artifact(artifact)
    require(artifact.get("manifest_path") == str(root / "crates/cc-eval/Cargo.toml") and
            artifact.get("target", {}).get("src_path") == str(root / "crates/cc-eval/src/bin/p8-scale.rs"),
            "Cargo artifact differs from exact producer checkout")
    executable = producer_path(artifact.get("executable"), "executable")
    require(target in executable.parents and executable.name == "p8-scale",
            "original executable differs from selected Cargo target")
    candidates = [message for message in cargo if message.get("reason") == "compiler-artifact" and
                  message.get("target", {}).get("name") == "p8-scale"]
    require(candidates == [artifact], "Cargo artifact missing/duplicated in original log")
    original = record.get("copy_source", {})
    require(isinstance(original, dict) and set(original) == {"path", "bytes", "sha256"} and original.get("path") == str(executable),
            "missing exact original executable copy source")
    integer(original.get("bytes"), "original executable bytes", 1)
    require(digest_ok(original.get("sha256")) and record.get("binary_bytes") == original["bytes"] and
            record.get("binary_sha256") == original["sha256"], "original executable/copy identity differs")


def build(root, output, target_directory=None):
    root = Path(root).resolve(strict=True)
    out = new_directory(output)
    target = (Path(target_directory).absolute() if target_directory else out.parent / (out.name + "-cargo")).resolve()
    require(target != out and out not in target.parents, "Cargo target must stay outside the evidence bundle")
    before = source_snapshot(root)
    driver = driver_snapshot(root)
    require(driver["source_commit"] == before["source_commit"], "build/driver source commits differ")
    write_new(out / "source-before.json", before)
    command = build_command(target)
    record = {"schema": SCHEMA, "kind": "build", "status": "failed", "started_utc": utc(),
              "command": command, "source_commit": before["source_commit"],
              "build_root": str(root), "target_directory": str(target),
              "source_manifest_sha256": before["manifest_sha256"],
              "driver_sha256": file_sha256(__file__), "driver_source": driver,
              "environment": runtime_environment(),
              "toolchain": {name: subprocess.check_output([name, "--version", "--verbose"], text=True).strip()
                            for name in ("cargo", "rustc")}}
    try:
        with (out / "cargo.jsonl").open("xb") as stdout, (out / "cargo.stderr").open("xb") as stderr:
            result = subprocess.run(command, cwd=root, stdout=stdout, stderr=stderr)
        record["exit_code"] = result.returncode
        after = source_snapshot(root)
        write_new(out / "source-after.json", after)
        require(before == after, "build source changed during compilation")
        require(driver_snapshot(root) == driver, "driver changed during compilation")
        require(result.returncode == 0, "Cargo build failed")
        messages = [decode(line) for line in (out / "cargo.jsonl").read_bytes().splitlines() if line]
        require(messages and messages[-1] == {"reason": "build-finished", "success": True}, "missing Cargo completion")
        candidates = [m for m in messages if m.get("reason") == "compiler-artifact" and
                      m.get("target", {}).get("name") == "p8-scale"]
        require(len(candidates) == 1, "expected one p8-scale Cargo artifact")
        artifact = candidates[0]
        verify_artifact(artifact, root)
        produced = Path(artifact["executable"]).resolve(strict=True)
        require(target in produced.parents and str(produced) == artifact["executable"] and produced.name == "p8-scale",
                "executable outside selected Cargo target or noncanonical producer path")
        original = {"path": str(produced), "bytes": produced.stat().st_size, "sha256": file_sha256(produced)}
        shutil.copy2(produced, out / "p8-scale")
        record.update(cargo_artifact=artifact, copy_source=original,
                      binary_bytes=(out / "p8-scale").stat().st_size,
                      binary_sha256=file_sha256(out / "p8-scale"),
                      binary_blake3=native_digest(out / "p8-scale", out / "p8-scale"))
        require(file_sha256(produced) == original["sha256"] and produced.stat().st_size == original["bytes"],
                "original executable changed while copying")
        verify_build_origin(record, messages)
        record["status"] = "passed"
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        record["error"] = str(error)
    record["finished_utc"] = utc()
    record["files"] = inventory(out)
    write_new(out / "build.json", record)
    return record


def validate_build(directory, root=None):
    directory = Path(directory).resolve(strict=True)
    record = read_json(directory / "build.json")
    require(record.get("schema") == SCHEMA and record.get("kind") == "build" and
            record.get("status") == "passed" and record.get("exit_code") == 0, "build receipt is not successful")
    require(record.get("files") == inventory(directory, ("build.json",)), "build evidence changed")
    before = read_json(directory / "source-before.json")
    validate_snapshot(before)
    require(before == read_json(directory / "source-after.json"), "build source snapshots differ")
    require(before["source_commit"] == record["source_commit"] and
            before["manifest_sha256"] == record["source_manifest_sha256"], "build source identity differs")
    driver = driver_snapshot(root if root is not None else Path(__file__).resolve().parents[1])
    require(record.get("driver_source") == driver and driver["source_commit"] == before["source_commit"] and
            record.get("driver_sha256") == driver["inputs"][DRIVER_FILES[0]],
            "build/execution/replay driver or helper source differs")
    if root is not None:
        require(source_snapshot(root) == before, "execution checkout differs from built source")
    cargo = [decode(line) for line in (directory / "cargo.jsonl").read_bytes().splitlines() if line]
    require(cargo and cargo[-1] == {"reason": "build-finished", "success": True}, "Cargo completion changed")
    verify_build_origin(record, cargo)
    binary = directory / "p8-scale"
    require(binary.stat().st_size == record["binary_bytes"] and file_sha256(binary) == record["binary_sha256"] and
            native_digest(binary, binary) == record["binary_blake3"], "preserved build executable changed")
    return record, binary


def registered_plan(scale, shard_index, shard_count, repetitions=30, seed=12648430, deadline_ms=18_000_000,
                    capacity_profile=None):
    require(scale in SCALES, "noncanonical scale")
    integer(repetitions, "registered repetitions", 30, 200)
    integer(shard_count, "shard count", 1, repetitions)
    integer(shard_index, "shard index", 0, shard_count - 1)
    integer(deadline_ms, "deadline", 1, 86_400_000)
    integer(seed, "seed", 0, (1 << 64) - 1)
    require(capacity_profile is None or capacity_profile in CAPACITY_PROFILES, "unknown capacity profile")
    plan = dict(schema_version=1, profile="release", files=[scale], seed=seed,
                repetitions=repetitions, shard=dict(index=shard_index, count=shard_count),
                skip_fanout=scale != SCALES[0], dirty_budget=8, max_resume_builds=128,
                batch_sizes=list(BATCHES), fanouts=list(FANOUTS),
                deadline_ms=deadline_ms, max_output_bytes=MAX_BYTES)
    if capacity_profile in CAPACITY_PROFILES:
        contract = capacity_contract(capacity_profile)
        require(shard_count == repetitions and deadline_ms == contract["deadline_ms"],
                "capacity profile requires one repetition per shard and its registered deadline")
        plan.update(capacity_profile=capacity_profile, dirty_budget=contract["scale_dirty_budget"],
                    max_resume_builds=contract["scale_max_resume_builds"])
    return plan


def disk_preflight(temporary_root, plan):
    """Observe the actual filesystem where the supervised native fixtures live."""
    contract = capacity_contract()
    required = plan["files"][0] * contract["minimum_free_bytes_per_file"] + contract["minimum_free_fixed_bytes"]
    actual = os.statvfs(temporary_root)
    available = actual.f_bavail * actual.f_frsize
    return {"schema": "p8-scale-disk-preflight-v1", "observed_utc": utc(),
            "temporary_root": str(temporary_root), "filesystem_device": os.stat(temporary_root).st_dev,
            "required_free_bytes": required, "available_bytes": available,
            "passed": available >= required, "scope": contract["disk_requirement_scope"]}


def repetition_range(plan):
    n, shard = plan["repetitions"], plan["shard"]
    return range(n * shard["index"] // shard["count"], n * (shard["index"] + 1) // shard["count"])


def run_shard(root, build_directory, output, plan):
    root = Path(root).resolve(strict=True)
    built, binary = validate_build(build_directory, root)
    out = new_directory(output)
    write_new(out / "registered-plan.json", plan)
    before = source_snapshot(root)
    write_new(out / "source-before.json", before)
    command = [str(binary), "--profile", "release", "--files", str(plan["files"][0]),
               "--seed", str(plan["seed"]), "--repetitions", str(plan["repetitions"]),
               "--shard-index", str(plan["shard"]["index"]), "--shard-count", str(plan["shard"]["count"]),
               "--dirty-budget", str(plan["dirty_budget"]), "--max-resume-builds", str(plan["max_resume_builds"]),
               "--batch-sizes", ",".join(map(str, plan["batch_sizes"])),
               "--fanouts", ",".join(map(str, plan["fanouts"])), "--deadline-ms", str(plan["deadline_ms"]),
               "--max-output-bytes", str(plan["max_output_bytes"]), "--output", str(out / "native")]
    if plan["skip_fanout"]:
        command.append("--skip-fanout")
    capacity = plan.get("capacity_profile")
    if capacity is not None:
        require(capacity in CAPACITY_PROFILES, "unknown capacity profile")
        command.extend(("--capacity-profile", capacity))
    if "stage_scope" in plan:
        require(plan["stage_scope"] in ("cold_only_v1", "profile_isolated_v1", "profile_task_descriptive_v1"), "unknown explicit stage scope")
        study = plan["cold_study"] if plan["stage_scope"] == "cold_only_v1" else plan["profile_study"]
        command.extend(("--stage-scope", plan["stage_scope"],
                        "--study-run-id", study["run_id"],
                        "--study-attempt", str(study["run_attempt"])))
        if plan["stage_scope"] in ("profile_isolated_v1", "profile_task_descriptive_v1"):
            command.extend(("--mutation-profile", study["mutation_profile"]))
            if study["fanout"] is not None:
                command.extend(("--profile-fanout", str(study["fanout"])))
    record = {"schema": SCHEMA, "kind": "shard", "status": "failed", "started_utc": utc(),
              "command": command, "plan": plan, "build_receipt_sha256": file_sha256(Path(build_directory) / "build.json"),
              "source_commit": built["source_commit"], "source_manifest_sha256": built["source_manifest_sha256"],
              "binary_sha256": built["binary_sha256"], "binary_blake3": built["binary_blake3"],
              "driver_source": built["driver_source"],
              "environment": runtime_environment(include_host=capacity is not None), "release_certification": "not_run"}
    try:
        native_environment = None
        if capacity is not None:
            record["capacity_contract"] = capacity_contract(capacity)
            temporary_root = out.parent
            record["temporary_environment"] = {key: str(temporary_root) for key in ("TMPDIR", "TMP", "TEMP")}
            preflight = disk_preflight(temporary_root, plan)
            write_new(out / "disk-preflight.json", preflight)
            if not preflight["passed"]:
                record["status"] = "not_run"
                record["exit_code"] = None
                raise ValueError("insufficient registered disk capacity; native measurement not run")
            native_environment = os.environ | record["temporary_environment"]
        with (out / "supervisor.stdout").open("xb") as stdout, (out / "supervisor.stderr").open("xb") as stderr:
            result = subprocess.run(command, cwd=root, stdout=stdout, stderr=stderr,
                                    timeout=plan["deadline_ms"] / 1000 + 120, env=native_environment)
        record["exit_code"] = result.returncode
        after = source_snapshot(root)
        write_new(out / "source-after.json", after)
        require(before == after, "source changed during measured shard")
        require(driver_snapshot(root) == built["driver_source"], "driver changed during measured shard")
        require(file_sha256(binary) == built["binary_sha256"], "binary changed during measured shard")
        require(result.returncode == 0, "native shard failed")
        record["status"] = "passed"
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        record["error"] = str(error)
    if capacity is not None:
        write_new(out / "disk-after.json", disk_preflight(out.parent, plan))
    record["finished_utc"] = utc()
    record["files"] = inventory(out)
    write_new(out / "shard.json", record)
    return record


def check_build_report(report):
    require(isinstance(report, dict), "missing original IndexReport")
    for field in ("files_scanned", "files_added", "files_updated", "files_removed", "files_skipped",
                  "files_parsed", "symbols_total", "chunks_total", "elapsed_ms"):
        integer(report.get(field), field)
    require(report.get("parse_errors") == [], "original parse errors are not empty")
    require(isinstance(report.get("dirty_plan"), dict) and isinstance(report.get("project_model"), dict),
            "missing original phase work reports")
    require(isinstance(report.get("document_changes"), dict), "missing document change report")
    phases = report.get("phase_timing", {})
    for phase in PHASES:
        integer(phases.get(phase), phase)
    complete = report.get("resolution_freshness", {}).get("complete")
    require(type(complete) is bool, "missing explicit closure status")
    timing = report.get("build_timing", {})
    require(timing.get("schema_version") == 1, "missing complete stage timing schema")
    for key in BUILD_TIMING:
        if key != "full_staging_us":
            integer(timing.get(key), "complete timing " + key)
    require("full_staging_us" in timing, "full staging execution status missing")
    if timing["full_staging_us"] is not None:
        integer(timing["full_staging_us"], "full staging microseconds")
    partition = sum(timing[key] for key in BUILD_STAGES)
    require(0 <= timing["total_us"] - partition <= 6, "complete stage timing does not conserve wall time")
    require(timing["total_us"] >= report["elapsed_ms"] * 1000, "complete clock ends before original report elapsed time")
    require(timing["prepare_snapshot_us"] + (timing["full_staging_us"] or 0) <= timing["prepare_us"],
            "nested snapshot/staging exceeds prepare stage")
    return {"elapsed_ms": report["elapsed_ms"], "phases": phases, "complete": complete,
            "build_timing": timing,
            "files_added": report["files_added"], "files_updated": report["files_updated"],
            "files_removed": report["files_removed"]}


def sum_build_timing(reports):
    result = {key: sum(report["build_timing"][key] for report in reports) for key in BUILD_TIMING if key != "full_staging_us"}
    staging = [report["build_timing"]["full_staging_us"] for report in reports]
    require(all(value is None for value in staging) or all(value is not None for value in staging),
            "mixed full/incremental staging timing")
    result["full_staging_us"] = None if all(value is None for value in staging) else sum(staging)
    return result


def check_counts(counts):
    require(isinstance(counts, dict) and set(counts.get("tables", {})) == TABLES, "incomplete table counts")
    for key, value in counts["tables"].items():
        integer(value, "table count " + key)
    for name in ("files", "symbols", "chunks"):
        integer(counts.get(name), "flattened " + name)
        require(counts.get(name) == counts["tables"][name], "flattened table count differs")
    for name in ("call_edges", "semantic_edges", "test_edges"):
        integer(counts.get("edges", {}).get(name), "edge " + name)
        require(counts.get("edges", {}).get(name) == counts["tables"][name], "edge count differs")
    require(counts.get("vectors", {}).get("state") == "disabled" and
            "count" in counts["vectors"] and counts["vectors"]["count"] is None,
            "disabled vectors misrepresented as measured workload")
    integer(counts.get("index_db_with_wal_shm_bytes"), "index bytes", 1)


def check_parity(parity, plan=None):
    require(parity.get("equal") is True and parity.get("status") == "equal" and
            parity.get("different_tables") == [], "incomplete or failed full parity")
    tables = parity.get("tables", [])
    require(len(tables) == len(TABLES) and {row.get("table") for row in tables} == TABLES,
            "parity must contain all fifteen unique tables")
    for row in tables:
        require(row.get("equal") is True and row.get("different_row_count") == 0,
                "a canonical table has a recorded difference")
        integer(row.get("incremental_rows"), "incremental row count")
        integer(row.get("full_rows"), "full row count")
        require(row["incremental_rows"] == row.get("full_rows"), "canonical row multiplicity differs")
        require(digest_ok(row.get("incremental_digest")) and digest_ok(row.get("full_digest")),
                "missing full canonical row digests")
        # Different encodings of equal signed zero may legitimately have
        # different digests. The original full Value comparison is retained.
    for side in ("incremental_counts", "full_counts"):
        check_counts(parity.get(side))
        require(all(parity[side]["tables"][row["table"]] == row[
                    "incremental_rows" if side == "incremental_counts" else "full_rows"] for row in tables),
                "count/parity row inventory differs")
    if plan is not None and plan.get("capacity_profile") in CAPACITY_PROFILES:
        require(parity.get("capacity_profile") == CAPACITY_PROFILE, "parity capacity profile differs")
        over = any(value > 100_000 for side in ("incremental_counts", "full_counts")
                   for value in parity[side]["tables"].values())
        if over:
            contract = capacity_contract()
            require(parity.get("oracle_mode") == "disk_backed_exact_v1" and
                    exact_equal(parity.get("limits"), contract["oracle_limits"]) and
                    parity.get("sorting_cache_kib") == contract["sorting_cache_kib"] and
                    parity.get("storage_layout") == contract["storage_layout"], "capacity oracle limits/layout differ")
            integer(parity.get("canonical_bytes"), "canonical bytes", 1, contract["oracle_limits"]["max_canonical_bytes"])
            integer(parity.get("scratch_peak_bytes"), "scratch bytes", 1, contract["oracle_limits"]["max_scratch_bytes"])


def check_fact(fact, config_files, changed):
    target = config_files[1 if changed else 0]
    require(fact == dict(consumer=config_files[2], expected_target=target, actual_targets=[target], passed=True),
            "independent config target assertion failed or changed")


def compact_build(event, started):
    require(event.get("label") == started.get("label") and event.get("full") is started.get("full") and
            event.get("start_us") == started.get("start_us"), "build start/finish identity differs")
    start = integer(event.get("start_us"), "build start")
    end = integer(event.get("end_us"), "build end", start)
    require(event.get("wall_us") == end - start, "build wall range differs")
    result = check_build_report(event.get("report"))
    require(result["build_timing"]["total_us"] <= end - start, "inner build clock exceeds enclosing MCP call")
    require((result["build_timing"]["full_staging_us"] is not None) == event["full"],
            "full staging measurement does not match actual build mode")
    result.update(wall_us=end - start, full=event["full"])
    return result


def inspect_raw(path, plan, *, cold_only=False):
    # Existing public full/cold callers cannot opt into a partial profile.
    return _inspect_raw(path, plan, cold_only=cold_only)


def _inspect_raw(path, plan, *, cold_only=False, isolated_profile=None,
                 profile_scope="profile_isolated_v1"):
    """Shared complete-record checks; new scope is selected only by its driver."""
    require(type(cold_only) is bool and not (cold_only and isolated_profile is not None), "mixed raw protocols")
    require(profile_scope in ("profile_isolated_v1", "profile_task_descriptive_v1")
            and (isolated_profile is not None or profile_scope == "profile_isolated_v1"),
            "profile scope requires an explicit profile inspector")
    scope = profile_scope if isolated_profile is not None else "cold_only_v1" if cold_only else None
    require(plan.get("stage_scope") == scope, "raw stage protocol differs")
    if isolated_profile is not None:
        require(isolated_profile in ("no_op", "body", "api", "config", "batch_1", "batch_10", "batch_100", "batch_1000", "fanout")
                and plan["profile_study"]["mutation_profile"] == isolated_profile, "raw isolated profile differs")
    require(not cold_only or plan["skip_fanout"] is True, "cold-only protocol contains fanout")
    scale, reps = plan["files"][0], set(repetition_range(plan))
    stages = ("no_op", "body", "api", "config") + tuple(f"batch_{n}" for n in plan["batch_sizes"])
    if cold_only or isolated_profile == "fanout":
        stages = ()
    elif isolated_profile is not None:
        stages = (isolated_profile,)
    expected_samples = set() if isolated_profile == "fanout" else {f"scale-{scale}/repetition-{r}" for r in reps}
    inputs, starts, builds, mutations, cold, finished = {}, {}, {}, {}, {}, {}
    used_builds, fanout_started, fanout_finished, measurements = set(), {}, {}, []
    engine = None
    sequence = 0
    last_build_end_us = 0
    require(path.stat().st_size <= plan["max_output_bytes"], "raw exceeds registered evidence budget")
    with path.open("rb") as raw:
        while line := raw.readline(MAX_LINE_BYTES + 1):
            require(len(line) <= MAX_LINE_BYTES and line.endswith(b"\n"), "oversized/truncated raw record")
            event = decode(line)
            kind = event.get("event")
            if kind == "run_started":
                require(sequence == 0 and event.get("plan") == plan, "run header/plan mismatch")
                require(event.get("release_certification") == "not_run" and event.get("shard_only") is True,
                        "single shard claimed full certification")
                engine = event.get("engine")
                require(isinstance(engine, dict), "missing raw engine provenance")
            elif kind == "input":
                sample = event.get("sample")
                require(sample in expected_samples and sample not in inputs, "unexpected/duplicate input sample")
                require(event.get("seed") == plan["seed"] and event.get("synthetic_files_requested") == scale and
                        event.get("synthetic_files_written") == scale and event.get("auxiliary_visible_config_files") == 1,
                        "input count/seed differs from registered corpus")
                records = event.get("files", [])
                paths = [row["path"] for row in records]
                require(len(records) == scale + 1 and paths == sorted(set(paths)) and "tsconfig.json" in paths,
                        "missing/duplicate generated input files")
                for row in records:
                    require(not row["path"].startswith("/") and ".." not in row["path"].split("/"), "unsafe input path")
                    integer(row.get("bytes"), "generated file bytes", 1)
                    require(digest_ok(row.get("digest")), "missing generated file digest")
                require(digest_ok(event.get("input_digest")), "missing whole input digest")
                require(event.get("batch_target_policy") == "generated_code_then_existing_route_and_yaml_v2",
                        "batch workload policy differs")
                runtime = event.get("runtime_config")
                expected_runtime = {"auto_index": {"enabled": False}, "indexing": {
                    "dirty_propagation": True, "dirty_propagation_max_files": plan["dirty_budget"],
                    "db_read_pool_size": 1, "max_concurrent_parse": 2}, "semantic": {
                        "enabled": False, "network_opt_in": False, "allow_query_network": False}}
                require(exact_equal(runtime, expected_runtime), "runtime config drift")
                config = event.get("config_fixture_augmented_code_files")
                require(isinstance(config, list) and len(config) == 3 and len(set(config)) == 3 and
                        all(p in paths and p.endswith(".ts") for p in config), "config fixture witness changed")
                inputs[sample] = dict(digest=event["input_digest"], paths=set(paths), config=config)
            elif kind == "build_started":
                label = event.get("label")
                require(isinstance(label, str) and label not in starts, "duplicate build identity")
                integer(event.get("start_us"), "ordered build start", last_build_end_us)
                starts[label] = event
            elif kind == "build_finished":
                label = event.get("label")
                require(label in starts and label not in builds, "missing/duplicate build completion")
                builds[label] = compact_build(event, starts[label])
                last_build_end_us = event["end_us"]
            elif kind == "cold_parity":
                sample = event.get("sample")
                require(sample in inputs and sample not in cold, "unexpected/duplicate cold comparison")
                labels = [sample + "/cold_incremental", sample + "/cold_full_control"]
                require(all(label in builds and builds[label]["full"] is True and builds[label]["complete"] is True
                            for label in labels), "missing/incomplete independent cold builds")
                check_parity(event.get("parity", {}), plan)
                check_fact(event.get("independent_config_fact"), inputs[sample]["config"], False)
                integer(event.get("parity_wall_us"), "cold parity wall")
                cold[sample] = event["parity"]["incremental_counts"]
                used_builds.update(labels)
                measurements.append(dict(group=f"scale-{scale}/cold", sample=sample,
                                         engine_ms=builds[labels[0]]["elapsed_ms"],
                                         index_wall_us=builds[labels[0]]["wall_us"],
                                         full_control_ms=builds[labels[1]]["elapsed_ms"],
                                         parity_wall_us=event["parity_wall_us"], counts=cold[sample],
                                         build_timing=sum_build_timing([builds[labels[0]]]),
                                         full_control_build_timing=sum_build_timing([builds[labels[1]]]),
                                         phases=builds[labels[0]]["phases"]))
            elif kind == "mutation":
                label = event.get("label", "")
                sample, _, stage = label.rpartition("/")
                require(sample in cold and stage in stages and label not in mutations, "unexpected/duplicate mutation")
                expected = 0 if stage == "no_op" else int(stage[6:]) if stage.startswith("batch_") else 1
                operations, receipts = event.get("operations", []), event.get("receipts", [])
                require(event.get("requested_files") == expected and len(operations) == len(receipts) == expected,
                        "mutation count differs or batch was silently shrunk")
                paths = [op.get("path") for op in operations]
                require(len(set(paths)) == expected and set(paths) <= inputs[sample]["paths"], "mutation not on unique existing inputs")
                require(paths == [receipt.get("path") for receipt in receipts], "mutation receipt order differs")
                repetition = int(sample.rsplit("-", 1)[1])
                if stage == "config":
                    require(paths == ["tsconfig.json"] and decode(operations[0].get("content", "")) == {
                        "compilerOptions": {"baseUrl": ".", "paths": {"@p8/config": ["./" + inputs[sample]["config"][1]]}}},
                        "config mutation did not switch the registered target")
                elif stage.startswith("batch_"):
                    require("tsconfig.json" not in paths, "batch changed the independent config control")
                    for op in operations:
                        comment = "#" if op["path"].endswith((".py", ".yaml")) else "//"
                        require(op.get("content", "").endswith(f"\n{comment} p8 batch {expected} repetition {repetition}\n"),
                                "batch mutation body differs from registered workload")
                elif stage == "body":
                    require(f"p8_body_{repetition}" in operations[0].get("content", ""), "body mutation marker missing")
                elif stage == "api":
                    require(f"p8_offset: number = {repetition + 1}" in operations[0].get("content", ""), "API mutation marker missing")
                for op, receipt in zip(operations, receipts):
                    require(isinstance(op.get("content"), str) and len(op["content"].encode()) == receipt.get("bytes"),
                            "mutation byte count differs")
                    require(digest_ok(receipt.get("before_digest")) and digest_ok(receipt.get("after_digest")) and
                            receipt["before_digest"] != receipt["after_digest"], "mutation is unchanged or unbound")
                mutations[label] = len(operations)
            elif kind == "stage_finished":
                label = event.get("label", "")
                sample, _, stage = label.rpartition("/")
                require(label in mutations and label not in finished and stage in stages, "unexpected/duplicate stage")
                count = integer(event.get("incremental_builds"), "incremental builds", 1, plan["max_resume_builds"] + 1)
                labels = [f"{label}/incremental-{n}" for n in range(count)]
                control = label + "/full_control"
                require(all(key in builds and builds[key]["full"] is False for key in labels) and
                        control in builds and builds[control]["full"] is True, "missing native incremental/full report")
                require(event.get("passed") is True and event.get("complete") is True and event.get("full_complete") is True and
                        builds[labels[-1]]["complete"] is True and builds[control]["complete"] is True,
                        "incomplete closure cannot certify full parity")
                require(all(builds[key]["complete"] is False for key in labels[:-1]), "unexpected resume after completed closure")
                check_parity(event.get("parity", {}), plan)
                check_fact(event.get("independent_config_fact"), inputs[sample]["config"],
                           stage == "config" or (isolated_profile is None and stage.startswith("batch_")))
                if stage == "no_op":
                    require(all(builds[labels[0]][key] == 0 for key in ("files_added", "files_updated", "files_removed")),
                            "no-op changed indexed files")
                require(event.get("no_op_unchanged") is True, "native no-op assertion failed")
                integer(event.get("incremental_wall_us"), "incremental wall")
                require(event["incremental_wall_us"] >= sum(builds[key]["wall_us"] for key in labels),
                        "outer incremental clock is smaller than its enclosed builds")
                integer(event.get("parity_wall_us"), "parity wall")
                used_builds.update(labels + [control])
                finished[label] = True
                measurements.append(dict(group=f"scale-{scale}/{stage}", sample=sample,
                    engine_ms=sum(builds[key]["elapsed_ms"] for key in labels),
                    index_wall_us=sum(builds[key]["wall_us"] for key in labels),
                    outer_incremental_wall_us=event["incremental_wall_us"],
                    full_control_ms=builds[control]["elapsed_ms"], parity_wall_us=event["parity_wall_us"],
                    builds=count, build_timing=sum_build_timing([builds[key] for key in labels]),
                    full_control_build_timing=sum_build_timing([builds[control]]),
                    phases={phase: sum(builds[key]["phases"][phase] for key in labels) for phase in PHASES}))
            elif kind == "fanout_started":
                key = (event.get("fanout"), event.get("repetition"))
                require(not plan["skip_fanout"] and key[0] in plan["fanouts"] and key[1] in reps and
                        key not in fanout_started, "unexpected/duplicate fanout start")
                fanout_started[key] = event.get("case")
            elif kind == "fanout_finished":
                key = (event.get("fanout"), event.get("repetition"))
                require(key in fanout_started and key not in fanout_finished, "unexpected/duplicate fanout completion")
                fanout, repetition = key
                case, result = fanout_started[key], event.get("result", {})
                require(event.get("passed") is True and result.get("passed") is True and result.get("failure_signature") is None,
                        "fanout replay failed")
                require(event.get("fixture_files") == fanout + 1 and set(case.get("initial", {})) ==
                        {"api.ts"} | {f"use_{i:03}.ts" for i in range(fanout)}, "fanout fixture file count differs")
                fanout_dirty, fanout_resume = ((8, 128) if plan.get("capacity_profile") in CAPACITY_PROFILES
                                                else (plan["dirty_budget"], plan["max_resume_builds"]))
                require(case.get("dirty_budget") == fanout_dirty and case.get("max_resume_builds") == fanout_resume,
                        "fanout closure budget differs")
                require(set(result.get("tables", [])) == TABLES and len(result["tables"]) == 15, "fanout oracle table set changed")
                checkpoints = result.get("checkpoints", [])
                require(len(checkpoints) == 1, "fanout must retain the actual checkpoint")
                point = checkpoints[0]
                require(point.get("status") == "compared" and point.get("settle_required") is True and point.get("different_tables") == [],
                        "fanout parity incomplete")
                require(set(point.get("incremental", {})) == TABLES and set(point.get("full", {})) == TABLES and
                        exact_equal(point["incremental"], point["full"]), "fanout original full canonical facts differ")
                expected_truth = [{"id": f"caller_{i}", "expected": 1, "actual": 1, "passed": True} for i in range(fanout)]
                require(point.get("truth") == expected_truth, "fanout independent truth receipts differ")
                for i in range(fanout):
                    wanted = dict(file_path=f"use_{i:03}.ts", callee_symbol="scale_ping", target_file_path="api.ts")
                    actual = sum(all(row.get(k) == v for k, v in wanted.items()) for row in point["incremental"]["call_edges"])
                    require(actual == 1, "fanout hand-authored call edge missing or duplicated")
                reports = [check_build_report(report) for report in point.get("reports", [])]
                require(1 <= len(reports) <= fanout_resume + 1 and reports[-1]["complete"] is True and all(r["complete"] is False for r in reports[:-1]),
                        "fanout closure not complete or resumed after completion")
                require(all(r["build_timing"]["full_staging_us"] is None for r in reports),
                        "fanout incremental reports contain full staging timing")
                require(event.get("incremental_builds") == len(reports) and event.get("incremental_engine_elapsed_ms") ==
                        sum(r["elapsed_ms"] for r in reports), "fanout time attribution differs from raw reports")
                incomplete = reports[0]["complete"] is False
                require(event.get("first_build_incomplete") is incomplete, "fanout budget claim differs from actual first report")
                integer(event.get("wall_us"), "whole fanout replay wall")
                require(event["wall_us"] >= sum(r["build_timing"]["total_us"] for r in reports),
                        "whole fanout replay clock is smaller than incremental work")
                fanout_finished[key] = True
                measurements.append(dict(group=f"fanout-{fanout}", sample=f"fanout-{fanout}/repetition-{repetition}",
                    engine_ms=event["incremental_engine_elapsed_ms"], whole_fixture_wall_us=event["wall_us"],
                    builds=len(reports), first_build_incomplete=incomplete,
                    build_timing=sum_build_timing(reports),
                    phases={phase: sum(r["phases"][phase] for r in reports) for phase in PHASES}))
            else:
                raise ValueError("failed, unknown or not-run raw event: " + str(kind))
            sequence += 1
    require(engine is not None and set(inputs) == set(cold) == expected_samples, "missing full scale input/cold samples")
    expected_stages = {sample + "/" + stage for sample in expected_samples for stage in stages}
    require(set(mutations) == set(finished) == expected_stages, "missing registered mutation stages")
    require(set(starts) == set(builds) == used_builds, "missing, incomplete or unconsumed native builds")
    selected_fanouts = [plan["profile_study"]["fanout"]] if isolated_profile == "fanout" else plan["fanouts"]
    expected_fanouts = set() if plan["skip_fanout"] else {(fanout, rep) for fanout in selected_fanouts for rep in reps}
    require(set(fanout_started) == set(fanout_finished) == expected_fanouts, "missing registered fanout samples")
    if isolated_profile == "fanout":
        require(len(fanout_started) == 1, "isolated fanout requires exactly one fixture")
        input_digest = hashlib.sha256(json_bytes(next(iter(fanout_started.values()))["initial"])).hexdigest()
    else:
        require(len({record["digest"] for record in inputs.values()}) == 1, "same-seed input digest changed across repetitions")
        input_digest = next(iter(inputs.values()))["digest"]
    return dict(engine=engine, input_digest=input_digest, measurements=measurements)


def validate_shard(directory, build_record, binary, build_receipt_sha256):
    return _validate_shard(directory, build_record, binary, build_receipt_sha256,
                           protocol_plan=registered_plan, raw_inspector=inspect_raw)


def _validate_shard(directory, build_record, binary, build_receipt_sha256, *,
                    protocol_plan, raw_inspector):
    """Common immutable evidence checks; public full-stage admission stays fixed."""
    directory = Path(directory).resolve(strict=True)
    record = read_json(directory / "shard.json")
    require(record.get("schema") == SCHEMA and record.get("kind") == "shard" and record.get("status") == "passed" and
            record.get("exit_code") == 0, "shard execution failed or is missing")
    require(record.get("files") == inventory(directory, ("shard.json",)), "shard evidence changed after sealing")
    require(record.get("build_receipt_sha256") == build_receipt_sha256, "shard used another build receipt")
    for key in ("source_commit", "source_manifest_sha256", "binary_sha256", "binary_blake3", "driver_source"):
        require(record.get(key) == build_record[key], "shard " + key + " mismatch")
    before = read_json(directory / "source-before.json")
    validate_snapshot(before)
    require(before == read_json(directory / "source-after.json") and before["source_commit"] == build_record["source_commit"] and
            before["manifest_sha256"] == build_record["source_manifest_sha256"], "shard source drift")
    plan = record["plan"]
    expected = protocol_plan(plan["files"][0], plan["shard"]["index"], plan["shard"]["count"],
                               plan["repetitions"], plan["seed"], plan["deadline_ms"], plan.get("capacity_profile"))
    require(exact_equal(plan, expected) and plan == read_json(directory / "registered-plan.json") and
            plan == read_json(directory / "native/plan.json"), "shard plan differs from fixed matrix protocol")
    if plan.get("capacity_profile") in CAPACITY_PROFILES:
        require(exact_equal(record.get("capacity_contract"), capacity_contract(plan["capacity_profile"])),
                "registered capacity contract changed")
        preflight = read_json(directory / "disk-preflight.json")
        expected_minimum = plan["files"][0] * capacity_contract()["minimum_free_bytes_per_file"] + capacity_contract()["minimum_free_fixed_bytes"]
        require(preflight.get("schema") == "p8-scale-disk-preflight-v1" and preflight.get("passed") is True and
                preflight.get("required_free_bytes") == expected_minimum, "missing registered disk preflight")
        integer(preflight.get("available_bytes"), "preflight available bytes", expected_minimum)
        require(record.get("temporary_environment") == {key: preflight.get("temporary_root") for key in ("TMPDIR", "TMP", "TEMP")},
                "preflight did not cover the registered native temporary filesystem")
        require(isinstance(record["environment"].get("host"), str) and record["environment"]["host"], "missing measured host identity")
    report, summary = read_json(directory / "native/report.json"), read_json(directory / "native/worker-summary.json")
    native_files = inventory(directory / "native")
    require(set(native_files) == {"plan.json", "raw.jsonl", "worker.stderr", "worker-summary.json", "report.json"}
            and sum(value["bytes"] for value in native_files.values()) <= plan["max_output_bytes"],
            "native evidence file inventory/budget differs")
    require(report.get("profile") == "release" and report.get("max_output_bytes") == plan["max_output_bytes"],
            "native profile/output budget differs")
    require(report.get("exit_code") == 0 and report.get("worker_exit_code") == 0 and report.get("status") == "measurement_complete" and
            report.get("stderr_complete") is True and report.get("fixture_cleanup", {}).get("error") is None,
            "native supervisor did not complete successfully")
    require(report.get("worker_binary_unchanged") is True and report.get("worker_binary_digest_before") == build_record["binary_blake3"] and
            report.get("worker_binary_digest_after") == build_record["binary_blake3"], "native binary provenance differs")
    for file, field in (("plan.json", "plan_digest"), ("raw.jsonl", "raw_digest")):
        require(native_digest(binary, directory / "native" / file) == report.get(field), "native " + file + " digest mismatch")
    require(report.get("summary") == summary and summary.get("passed") is True and report.get("shard_only") is True and
            summary.get("shard_only") is True and report.get("registered_repetitions") == plan["repetitions"],
            "native summary/shard coverage differs")
    expected_range = dict(start=repetition_range(plan).start, end=repetition_range(plan).stop)
    require(report.get("executed_repetition_range") == summary.get("executed_repetition_range") == expected_range,
            "native global sample range differs")
    require(report.get("release_certification") == summary.get("release_certification") == "not_run" and
            report.get("full_100k_certification") == summary.get("full_100k_certification") == "not_run",
            "a shard cannot certify a full release")
    inspected = raw_inspector(directory / "native/raw.jsonl", plan)
    engine = inspected["engine"]
    require(engine.get("eval_debug_assertions") is False and engine.get("engine_head_observed") == build_record["source_commit"] and
            engine.get("binary_digest") == build_record["binary_blake3"] and digest_ok(engine.get("source_files_digest")) and
            digest_ok(engine.get("cargo_lock_digest")), "raw engine provenance is not the built release candidate")
    require(summary.get("sample_count") == len(inspected["measurements"]) and summary.get("raw_bytes") ==
            (directory / "native/raw.jsonl").stat().st_size, "summary count/raw byte coverage differs")
    groups = summary.get("groups", [])
    expected_groups = Counter(item["group"] for item in inspected["measurements"])
    require(len(groups) == len(expected_groups) and {group.get("group") for group in groups} == set(expected_groups),
            "native summary group coverage differs")
    for group in groups:
        require(group.get("samples") == group.get("passed") == expected_groups[group["group"]] and
                group.get("failed_or_not_compared") == 0, "native summary masks failed samples")
    return dict(directory=str(directory), receipt_sha256=file_sha256(directory / "shard.json"),
                plan=plan, environment=record["environment"], **inspected)


def statistics(values):
    require(values and all(isinstance(value, (int, float)) and math.isfinite(value) for value in values), "invalid raw latency population")
    return {"n": len(values), "min": min(values), "max": max(values), "mean": sum(values) / len(values), "samples": values}


def summarize_group(name, samples):
    n = len(samples)
    group = {"group": name, "n": n, "sample_ids": [sample["sample"] for sample in samples],
             "engine_ms": statistics([s["engine_ms"] for s in samples]),
             "phase_ms": {phase: statistics([s["phases"][phase] for s in samples]) for phase in PHASES}}
    for timing_scope in ("build_timing", "full_control_build_timing"):
        reports = [sample[timing_scope] for sample in samples if timing_scope in sample]
        if not reports:
            require(timing_scope == "full_control_build_timing" and name.startswith("fanout-"),
                    "missing complete build timing population")
            continue
        require(len(reports) == n, "partly missing complete build timing population")
        group[timing_scope] = {}
        for key in BUILD_TIMING:
            values = [report[key] for report in reports]
            if all(value is None for value in values):
                require(key == "full_staging_us", "unexpected missing stage timing")
                group[timing_scope][key] = {"state": "not_executed", "n": 0, "samples": []}
            else:
                require(all(value is not None for value in values), "partly missing stage timing")
                group[timing_scope][key] = statistics(values)
    for metric in ("index_wall_us", "outer_incremental_wall_us", "full_control_ms", "parity_wall_us", "whole_fixture_wall_us", "builds"):
        present = [s[metric] for s in samples if metric in s]
        if present:
            require(len(present) == n, "partly missing metric: " + metric)
            group[metric] = statistics(present)
    if name.endswith("/cold"):
        group["counts_by_repetition"] = [sample["counts"] for sample in samples]
    if name.startswith("fanout-"):
        group["first_build_incomplete_samples"] = sum(sample["first_build_incomplete"] for sample in samples)
    return group


def combine(shards, repetitions=30, shard_count=6, capacity_profile=None):
    integer(repetitions, "aggregate repetitions", 30, 200)
    integer(shard_count, "aggregate shard count", 1, repetitions)
    require(capacity_profile is None or capacity_profile in CAPACITY_PROFILES, "unknown capacity profile")
    stratified = capacity_profile in CAPACITY_PROFILES
    if stratified:
        require(shard_count == repetitions, "capacity profile requires one repetition per shard")
    expected = {(scale, index) for scale in SCALES for index in range(shard_count)}
    seen, by_group, sources, environments, inputs, common = set(), defaultdict(list), set(), {}, {}, set()
    for shard in shards:
        plan = shard["plan"]
        key = (plan["files"][0], plan["shard"]["index"])
        require(key in expected and key not in seen, "duplicate or unexpected matrix shard")
        require(plan["repetitions"] == repetitions and plan["shard"]["count"] == shard_count, "matrix repetition/shard population changed")
        require(plan.get("capacity_profile") == capacity_profile, "matrix capacity profile mismatch")
        if stratified:
            require(exact_equal(plan, registered_plan(key[0], key[1], shard_count, repetitions,
                    plan["seed"], plan["deadline_ms"], capacity_profile)), "matrix capacity parameters mismatch")
        seen.add(key)
        signature = dict(plan)
        for field in ("files", "shard", "skip_fanout"):
            signature.pop(field)
        common.add(json.dumps(signature, sort_keys=True))
        engine = shard["engine"]
        source_fields = ("engine_head_observed", "source_files_digest", "cargo_lock_digest", "binary_digest",
                         "eval_version", "os", "arch", "rustflags", "eval_debug_assertions")
        if not stratified:
            source_fields += ("cpu_parallelism",)
        sources.add(tuple(engine.get(key) for key in source_fields))
        environment = dict(shard["environment"])
        if stratified:
            require(isinstance(environment.get("host"), str) and environment["host"], "missing measured host identity")
            environment["engine_cpu_parallelism"] = engine.get("cpu_parallelism")
            integer(environment["engine_cpu_parallelism"], "native CPU parallelism", 1)
        environment_id = hashlib.sha256(json_bytes(environment)).hexdigest()
        environments[environment_id] = environment
        scale = plan["files"][0]
        if scale in inputs:
            require(inputs[scale] == shard["input_digest"], "matrix input changed between same-scale shards")
        inputs[scale] = shard["input_digest"]
        for sample in shard["measurements"]:
            by_group[sample["group"]].append(sample | {"environment_id": environment_id})
    require(seen == expected, "missing matrix shards: " + str(sorted(expected - seen)))
    require(len(common) == len(sources) == 1 and (stratified or len(environments) == 1),
            "matrix source/binary/profile/runtime/environment mismatch")
    stages = ("cold", "no_op", "body", "api", "config") + tuple(f"batch_{n}" for n in BATCHES)
    expected_groups = {f"scale-{scale}/{stage}" for scale in SCALES for stage in stages} | {f"fanout-{n}" for n in FANOUTS}
    require(set(by_group) == expected_groups, "missing or unexpected full matrix groups")
    groups = []
    for name, samples in sorted(by_group.items()):
        prefix = name.split("/")[0]
        expected_ids = {f"{prefix}/repetition-{r}" for r in range(repetitions)}
        require(len(samples) == repetitions and {sample["sample"] for sample in samples} == expected_ids,
                "missing/duplicate global repetition in " + name)
        samples.sort(key=lambda item: int(item["sample"].rsplit("-", 1)[1]))
        if stratified:
            strata = defaultdict(list)
            for sample in samples:
                strata[sample["environment_id"]].append(sample)
            group = {"group": name, "n": repetitions,
                     "sample_environments": [{"sample": s["sample"], "environment_id": s["environment_id"]} for s in samples],
                     "environment_strata": [{"environment_id": identity, **summarize_group(name, rows)}
                                             for identity, rows in sorted(strata.items())],
                     "pooled_latency_statistics": "not_computed_heterogeneous_coverage"}
            if name.startswith("fanout-"):
                group["first_build_incomplete_samples"] = sum(s["first_build_incomplete"] for s in samples)
        else:
            group = summarize_group(name, samples)
        groups.append(group)
    require(any(group.get("first_build_incomplete_samples", 0) > 0 for group in groups), "no actual over-budget closure was observed")
    common_engine = dict(shards[0]["engine"])
    if stratified:
        common_engine.pop("cpu_parallelism", None)
    result = {"schema": SCHEMA, "status": "complete_measurement_coverage", "passed": True,
            "registered_repetitions": repetitions, "shard_count_per_scale": shard_count,
            "scales": list(SCALES), "batch_sizes": list(BATCHES), "fanouts": list(FANOUTS),
            "fanout_owner_scale": SCALES[0], "sample_count": sum(len(s) for s in by_group.values()),
            "input_digests": inputs, "engine": common_engine, "groups": groups,
            "evidence": [{"directory": s["directory"], "receipt_sha256": s["receipt_sha256"]} for s in shards],
            "statistics_scope": "all registered samples; no best-of, causal speedup, stable p95/p99 or quality inference",
            "resources_scope": "original single-worker snapshots; whole-process-tree peaks not certified",
            "release_certification": "not_run", "G8": "not_evaluated", "task_statuses_changed": False}
    if stratified:
        result.update(capacity_contract=capacity_contract(capacity_profile), environment_policy=STRATIFIED_ENVIRONMENT,
                      environments=environments,
                      statistics_scope="all samples retained with host/CPU/kernel strata; N>=30 is global coverage only; per-stratum counts explicit; no pooled latency, same-environment effect, stable tail or quality inference")
    else:
        result["environment"] = next(iter(environments.values()))
    return result


def aggregate(build_directory, directories, output, repetitions=30, shard_count=6, capacity_profile=None):
    out = new_directory(output)
    result = {"schema": SCHEMA, "status": "failed", "passed": False, "release_certification": "not_run"}
    try:
        built, binary = validate_build(build_directory)
        build_digest = file_sha256(Path(build_directory) / "build.json")
        shards = [validate_shard(path, built, binary, build_digest) for path in directories]
        result = combine(shards, repetitions, shard_count, capacity_profile)
        result["build_receipt_sha256"] = build_digest
        result["replay_driver_sha256"] = file_sha256(__file__)
        result["driver_source"] = built["driver_source"]
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        result["error"] = str(error)
    write_new(out / "matrix.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="mode", required=True)
    compile_parser = commands.add_parser("build")
    compile_parser.add_argument("--root", type=Path, default=Path.cwd())
    compile_parser.add_argument("--output", type=Path, required=True)
    compile_parser.add_argument("--target-directory", type=Path)
    run_parser = commands.add_parser("run")
    run_parser.add_argument("--root", type=Path, default=Path.cwd())
    run_parser.add_argument("--build", type=Path, required=True)
    run_parser.add_argument("--output", type=Path, required=True)
    run_parser.add_argument("--scale", type=int, choices=SCALES, required=True)
    run_parser.add_argument("--shard-index", type=int, required=True)
    run_parser.add_argument("--shard-count", type=int, default=6)
    run_parser.add_argument("--repetitions", type=int, default=30)
    run_parser.add_argument("--seed", type=int, default=12648430)
    run_parser.add_argument("--deadline-ms", type=int, default=18_000_000)
    run_parser.add_argument("--capacity-profile", choices=CAPACITY_PROFILES)
    aggregate_parser = commands.add_parser("aggregate")
    aggregate_parser.add_argument("--build", type=Path, required=True)
    aggregate_parser.add_argument("--inputs", type=Path, nargs="+", required=True)
    aggregate_parser.add_argument("--output", type=Path, required=True)
    aggregate_parser.add_argument("--repetitions", type=int, default=30)
    aggregate_parser.add_argument("--shard-count", type=int, default=6)
    aggregate_parser.add_argument("--capacity-profile", choices=CAPACITY_PROFILES)
    args = parser.parse_args()
    try:
        if args.mode == "build":
            result = build(args.root, args.output, args.target_directory)
        elif args.mode == "run":
            plan = registered_plan(args.scale, args.shard_index, args.shard_count, args.repetitions, args.seed, args.deadline_ms, args.capacity_profile)
            result = run_shard(args.root, args.build, args.output, plan)
        else:
            result = aggregate(args.build, args.inputs, args.output, args.repetitions, args.shard_count, args.capacity_profile)
        print(json.dumps({key: result.get(key) for key in ("status", "passed", "error", "sample_count")}, indent=2))
        return 0 if result.get("status") == "passed" or result.get("passed") is True else 1
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
