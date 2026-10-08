#!/usr/bin/env python3
"""Execute the existing local benchmark from frozen, build-bound P8 inputs.

This supplies an actual execution witness to p8_release_evidence's input lock.
It preserves the original scorer, suite, raw gate, and build profile. It does
not approve P7/G8, optimize a dev binary, or read a holdout corpus.
"""
import sys
sys.dont_write_bytecode = True

import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import time

from p7_build_identity import file_sha256, json_bytes, source_snapshot, verify_release_profile
from p7_stdio_build_receipt import compiler_environment, toolchain_identity
import p8_release_evidence as lock


SUITE = "crates/cc-eval/benchmarks/manifests/p0-rust-api.json"
QUERIES = "crates/cc-eval/benchmarks/native/p0-rust-api.jsonl"
FIXTURE = "crates/cc-eval/fixtures/index-v2/rust-api"
CORPUS_FILES = (SUITE, QUERIES, FIXTURE + "/lib.rs", FIXTURE + "/provider.rs")
SCORER_FILES = tuple("crates/cc-eval/src/benchmark/" + name for name in (
    "schema.rs", "manifest.rs", "metrics.rs", "span_metrics.rs", "statistics.rs",
    "normalizer.rs", "gate.rs", "report.rs", "runner.rs"))
SOURCE_PATHS = ("Cargo.toml", "Cargo.lock", "crates", "scripts", ".github/workflows")
BUILD_VARIABLES = (
    "RUSTFLAGS", "RUSTDOCFLAGS", "RUSTUP_TOOLCHAIN", "SDKROOT", "CC", "AR", "CFLAGS",
    "CARGO_INCREMENTAL", "CARGO_BUILD_JOBS", "CARGO_PROFILE_DEV_DEBUG",
    "CARGO_PROFILE_TEST_DEBUG", "CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR",
    "CODECORTEX_BENCH_PROCESS_PROBE", "LANG", "LC_ALL", "TZ", "RAYON_NUM_THREADS")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(json_bytes(value))


def source_summary(root):
    return {key: value for key, value in source_snapshot(root).items() if key != "inputs"}


def environment_record():
    def text_file(path):
        try:
            return Path(path).read_text()[:16384].strip()
        except OSError:
            return None
    cpu = text_file("/proc/cpuinfo")
    memory = text_file("/proc/meminfo")
    cpu_model = next((line.split(":", 1)[1].strip() for line in (cpu or "").splitlines()
                      if line.startswith("model name") and ":" in line), None)
    memory_kib = next((int(line.split()[1]) for line in (memory or "").splitlines()
                       if line.startswith("MemTotal:")), None)
    return {"system": platform.system(), "kernel": platform.release(),
            "architecture": platform.machine(), "cpu_model": cpu_model,
            "logical_cpu_count": os.cpu_count(),
            "host_memory_bytes": memory_kib * 1024 if memory_kib is not None else None,
            "cgroup_cpu_max": text_file("/sys/fs/cgroup/cpu.max"),
            "cgroup_memory_max": text_file("/sys/fs/cgroup/memory.max"),
            "python": {"version": platform.python_version(),
                       "executable_sha256": file_sha256(sys.executable)},
            "variables": {key: os.environ.get(key) for key in BUILD_VARIABLES},
            "capture_policy": "explicit noncredential variables and hardware fields only",
            "provider": {"mode": "disabled", "live_cost": None,
                         "live_quality_certification": "not_run"}}


def execute(command, directory, root, environment=None, timeout=180):
    """Run an owned process, retaining exit and raw streams even on failure."""
    directory.mkdir(parents=True, exist_ok=False)
    started = time.monotonic()
    receipt = {"command": list(map(str, command)), "cwd": str(root),
               "started_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
               "timeout_seconds": timeout, "executable_sha256": file_sha256(command[0])}
    with (directory / "stdout.log").open("wb") as stdout, (directory / "stderr.log").open("wb") as stderr:
        process = subprocess.Popen(command, cwd=root, env=environment, stdin=subprocess.DEVNULL,
                                   stdout=stdout, stderr=stderr, start_new_session=True)
        receipt["pid"] = process.pid
        try:
            receipt["exit_code"] = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
            receipt.update(exit_code=None, timed_out=True, terminated_owned_process_group=True)
    receipt["elapsed_seconds"] = time.monotonic() - started
    receipt["raw_files"] = {name: file_sha256(directory / name)
                            for name in ("stdout.log", "stderr.log")}
    write(directory / "execution.json", receipt)
    return receipt


def artifact_from_log(path, target_name):
    selected, finished = [], []
    require(path.stat().st_size <= 256 * 1024 * 1024, "Cargo log exceeds receipt budget")
    with path.open() as stream:
        for line in stream:
            message = json.loads(line)
            if message.get("reason") == "build-finished":
                finished.append(message.get("success"))
            if (message.get("reason") == "compiler-artifact"
                    and message.get("target", {}).get("name") == target_name
                    and message.get("target", {}).get("kind") == ["bin"]):
                selected.append(message)
    require(finished == [True] and len(selected) == 1,
            "raw Cargo log must contain one successful finish and one requested binary artifact")
    return selected[0]


def validate_build(binary, receipt_path, root, expected_source, role):
    receipt = lock.read_json(receipt_path)
    expected_name, package = {"product": ("codecortex", "cc-server"),
                              "runner": ("cc-eval", "cc-eval")}[role]
    require(receipt.get("build_exit_code") == 0 and type(receipt["build_exit_code"]) is int,
            "a real successful build receipt is required")
    require(receipt["source_before"] == expected_source == receipt["source_after"],
            "build receipt source differs from the fixed candidate")
    require(receipt["binary_sha256"] == file_sha256(binary), "build/product bytes disagree")
    artifact = receipt["cargo_artifact"]
    target = artifact["target"]
    require(target["name"] == expected_name and target["kind"] == ["bin"]
            and artifact["reason"] == "compiler-artifact" and artifact["features"] == []
            and artifact["profile"]["test"] is False,
            "candidate requires the actual default local product and runner binaries")
    source_path = "src/main.rs" if role == "product" else "src/bin/cc-eval.rs"
    require(artifact["package_id"].rpartition("#")[0].endswith("/" + package)
            and Path(artifact["manifest_path"]) == root / "crates" / package / "Cargo.toml"
            and Path(target["src_path"]) == root / "crates" / package / source_path,
            "Cargo artifact belongs to a different package/source checkout")
    profile = receipt["build_profile"]
    require(profile in ("dev", "release"), "actual dev/release build profile is required")
    command = receipt["build_command"]
    require(command[:2] == ["cargo", "build"] and "--locked" in command
            and "--no-default-features" in command and "--profile" not in command
            and command[command.index("--bin") + 1] == expected_name
            and ("--release" in command) == (profile == "release"),
            "build profile or binary does not match the recorded Cargo command")
    if profile == "release":
        require(receipt.get("actual_cargo_profile") == artifact["profile"],
                "actual release profile differs from Cargo artifact")
        verify_release_profile(artifact["profile"])
    raw = receipt_path.parent / "cargo-build.jsonl"
    require(artifact_from_log(raw, expected_name) == artifact,
            "receipt artifact differs from the actual retained Cargo output")
    require(receipt["source_manifest"] == "source-inputs.json", "unexpected source manifest location")
    source_manifest = lock.read_json(receipt_path.parent / receipt["source_manifest"])
    require(source_manifest == source_snapshot(root)["inputs"], "retained source manifest drift")
    require(receipt.get("toolchain"), "compiler/toolchain build identity is missing")
    return {"receipt": receipt, "receipt_sha256": file_sha256(receipt_path),
            "raw_build_sha256": file_sha256(raw), "binary_sha256": file_sha256(binary),
            "source_manifest_sha256": file_sha256(receipt_path.parent / receipt["source_manifest"])}


def build_runner(args):
    root = Path(__file__).resolve().parents[1]
    output = lock.path(args.output, exists=False)
    output.mkdir(parents=True, exist_ok=False)
    args.output_owned = True
    before = source_snapshot(root)
    environment, overrides = compiler_environment(root)
    target = lock.path(args.target_dir, exists=False) if args.target_dir else output / "cargo-target"
    initially_absent = not target.exists()
    environment.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_BUILD_DIR=str(target))
    toolchain = toolchain_identity(root, environment)
    command = ["cargo", "build", "-p", "cc-eval", "--bin", "cc-eval", "--locked",
               "--no-default-features", "--message-format=json-render-diagnostics"]
    if args.release:
        command.append("--release")
    if args.offline:
        command.append("--offline")
    write(output / "source-inputs.json", before["inputs"])
    invocation = execute([shutil.which("cargo"), *command[1:]], output / "invocation", root,
                         environment, timeout=1800)
    shutil.copyfile(output / "invocation/stdout.log", output / "cargo-build.jsonl")
    shutil.copyfile(output / "invocation/stderr.log", output / "cargo-build.stderr.log")
    require(invocation["exit_code"] == 0, "cc-eval build failed; original output retained")
    require(source_snapshot(root) == before and toolchain_identity(root, environment) == toolchain,
            "source/toolchain changed while building cc-eval")
    artifact = artifact_from_log(output / "cargo-build.jsonl", "cc-eval")
    if args.release:
        verify_release_profile(artifact["profile"])
    executable = Path(artifact["executable"]).resolve(strict=True)
    require(executable.is_relative_to(target), "Cargo runner artifact escaped the chosen target")
    snapshot = output / "cc-eval"
    lock.file_record(executable, snapshot)
    summary = {key: value for key, value in before.items() if key != "inputs"}
    receipt = {"schema_version": 1, "package_kind": "eval-default", "build_command": command,
               "build_exit_code": 0, "binary_path": str(snapshot),
               "binary_sha256": file_sha256(snapshot), "cargo_artifact": artifact,
               "source_before": summary, "source_after": summary,
               "source_manifest": "source-inputs.json", "toolchain": toolchain,
               "compiler_environment": overrides, "cargo_target_dir": str(target),
               "cargo_build_dir": str(target), "cargo_target_initially_absent": initially_absent,
               "build_profile": "release" if args.release else "dev",
               "actual_cargo_profile": artifact["profile"],
               "builder_sha256": file_sha256(__file__),
               "build_environment": {key: environment.get(key) for key in BUILD_VARIABLES},
               "execution_receipt": "invocation/execution.json"}
    write(output / "build-receipt.json", receipt)
    return {"status": "runner_built", "binary": str(snapshot),
            "receipt": str(output / "build-receipt.json"), "source": summary}


def drift_controls(candidate, external_pin, output):
    """Mutate new disposable copies only; preserve the real candidate/run."""
    output.mkdir(parents=True, exist_ok=False)
    manifest = lock.verify_candidate(candidate, external_pin, current=False)
    source_file = next(path for path in sorted(manifest["source"]["entries"]) if path.endswith(".rs"))
    changes = [(role, "inputs/" + role) for role in ("binary", "config", "scoring", "model")]
    changes.extend((label, path) for label, path in (
        ("source", "source/" + source_file), ("query-and-embedded-gold", "corpus/" + QUERIES),
        ("corpus-source", "corpus/" + FIXTURE + "/provider.rs")))
    records = []
    for label, relative in changes:
        copy = output / label
        shutil.copytree(candidate, copy)
        changed = copy / relative
        original = file_sha256(changed)
        with changed.open("ab") as stream:
            stream.write(b"\n ")
        try:
            lock.verify_candidate(copy, external_pin, current=False)
        except (OSError, ValueError) as error:
            records.append({"case": label, "changed_path": relative, "rejected": True,
                            "before_sha256": original, "after_sha256": file_sha256(changed),
                            "error": str(error)})
        else:
            raise ValueError("candidate drift was accepted: " + label)
        finally:
            shutil.rmtree(copy)  # Only the new disposable copy owned above.
        lock.verify_candidate(candidate, external_pin, current=False)
    write(output / "results.json", records)
    return records


def run_candidate(args):
    root = Path(__file__).resolve().parents[1]
    output = lock.path(args.output, exists=False)
    output.mkdir(parents=True, exist_ok=False)
    args.output_owned = True
    expected_source = source_summary(root)
    require(expected_source["source_commit"] == args.expected_source_commit,
            "the caller's immutable source commit is not the current candidate")
    product, runner = lock.path(args.product), lock.path(args.runner)
    product_receipt, runner_receipt = lock.path(args.product_build_receipt), lock.path(args.runner_build_receipt)
    product_build = validate_build(product, product_receipt, root, expected_source, "product")
    runner_build = validate_build(runner, runner_receipt, root, expected_source, "runner")
    environment, _ = compiler_environment(root)
    tools = toolchain_identity(root, environment)
    require(product_build["receipt"]["toolchain"] == tools == runner_build["receipt"]["toolchain"],
            "product, runner, and current toolchain identities disagree")
    before_environment = environment_record()
    write(output / "environment.json", before_environment)
    corpus = output / "prepared-corpus"
    corpus_records = {relative: lock.file_record(root / relative, corpus / relative)
                      for relative in CORPUS_FILES}
    suite = lock.read_json(corpus / SUITE)
    require(suite["scoring"] == "codecortex-native-v1" and suite["source"]["commit"] is None,
            "unexpected original fixture/scoring contract")
    questions = [json.loads(line) for line in (corpus / QUERIES).read_text().splitlines() if line.strip()]
    require(questions and all(row["split"] == "dev" for row in questions),
            "only the existing authored DEV fixture is admitted")
    evidence = output / "evidence"
    evidence.mkdir()
    bound_builds = {}
    for role, executable, receipt_path, proof in (
            ("product", product, product_receipt, product_build),
            ("runner", runner, runner_receipt, runner_build)):
        destination = evidence / "builds" / role
        for name in ("build-receipt.json", "cargo-build.jsonl", "cargo-build.stderr.log",
                     proof["receipt"]["source_manifest"]):
            lock.file_record(receipt_path.parent / name, destination / name)
        lock.file_record(executable, destination / executable.name)
        bound_builds[role] = proof
    frozen_runner = evidence / "builds/runner" / runner.name
    require(file_sha256(frozen_runner) == runner_build["binary_sha256"], "runner copy changed")
    inputs = output / "prepared-inputs"
    write(inputs / "config.json", suite["engine_config"])
    query_digest = corpus_records[QUERIES]["sha256"]
    write(inputs / "corpus.json", {"split": "fixture", "source": "original P0 rust API DEV fixture",
          "corpus_sha256": lock.digest(corpus_records), "query_sha256": query_digest,
          "gold_sha256": query_digest, "gold_storage": "embedded in the same complete query JSONL",
          "file_records": corpus_records, "original_cc_eval_locks": {
              "algorithm": "BLAKE3", "source": suite["source"]["digest"],
              "queries": suite["queries_digest"]}})
    write(inputs / "model.json", {"mode": "disabled"})
    write(inputs / "scoring.json", {"scoring": suite["scoring"],
          "implementation": {path: file_sha256(root / path) for path in SCORER_FILES},
          "builds": bound_builds, "environment": before_environment,
          "driver_sha256": file_sha256(__file__),
          "scope": "original deterministic scorer; actual default product and runner; no live model"})
    validation = execute([str(frozen_runner), "validate", "--suite", str(corpus / SUITE)],
                         evidence / "commands/validate", root)
    require(validation["exit_code"] == 0, "original cc-eval fixture lock validation failed")
    freeze, _ = lock.freeze(argparse.Namespace(
        source_root=str(root), source_path=list(SOURCE_PATHS), binary=str(product),
        config=str(inputs / "config.json"), corpus=str(inputs / "corpus.json"),
        corpus_root=str(corpus), scoring=str(inputs / "scoring.json"), model=str(inputs / "model.json"),
        build_profile={"dev": "debug", "release": "release"}[product_build["receipt"]["build_profile"]],
        feature=[], output=str(output / "candidate")))
    candidate, pin = output / "candidate", freeze["candidate_sha256"]
    write(evidence / "candidate-pin.json", freeze)
    frozen_product, frozen_suite = candidate / "inputs/binary", candidate / "corpus" / SUITE
    lock.verify_candidate(candidate, pin)
    measurement = execute([str(frozen_runner), "run", "--backend", "mcp-stdio", "--binary",
        str(frozen_product), "--suite", str(frozen_suite), "--output", str(evidence / "measurement"),
        "--profile", "smoke"], evidence / "commands/measurement", root)
    lock.verify_candidate(candidate, pin)
    require(file_sha256(frozen_runner) == runner_build["binary_sha256"], "executed runner changed")
    manifest = lock.read_json(evidence / "measurement/manifest.json")
    gate = lock.read_json(evidence / "measurement/gate.json")
    require(measurement["exit_code"] == gate["exit_code"] == 0,
            "actual benchmark failed; original gate and execution output retained")
    require(manifest["adapter"] == "mcp-stdio" and manifest["infrastructure_failure"] is None
            and manifest["suite"] == suite
            and manifest["engine"]["engine_head_observed"] == args.expected_source_commit
            and manifest["engine"]["dirty_observed"] == "",
            "actual MCP result does not bind the clean frozen candidate/suite")
    rows = [json.loads(line) for line in (evidence / "measurement/normalized.jsonl").read_text().splitlines()]
    require(len(rows) == len(questions) * suite["repetitions"] and rows,
            "actual measurement did not execute every planned fixture request")
    original_files = lock.inventory(evidence / "measurement")
    shutil.copytree(evidence / "measurement", evidence / "replay")
    replay = execute([str(frozen_runner), "replay", "--run", str(evidence / "replay")],
                     evidence / "commands/replay", root)
    require(replay["exit_code"] == 0, "original scorer replay failed")
    for name in ("metrics.json", "gate.json", "report.md"):
        require(file_sha256(evidence / "measurement" / name) == file_sha256(evidence / "replay" / name),
                "actual scorer replay differs: " + name)
    require(lock.inventory(evidence / "measurement") == original_files,
            "original measurement was modified during replay")
    controls = drift_controls(candidate, pin, evidence / "drift-controls")
    lock.verify_candidate(candidate, pin)
    require(source_summary(root) == expected_source and environment_record() == before_environment,
            "source or execution environment changed during the candidate run")
    require(toolchain_identity(root, environment) == tools, "toolchain changed during candidate execution")
    require(file_sha256(frozen_runner) == runner_build["binary_sha256"], "runner changed during replay")
    witness = {"schema_version": 1, "status": "executed_frozen_candidate",
        "candidate_sha256": pin, "source": expected_source,
        "product_sha256": file_sha256(frozen_product), "runner_sha256": file_sha256(frozen_runner),
        "actual_build_profile": product_build["receipt"]["build_profile"],
        "actual_features": [], "environment": before_environment,
        "build_receipts": {role: proof["receipt_sha256"] for role, proof in bound_builds.items()},
        "measurement_execution": measurement, "replay_execution": replay,
        "measured_rows": len(rows), "original_gate": gate,
        "original_measurement_files": original_files,
        "drift_controls": controls, "release_certified": False,
        "scope": "actual default local MCP execution and unchanged native scorer replay; input identity only",
        "unexecuted_certifications": ["P8-002 corpus certification", "holdout", "100k", "soak",
                                      "live provider quality", "optimized release/platform certification", "G8"]}
    write(evidence / "execution-witness.json", witness)
    write(evidence / "gate.json", {**gate, "candidate_sha256": pin,
          "original_gate_sha256": file_sha256(evidence / "measurement/gate.json"),
          "execution_witness_sha256": file_sha256(evidence / "execution-witness.json")})
    archived, code = lock.archive(argparse.Namespace(candidate=str(candidate), evidence=str(evidence),
        output_root=str(output / "archives"), run_id="p8-candidate-" + pin[:12], update_latest=False))
    require(code == 0, "candidate witness archive failed")
    write(output / "result.json", {"status": witness["status"], "candidate_sha256": pin,
          "archive": archived, "actual_build_profile": witness["actual_build_profile"],
          "measured_rows": len(rows), "release_certified": False})
    return lock.read_json(output / "result.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    operations = parser.add_subparsers(dest="operation", required=True)
    build = operations.add_parser("build-runner")
    build.add_argument("--output", required=True)
    build.add_argument("--target-dir", help="explicit same-source target; absence/reuse is recorded honestly")
    build.add_argument("--release", action="store_true")
    build.add_argument("--offline", action="store_true")
    run = operations.add_parser("run")
    for name in ("product", "product-build-receipt", "runner", "runner-build-receipt",
                 "expected-source-commit", "output"):
        run.add_argument("--" + name, required=True)
    args = parser.parse_args()
    args.output_owned = False
    try:
        result = build_runner(args) if args.operation == "build-runner" else run_candidate(args)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        failure = {"schema_version": 1, "status": "failed", "error": str(error),
                   "operation": args.operation, "release_certified": False}
        output = Path(args.output)
        if args.output_owned and output.is_dir() and not (output / "failure.json").exists():
            write(output / "failure.json", failure)
        print(json.dumps(failure), file=sys.stderr)
        return 2
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
