#!/usr/bin/env python3
"""Locked local cc-eval compatibility runs and comparisons; no new scorer.

The external targets remain cc-switch/Flask at the roadmap's exact commits.
Public development controls exercise this harness, never satisfy those targets.
All input, binary, scorer, profile, budget and glob identities must remain bound.
"""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import posixpath
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

from p8_corpus_audit import sha, parse_json, git
from p8_release_evidence import (Invalid, canonical, digest, disjoint, file_record,
                                 name, path, read_bytes, read_json, require, tree_files, write_json)


ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "cc-switch": ("farion1231/cc-switch", "40cac1a68edf8c9e7b3a89125cf40bb93a348404"),
    "flask": ("pallets/flask", "22d924701a6ae2e4cd01e9a15bbaf3946094af65"),
}
SCORER_FILES = {"crates/cc-eval/src/benchmark/" + n for n in
                ("schema.rs", "metrics.rs", "manifest.rs", "normalizer.rs", "validation.rs", "comparison.rs",
                 "statistics.rs", "report.rs", "gate.rs", "importer_oce.rs")}
PROFILES = {"compat": "oce-compat-v1", "native": "codecortex-native-v1"}
GLOB_POLICY = "case-sensitive-slash-normalized-no-overlap-v1"
MAX_FILE = 16 * 1024 * 1024
MAX_RAW = 128 * 1024 * 1024
MAX_LOG = 2 * 1024 * 1024


def current_platform():
    return {"os": platform.system(), "architecture": platform.machine(), "glob_policy": GLOB_POLICY}


def pin_file(filename, expected, *, executable=False):
    require(isinstance(expected, str) and re.fullmatch(r"[0-9a-f]{64}", expected), "invalid SHA256 pin")
    filename = path(filename)
    record = file_record(filename, limit=512 * 1024 * 1024 if executable else MAX_FILE)
    require(record["sha256"] == expected, "file/binary/scorer lock drift")
    require(not executable or os.access(filename, os.X_OK), "binary is not executable")
    return filename, record


def jsonl(filename):
    rows = [parse_json(line) for line in read_bytes(filename, limit=MAX_FILE).splitlines() if line.strip()]
    require(rows and all(isinstance(row, dict) for row in rows), "empty or invalid JSONL")
    return rows


def joined_input(root, base, relative):
    # Existing suite source roots use ../. Normalize the string without following
    # a symlink, then require the result to remain inside this declared input root.
    require(isinstance(relative, str) and not relative.startswith("/") and "\\" not in relative,
            "absolute or nonportable suite pointer")
    normalized = posixpath.normpath(posixpath.join(base, relative))
    if normalized != ".":
        name(normalized)
    result = path(root if normalized == "." else root / normalized)
    require(result.is_relative_to(root), "suite pointer escaped input root")
    return result


def git_checkout_lock(root, expected):
    require(git(root, "rev-parse", "--show-toplevel").decode().strip() == str(root),
            "external source must be the exact Git checkout root")
    require(git(root, "rev-parse", "HEAD").decode().strip() == expected, "external Git HEAD drift")
    require(not git(root, "status", "--porcelain", "--untracked-files=all").strip(), "dirty external source checkout")
    require(not git(root, "submodule", "status", "--recursive").strip(), "expanded submodule source lock required")


def inspect_lock(lock_file):
    lock_file = path(lock_file)
    lock = read_json(lock_file)
    require(set(lock) == {"schema_version", "dataset", "repository", "commit", "source_mode", "input_root",
                          "evaluator", "backend", "scorer", "platform", "suites", "comparison_policy", "timeout_seconds"}
            and lock["schema_version"] == 1, "invalid compatibility lock schema")
    dataset = lock["dataset"]
    require(dataset in TARGETS or dataset == "public-dev-control", "unknown dataset scope")
    require(isinstance(lock["commit"], str) and re.fullmatch(r"[0-9a-f]{40}", lock["commit"]), "full source commit required")
    if dataset in TARGETS:
        require((lock["repository"], lock["commit"]) == TARGETS[dataset], "external target commit mismatch")
        require(lock["source_mode"] == "git_checkout", "external certification requires a Git source lock")
    else:
        require(lock["source_mode"] == "snapshot_control", "public dev control must be labelled snapshot")
    require(lock["platform"] == current_platform(), "platform or glob-policy mismatch")
    require(type(lock["timeout_seconds"]) is int and 1 <= lock["timeout_seconds"] <= 600, "process timeout bounds")
    root = path(lock["input_root"])
    evaluator = lock["evaluator"]
    require(set(evaluator) == {"path", "sha256", "source_sha", "build_receipt", "build_receipt_sha256"}, "evaluator identity fields")
    require(re.fullmatch(r"[0-9a-f]{40}", evaluator["source_sha"]) is not None, "evaluator source SHA required")
    binary, _ = pin_file(evaluator["path"], evaluator["sha256"], executable=True)
    build_receipt, _ = pin_file(evaluator["build_receipt"], evaluator["build_receipt_sha256"])
    read_json(build_receipt)  # Bound provenance, not a cryptographic build attestation.
    backend = lock["backend"]
    require(set(backend) == {"kind", "path", "sha256"} and backend["kind"] in ("rg", "mcp-stdio"), "local backend required")
    backend_binary, _ = pin_file(backend["path"], backend["sha256"], executable=True)
    if backend["kind"] == "rg":
        require(backend_binary.name == "rg", "rg adapter invokes the pinned rg executable by name")
    scorer = lock["scorer"]
    require(set(scorer) == {"root", "files"} and set(scorer["files"]) == SCORER_FILES, "complete scorer file lock required")
    scorer_root = path(scorer["root"])
    for relative, expected in scorer["files"].items():
        pin_file(scorer_root / name(relative), expected)
    policy, _ = pin_file(lock["comparison_policy"]["path"], lock["comparison_policy"]["sha256"])
    policy_json = read_json(policy)
    require(policy_json.get("schema_version") == 1, "comparison policy schema")
    require(isinstance(lock["suites"], list) and 1 <= len(lock["suites"]) <= 2, "compat plus optional native suites required")
    suites, signatures, seen = [], {}, set()
    input_paths = [lock_file, root, binary, backend_binary, build_receipt, scorer_root, policy]
    for entry in lock["suites"]:
        require(set(entry) == {"profile", "path", "sha256", "query_sha256", "source_files"}, "suite lock fields")
        profile = entry["profile"]
        require(profile in PROFILES and profile not in seen, "unknown or repeated profile")
        seen.add(profile)
        suite_path, _ = pin_file(root / name(entry["path"]), entry["sha256"])
        suite = read_json(suite_path)
        require(suite["scoring"] == PROFILES[profile], "mixed scoring profile")
        require(suite["engine_config"] == {"auto_index": {"enabled": False}}, "only the explicit local default config is admitted")
        base = str(suite_path.parent.relative_to(root))
        query_path = joined_input(root, base, suite["queries"])
        pin_file(query_path, entry["query_sha256"])
        rows = jsonl(query_path)
        require(all(row.get("split") == "dev" for row in rows), "only public DEV input accepted; no body echoed")
        ids = [row["id"] for row in rows]
        require(len(ids) == len(set(ids)), "duplicate query ID")
        source_root = joined_input(root, base, suite["source"]["root"])
        if dataset in TARGETS:
            require(suite["source"]["commit"] == lock["commit"], "missing external Git commit lock")
            git_checkout_lock(source_root, lock["commit"])
        else:
            require(suite["source"]["commit"] is None, "control must retain snapshot-only provenance")
        files = suite["source"]["files"]
        require(files and len(files) == len(set(files)) and set(files) == set(entry["source_files"]), "source input inventory drift")
        source_records = {}
        for relative in files:
            source_path, record = pin_file(source_root / name(relative), entry["source_files"][relative])
            require(source_path != query_path, "gold cannot be indexed source")
            source_records[relative] = record
        source_signature = {"sha256": digest(source_records), "files": len(files)}
        signatures[profile] = {"suite_sha256": entry["sha256"], "query_sha256": entry["query_sha256"],
                               "source": source_signature, "scoring": PROFILES[profile],
                               "configuration": suite["engine_config"],
                               "budget": {k: suite[k] for k in ("top_k", "timeout_ms", "warmup", "repetitions", "seed")}}
        suites.append({"profile": profile, "path": suite_path, "suite": suite, "query_rows": len(rows)})
    require("compat" in seen, "compat suite is required")
    if "native" in seen:
        require(signatures["native"]["source"] == signatures["compat"]["source"], "native/compat source input mismatch")
        require(signatures["native"]["budget"] == signatures["compat"]["budget"], "native/compat budget mismatch")
    identity = {"dataset": dataset, "repository": lock["repository"], "commit": lock["commit"],
                "source_mode": lock["source_mode"], "platform": lock["platform"], "suites": signatures,
                "measurement_profile": "smoke",
                "evaluator_sha256": evaluator["sha256"], "evaluator_source_sha": evaluator["source_sha"],
                "evaluator_build_receipt_sha256": evaluator["build_receipt_sha256"],
                "backend": {"kind": backend["kind"], "sha256": backend["sha256"]},
                "scorer_sha256": digest(scorer["files"]), "comparison_policy_sha256": lock["comparison_policy"]["sha256"]}
    return {"lock": lock, "lock_path": lock_file, "root": root, "binary": binary,
            "backend_binary": backend_binary, "policy": policy, "suites": suites,
            "identity": identity, "input_paths": input_paths}


def command(argv, log_path, timeout, *, env=None):
    started = datetime.now(timezone.utc).isoformat()
    t0 = time.monotonic()
    timed_out = threading.Event()
    with subprocess.Popen([str(v) for v in argv], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, env=env, start_new_session=True) as child:
        def terminate():
            timed_out.set()
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        timer = threading.Timer(timeout, terminate)
        timer.daemon = True
        timer.start()
        try:
            raw = child.stdout.read(MAX_LOG + 1)
            truncated = len(raw) > MAX_LOG
            if truncated:
                terminate()
            exit_code = child.wait()
        finally:
            timer.cancel()
    with path(log_path, exists=False).open("xb") as stream:
        stream.write(raw[:MAX_LOG])
    return {"argv": [str(v) for v in argv], "started_at_utc": started,
            "duration_seconds": round(time.monotonic() - t0, 6), "process_exit_code": exit_code,
            "exit_code": 2 if timed_out.is_set() or truncated else exit_code,
            "timed_out_or_log_budget": timed_out.is_set(), "log_truncated": truncated,
            "log_sha256": sha(raw[:MAX_LOG])}


def raw_inventory(root):
    files, total = {}, 0
    for relative in tree_files(root):
        record = file_record(root / relative, limit=MAX_FILE)
        total += record["bytes"]
        require(total <= MAX_RAW, "raw artifact byte budget")
        files[relative] = record
    return files


def copy_raw(root, destination):
    before = raw_inventory(root)
    destination.mkdir()
    for relative, record in before.items():
        copied = file_record(root / relative, destination / relative, limit=MAX_FILE)
        require(copied == record, "raw changed during replay-copy")
    require(raw_inventory(root) == before, "raw input drift while copying")
    return before


def check_run(run, suite, process_exit):
    required = {"manifest.json", "queries.jsonl", "normalized.jsonl", "metrics.json", "gate.json", "report.md"}
    names = set(tree_files(run))
    require(required.issubset(names), "successful process omitted required raw artifacts")
    manifest = read_json(run / "manifest.json")
    gate = read_json(run / "gate.json")
    metrics = read_json(run / "metrics.json")
    require(manifest["suite"] == suite["suite"], "run manifest changed locked suite")
    require(gate["exit_code"] == process_exit and type(process_exit) is int and process_exit in (0, 1, 2, 3),
            "process/gate exit mismatch")
    rows = jsonl(run / "normalized.jsonl")
    queries = jsonl(run / "queries.jsonl")
    require(len(queries) == suite["query_rows"] and all(q["split"] == "dev" for q in queries), "query denominator drift")
    require(len(rows) == len(queries) * suite["suite"]["repetitions"], "missing measured rows")
    require(metrics["queries"] == len(queries) and metrics["measured_rows"] == len(rows), "metrics denominator drift")
    require(len({(r["case_id"], r["repetition"]) for r in rows}) == len(rows), "duplicate measured rows")
    require(all(r["raw_path"] in names for r in rows), "missing raw response")
    require(manifest["input"]["query_digest"] == suite["suite"]["queries_digest"]
            and manifest["input"]["source_digest"] == suite["suite"]["source"]["digest"], "run source/query lock mismatch")
    return {"gate": gate, "metrics": {k: metrics[k] for k in ("queries", "measured_rows", "mean_top1", "mean_ndcg10")},
            "adapter": manifest["adapter"], "adapter_version": manifest["adapter_version"]}


def run_locked(lock_file, output, *, validate_only=False):
    checked = inspect_lock(lock_file)
    output = path(output, exists=False)
    disjoint(output, checked["input_paths"])
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir()
    lock = checked["lock"]
    write_json(output / "lock.json", lock)
    file_record(checked["policy"], output / "comparison-policy.json")
    result = {"schema_version": 1, "scope": "local_compat_engineering_only", "identity": checked["identity"],
              "mode": "validate_only" if validate_only else "run", "profiles": {}, "commands": [],
              "release_certified": False, "external_targets_completed": [],
              "source_binding": "pinned_declared_source_and_build_receipt_not_cryptographic_attestation",
              "script_sha256": file_record(Path(__file__), limit=MAX_FILE)["sha256"]}
    code = 0
    env = {**os.environ, "CODECORTEX_BENCH_PROCESS_PROBE": "0", "GIT_TERMINAL_PROMPT": "0", "GIT_NO_LAZY_FETCH": "1"}
    if lock["backend"]["kind"] == "rg":
        env["PATH"] = str(checked["backend_binary"].parent) + os.pathsep + env.get("PATH", "")
        require(Path(shutil.which("rg", path=env["PATH"])) == checked["backend_binary"], "rg resolution differs from binary pin")
    try:
        for suite in checked["suites"]:
            profile = suite["profile"]
            validate = command([checked["binary"], "validate", "--suite", suite["path"]], output / (profile + "-validate.log"), lock["timeout_seconds"], env=env)
            result["commands"].append(validate)
            require(validate["exit_code"] == 0, "cc-eval input validation failed")
            if validate_only:
                result["profiles"][profile] = {"status": "input_validation_only", "queries": suite["query_rows"], "ranking": "not_run"}
                continue
            run = output / profile
            argv = [checked["binary"], "run", "--backend", lock["backend"]["kind"], "--suite", suite["path"], "--output", run, "--profile", "smoke"]
            if lock["backend"]["kind"] == "mcp-stdio":
                argv.extend(["--binary", checked["backend_binary"]])
            executed = command(argv, output / (profile + "-run.log"), lock["timeout_seconds"], env=env)
            result["commands"].append(executed)
            measured = check_run(run, suite, executed["exit_code"])
            before = raw_inventory(run)
            with tempfile.TemporaryDirectory(prefix="p8-compat-replay-") as temporary:
                replay_dir = Path(temporary) / "run"
                copy_raw(run, replay_dir)
                replayed = command([checked["binary"], "replay", "--run", replay_dir], output / (profile + "-replay.log"), lock["timeout_seconds"], env=env)
                result["commands"].append(replayed)
                require(replayed["exit_code"] == executed["exit_code"], "replay status drift")
                require(read_json(replay_dir / "metrics.json") == read_json(run / "metrics.json"), "raw replay metrics drift")
                require(read_json(replay_dir / "gate.json") == measured["gate"], "raw replay gate drift")
            require(raw_inventory(run) == before, "original raw run changed during replay")
            result["profiles"][profile] = {**measured, "raw_files": before, "replay_exit_code": replayed["exit_code"]}
            code = max(code, executed["exit_code"])
        require(inspect_lock(lock_file)["identity"] == checked["identity"], "input identity drift during run")
        result["status"] = "validated_inputs_only" if validate_only else ("baseline_recorded_not_quality_certified" if code == 0 else "gate_not_passed")
    except (Invalid, OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
        code = 2
        result.update(status="invalid_measurement", error_type=type(exc).__name__, error="input, process, or raw verification failed; retained artifacts are not a pass")
    result["exit_code"] = code
    result["receipt_sha256"] = digest(result)
    write_json(output / "receipt.json", result)
    return result, code


def load_run(root):
    root = path(root)
    receipt = read_json(root / "receipt.json")
    unsigned = dict(receipt)
    signature = unsigned.pop("receipt_sha256", None)
    require(signature == digest(unsigned), "run receipt checksum drift")
    require(receipt["scope"] == "local_compat_engineering_only" and receipt["mode"] == "run"
            and receipt["status"] == "baseline_recorded_not_quality_certified" and receipt["exit_code"] == 0,
            "incomplete or failed run cannot enter comparison")
    require(receipt["release_certified"] is False, "unsupported certification claim")
    require(receipt["profiles"] and set(receipt["profiles"]) == set(receipt["identity"]["suites"]), "profile receipt incomplete")
    for profile, record in receipt["profiles"].items():
        require(profile in PROFILES and raw_inventory(root / profile) == record["raw_files"], "raw artifact checksum drift")
    pin_file(root / "comparison-policy.json", receipt["identity"]["comparison_policy_sha256"])
    return receipt


def compare_runs(left, right, evaluator, output):
    left, right = path(left), path(right)
    require(left != right, "two distinct retained runs required")
    output = path(output, exists=False)
    disjoint(output, [left, right, path(evaluator)])
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir()
    result = {"schema_version": 1, "scope": "same_locked_input_per_profile_comparison", "profiles": {},
              "release_certified": False, "original_raw_mutated": False, "external_targets_completed": []}
    code = 2
    try:
        a, b = load_run(left), load_run(right)
        differences = [key for key in sorted(set(a["identity"]) | set(b["identity"])) if a["identity"].get(key) != b["identity"].get(key)]
        require(not differences, "non-comparable locked inputs: " + ",".join(differences))
        require(a["identity"]["platform"] == current_platform(), "replay platform/glob mismatch")
        binary, _ = pin_file(evaluator, a["identity"]["evaluator_sha256"], executable=True)
        result["identity"] = a["identity"]
        code = 0
        with tempfile.TemporaryDirectory(prefix="p8-compat-compare-") as temporary:
            temp = Path(temporary)
            for profile in sorted(a["profiles"]):
                baseline, candidate = temp / (profile + "-baseline"), temp / (profile + "-candidate")
                before_a = copy_raw(left / profile, baseline)
                before_b = copy_raw(right / profile, candidate)
                comparison = output / (profile + "-comparison.json")
                execution = command([binary, "compare", "--baseline", baseline, "--candidate", candidate,
                                     "--gate", left / "comparison-policy.json", "--output", comparison],
                                    output / (profile + "-compare.log"), 120)
                require(comparison.is_file(), "zero-exit comparator omitted comparison artifact")
                compared = read_json(comparison)
                require(type(compared["exit_code"]) is int and compared["exit_code"] in (0, 1, 2, 3)
                        and compared["exit_code"] == execution["exit_code"], "comparison exit-code mismatch")
                require(compared["status"] in ("passed", "failed", "inconclusive", "invalid_measurement")
                        and (compared["status"] == "passed") == (compared["exit_code"] == 0), "false-green comparison")
                require(raw_inventory(left / profile) == before_a and raw_inventory(right / profile) == before_b, "original raw comparison input mutated")
                result["profiles"][profile] = {"comparison": compared, "execution": execution}
                code = max(code, execution["exit_code"])
        pin_file(evaluator, a["identity"]["evaluator_sha256"], executable=True)
        result["status"] = "comparison_passed_local_scope" if code == 0 else "comparison_not_passed"
    except (Invalid, OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
        result.update(status="non_comparable_or_invalid", error_type=type(exc).__name__, reason=str(exc) if isinstance(exc, Invalid) else "comparison input or execution failure")
        code = 2
    result["exit_code"] = code
    write_json(output / "comparison-receipt.json", result)
    return result, code


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--lock", required=True, type=Path)
    run.add_argument("--output", required=True, type=Path)
    run.add_argument("--validate-only", action="store_true")
    compare = commands.add_parser("compare")
    compare.add_argument("--left", required=True, type=Path)
    compare.add_argument("--right", required=True, type=Path)
    compare.add_argument("--evaluator", required=True, type=Path)
    compare.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        result, code = (run_locked(args.lock, args.output, validate_only=args.validate_only) if args.command == "run"
                        else compare_runs(args.left, args.right, args.evaluator, args.output))
        print(json.dumps({"status": result["status"], "exit_code": code, "release_certified": False}))
        return code
    except (Invalid, OSError, ValueError, TypeError, KeyError) as exc:
        print(json.dumps({"status": "invalid_inputs", "exit_code": 2, "error_type": type(exc).__name__, "release_certified": False}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
