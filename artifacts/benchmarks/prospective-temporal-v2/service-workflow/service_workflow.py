#!/usr/bin/env python3
"""Presealed ordinary-Actions orchestration; never authors questions or changes scores."""
import sys
sys.dont_write_bytecode = True
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import time
from types import SimpleNamespace

SOURCE = "2cd04485b0e0b23483f64671d56538bbaa47f442"
PACKAGE_RUN = "37749276827"
PACKAGE_MANIFEST = "1dce263321c34dc60e69132d7e386ffa90c9d0436c1be5759efd7fefc957958b"
PACKAGE_ARCHIVE = "acb16c6c8b34dc513569d4667b0dff721713f791eb7c7752791ad2430ed438f3"
HELPER_MAP = "c9e27b4f838d9c39f00fe7af95953d5fc8c596c25ef6b0a1c4d93f9d30c87cb4"
UPSTREAM = "4e5faa3d7e4008d89e0d8bf1ea87b6d9a061a16d"
BASE = "artifacts/benchmarks/prospective-temporal-v2"
DRIVER = BASE + "/service-workflow/service_workflow.py"
GUIDE = BASE + "/service-workflow/OPERATIONS.md"
RUST_PROBE = "artifacts/checkpoints/p8-temporal-preseal-environment-20261008/shared_rust.py"
WORKFLOWS = (".github/workflows/p8-temporal-prepare-data.yml",
             ".github/workflows/p8-temporal-execute.yml",
             ".github/workflows/p8-temporal-replay.yml")
LOGIC = tuple(sorted((*WORKFLOWS, DRIVER, GUIDE, RUST_PROBE)))
HELPER_NAMES = {"temporal.py", "README.md", "PROTOCOL.md", "protocol.json",
                "locks.template.json", "schedule.row-indices.json"}
CONTROL = BASE + "/workflow-control/contract.json"
DRAFT = BASE + "/draft-data"
DATA = BASE + "/sealed-data"
REQUEST = BASE + "/workflow-control/replay-request.json"

def need(value, reason):
    if not value:
        raise ValueError(reason)

def encoded(value):
    return (json.dumps(value, sort_keys=True, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode()

def sha(content):
    return hashlib.sha256(content).hexdigest()

def read(path):
    return json.loads(Path(path).read_bytes())

def record(path):
    path = Path(path)
    need(path.is_file() and not path.is_symlink(), "regular file required: " + str(path))
    content = path.read_bytes()
    return {"bytes": len(content), "mode": stat.S_IMODE(path.stat().st_mode), "sha256": sha(content)}

def inventory(root):
    return {p.relative_to(root).as_posix(): record(p) for p in sorted(root.rglob("*"))
            if not p.is_dir() or p.is_symlink()}

def write(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as f:
        f.write(encoded(value))
        f.flush()
        os.fsync(f.fileno())

def git(root, *argv):
    return subprocess.check_output(["git", "-C", str(root), *argv], stderr=subprocess.PIPE,
        env=dict(os.environ, GIT_OPTIONAL_LOCKS="0", GIT_NO_LAZY_FETCH="1", GIT_TERMINAL_PROMPT="0"))

def clean(root, head=None):
    actual = git(root, "rev-parse", "HEAD").decode().strip()
    need(head is None or actual == head, "checkout HEAD mismatch")
    need(not git(root, "status", "--porcelain", "--untracked-files=all").strip(), "dirty checkout")
    return actual

def check_helpers(utility):
    directory = utility / BASE / "implementation"
    observed = inventory(directory)
    need(set(observed) == HELPER_NAMES and sha(encoded(observed)) == HELPER_MAP,
         "fixed six-file helper closure mismatch")
    return directory

def check_logic(utility, formal):
    current = clean(utility)
    contract = read(utility / CONTROL)
    need(type(contract["schema_version"]) is int and contract["schema_version"] == 1
         and set(contract["files"]) == set(LOGIC),
         "complete separately sealed workflow logic inventory")
    baseline = contract["frozen_logic_commit"]
    need(re.fullmatch("[0-9a-f]{40}", baseline), "full pre-author workflow commit required")
    need(subprocess.run(["git", "-C", str(utility), "merge-base", "--is-ancestor", baseline, current],
                        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE).returncode == 0,
         "workflow commit is not an ancestor")
    for path in LOGIC:
        need(record(utility / path) == contract["files"][path]
             and sha(git(utility, "show", baseline + ":" + path)) == contract["files"][path]["sha256"],
             "workflow logic changed after its seal: " + path)
    previous = baseline
    for commit in git(utility, "rev-list", "--reverse", baseline + ".." + current).decode().splitlines():
        need(git(utility, "show", "-s", "--format=%P", commit).decode().strip() == previous,
             "only linear data-only descendants may carry sealed workflow")
        for raw_name in git(utility, "diff-tree", "--no-commit-id", "--name-only", "-r", "-z", commit).split(b"\0"):
            if not raw_name:
                continue
            name = raw_name.decode()
            allowed = (name.startswith(DRAFT + "/") or name.startswith(DATA + "/")
                       or name.startswith(BASE + "/workflow-control/"))
            need(allowed and Path(name).suffix in (".json", ".jsonl", ".md", ".txt", ".log"),
                 "post-seal change is not a declared data-only file: " + name)
            entry = git(utility, "ls-tree", commit, "--", name).decode().strip()
            need(not entry or entry.startswith("100644 blob "), "post-seal data cannot be executable/symlink")
        previous = commit
    need(previous == current, "data-only ancestry coverage")
    if formal:
        locked = read(utility / DATA / "locks.json")
        need(locked["measurement"]["exact_commands"]["workflow_contract_sha256"] ==
             record(utility / CONTROL)["sha256"], "data seal does not bind pre-author workflow contract")
    return {"utility_commit": current, "contract": contract, "contract_record": record(utility / CONTROL),
            "helper_map_sha256": HELPER_MAP}

def paths(workspace):
    return {"utility": workspace / "utility", "source": workspace / "candidate",
            "upstream": workspace / "upstream", "package": workspace / "package-input/candidate-package"}

def shared(p, helper, output):
    need(record(p["package"] / "manifest.json")["sha256"] == PACKAGE_MANIFEST
         and record(p["package"] / "candidate-tools.zip")["sha256"] == PACKAGE_ARCHIVE,
         "actual joint release package bytes")
    return [sys.executable, "-I", "-S", str(helper / "temporal.py")], [
        "--source-root", str(p["source"]), "--expected-source", SOURCE,
        "--package-manifest", str(p["package"] / "manifest.json"),
        "--package-manifest-sha256", PACKAGE_MANIFEST,
        "--archive", str(p["package"] / "candidate-tools.zip"),
        "--helper-map-sha256", HELPER_MAP, "--rg", "/usr/bin/rg", "--output", str(output)]

def invoke(argv, output):
    output.mkdir(parents=True, exist_ok=False)
    started = time.time()
    print(json.dumps({"event": "command_started", "argv": [str(x) for x in argv],
                      "observed_unix": started}), flush=True)
    with (output / "stdout.log").open("xb") as stdout, (output / "stderr.log").open("xb") as stderr:
        result = subprocess.run([str(x) for x in argv], stdout=stdout, stderr=stderr)
        stdout.flush()
        stderr.flush()
        os.fsync(stdout.fileno())
        os.fsync(stderr.fileno())
    receipt = {"argv": [str(x) for x in argv], "process_exit_code": result.returncode,
               "started_unix": started, "completed_unix": time.time(),
               "stdout": record(output / "stdout.log"), "stderr": record(output / "stderr.log")}
    write(output / "command.json", receipt)
    print(json.dumps({"event": "command_completed", "process_exit_code": result.returncode,
                      "command_receipt": str(output / "command.json")}), flush=True)
    return result.returncode

def import_operator(helper):
    spec = importlib.util.spec_from_file_location("locked_temporal_prepare_only", helper / "temporal.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module

def prepare_data(p, helper, output):
    operator = import_operator(helper)
    draft = p["utility"] / DRAFT
    need(not (draft / "locks.json").exists(), "draft must not falsely contain a completed data seal")
    original = inventory(draft)
    prepared = output / "prepared-data"
    shutil.copytree(draft, prepared)
    bundle = read(prepared / "data.json")
    suite = prepared / operator.relative(bundle["suite"])
    before_suite = read(suite)
    # Preserve the same actual source-root bytes in all three workflows.
    # Authors use the observed pre-author source-lock.root; no after-body path
    # rewrite is performed here or during execution.
    need(before_suite["source"]["root"] == str(p["upstream"].resolve()),
         "draft source.root must equal the actual pre-author upstream checkout path")
    need((suite.parent / before_suite["queries"]).resolve().is_relative_to(prepared.resolve()),
         "query input must remain inside owned data copy")
    args = SimpleNamespace(source_root=p["source"], expected_source=SOURCE,
        package_manifest=p["package"] / "manifest.json", package_manifest_sha256=PACKAGE_MANIFEST,
        archive=p["package"] / "candidate-tools.zip", helper_map_sha256=HELPER_MAP, rg=Path("/usr/bin/rg"))
    setup_out = output / "original-package-validation"
    setup_out.mkdir()
    root, helper_dir, closure, env, package, candidate, binaries, child_env, binary_before = operator.setup(args, setup_out)
    checked_before = operator.validate_data(prepared, False)
    original_upstream = operator.generic_source_admission(p["upstream"], UPSTREAM)
    for verb in ("freeze", "validate"):
        result = operator.command([binaries["runner"], verb, "--suite", suite],
            output / "commands" / ("original-" + verb), root, child_env, 600)
        operator.zero(result)
    after_suite = read(suite)
    comparable = json.loads(json.dumps(after_suite))
    comparable["source"]["digest"] = before_suite["source"]["digest"]
    comparable["queries_digest"] = before_suite["queries_digest"]
    need(comparable == before_suite, "original freeze changed more than its two content digests")
    changed = [name for name in original if record(prepared / name) != original[name]]
    need(set(inventory(prepared)) == set(original)
         and set(changed).issubset({suite.relative_to(prepared).as_posix()}),
         "preparation changed non-suite data")
    checked = operator.validate_data(prepared, False)
    need(inventory(draft) == original and operator.generic_source_admission(p["upstream"], UPSTREAM) == original_upstream,
         "original draft/upstream changed")
    need(operator.candidate_snapshot(root, SOURCE) == candidate
         and operator.helper_snapshot(helper_dir) == closure
         and operator.binary_snapshot(binaries) == binary_before, "fixed tool/source bytes changed")
    write(output / "result.json", {"status": "original_native_freeze_validate_recorded_before_data_seal",
        "scheduled_product_queries": 0, "original_draft_inventory": original,
        "prepared_data_inventory": inventory(prepared), "actual_native_suite": after_suite,
        "actual_suite_record": record(suite), "derived_data_commitments": operator.data_commitments(checked),
        "original_commands": {"freeze": "commands/original-freeze", "validate": "commands/original-validate"},
        "candidate": candidate, "helper_closure": closure,
        "preparation_python_dependency_closure": operator.python_closure(),
        "measurement_dependency_fingerprint": "not_claimed; this preparation process imports the orchestration module",
        "data_sealed": False, "formal_execution_authorized": False,
        "next": "Independent byte/source-gold/service review, then data-only sealed-data copy and non-null locks."})
    return 0

def execute(p, helper, output):
    need(os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "formal rerun is not a new authorized attempt")
    need(os.environ.get("GITHUB_REF") == "refs/heads/work/p8-temporal-execute-996727-20261008",
         "formal dispatch branch mismatch")
    base, args = shared(p, helper, output / "execution")
    identity = {key: os.environ.get(key) for key in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_JOB", "GITHUB_SHA")}
    identity.update(permission_branch="refs/heads/work/ptv2-permit-" + os.environ["GITHUB_RUN_ID"],
                    wait_seconds=600, scheduled_product_queries_before_permission=0)
    write(output / "service-start.json", identity)
    print(json.dumps({"event": "service_identity_before_authorization", **identity}), flush=True)
    waiting = output / "authorization"
    code = invoke(base + ["wait-authorization", "--output", str(waiting)], output / "wait-command")
    if code != 0:
        return code
    binding = read(waiting / "binding.json")
    # The child remains a separate -I -S Python process, so this workflow
    # driver never enters temporal.py's strict measurement module fingerprint.
    code = invoke(base + ["execute", *args, "--data", str(p["utility"] / DATA),
        "--authorization", str(waiting / "files/authorization.json"),
        "--authorization-sha256", binding["authorization_sha256"],
        "--authorization-binding", str(waiting / "binding.json")], output / "execute-command")
    write(output / "result.json", {"status": "formal_process_recorded_not_task_acceptance",
        "actual_helper_process_exit_code": code,
        "same_seal_external_service_history_final_review": "required",
        "original_execution": "execution", "permission": "authorization",
        "quality_gate": "retain original0/1/2/3 and complete raw; no conversion of1 to pass"})
    return code

def replay(p, helper, output, request_path, original):
    request = read(request_path)
    expected_keys = {"schema_version", "execution_run_id", "execution_run_attempt", "execution_utility_commit",
                     "execution_artifact_name", "raw_inventory_sha256", "data_inventory_sha256",
                     "execution_json_record", "execution_result_record", "independent_review_record"}
    need(set(request) == expected_keys and type(request["schema_version"]) is int
         and request["schema_version"] == 1, "raw replay request schema")
    review = request["independent_review_record"]
    need(set(review) == {"path", "record"} and isinstance(review["path"], str)
         and not Path(review["path"]).is_absolute()
         and all(part not in ("", ".", "..") for part in review["path"].split("/"))
         and "\\" not in review["path"], "relative pinned independent review file")
    need(record(request_path.parent / review["path"]) == review["record"],
         "independent raw input review file bytes/mode")
    # Byte binding is mechanical. Review authorship, original service provenance
    # and its substantive conclusion still require independent manual review.
    need(request["execution_run_attempt"] == "1"
         and re.fullmatch("[0-9]+", request["execution_run_id"])
         and request["execution_artifact_name"] == "p8-prospective-temporal-execution",
         "actual original formal artifact identity")
    need(clean(p["utility"]) == request["execution_utility_commit"], "replay must use actual original data utility")
    for key in ("raw_inventory_sha256", "data_inventory_sha256"):
        need(re.fullmatch("[0-9a-f]{64}", request[key]), "independently observed replay SHA required")
    need(record(original / "execution/execution.json") == request["execution_json_record"],
         "actual original execution receipt bytes/mode")
    observed = read(original / "execution/execution.json")
    need(observed["synthetic_control_only"] is False
         and observed["actions"]["GITHUB_RUN_ID"] == request["execution_run_id"]
         and observed["actions"]["GITHUB_RUN_ATTEMPT"] == "1"
         and observed["actions"]["GITHUB_SHA"] == request["execution_utility_commit"],
         "original execution service identity")
    need(sha(encoded(inventory(p["utility"] / DATA))) == request["data_inventory_sha256"],
         "actual original sealed data inventory")
    need(record(original / "execution/result.json") == request["execution_result_record"],
         "actual original result record")
    raw_map = read(original / "execution/result.json")["execution_inventory"]
    need(sha(encoded(raw_map)) == request["raw_inventory_sha256"],
         "independent expected raw map digest")
    downloaded = original / "execution/runs"
    observed_map = inventory(downloaded)
    need(set(observed_map) == set(raw_map), "complete original raw input inventory")
    for name, expected in raw_map.items():
        need(Path(name).as_posix() == name and not Path(name).is_absolute()
             and all(p not in ("", ".", "..") for p in name.split("/")), "relative raw path")
        need(all(observed_map[name][key] == expected[key] for key in ("bytes", "sha256"))
             and expected["mode"] in (0o644, 0o755), "original raw bytes or unsupported original mode")
    # Artifact transport may replace Unix modes. Restore only a new owned copy
    # from the independently pinned original map; preserve the downloaded bytes.
    restored = output / "restored-original-runs"
    shutil.copytree(downloaded, restored)
    changes = []
    for name, expected in raw_map.items():
        target = restored / name
        before_mode = stat.S_IMODE(target.stat().st_mode)
        target.chmod(expected["mode"])
        if before_mode != expected["mode"]:
            changes.append({"path": name, "downloaded_mode": before_mode, "restored_original_mode": expected["mode"]})
    need(inventory(restored) == raw_map and inventory(downloaded) == observed_map,
         "exact restored copy or original artifact bytes changed")
    write(output / "mode-restoration.json", {"original_map_sha256": request["raw_inventory_sha256"],
        "changes": changes, "original_download_unchanged": True,
        "scope": "Unix modes restored on an owned copy; not a claim that GitHub ZIP preserved them"})
    base, args = shared(p, helper, output / "raw-only-replay")
    code = invoke(base + ["replay", *args, "--data", str(p["utility"] / DATA),
        "--runs", str(restored),
        "--raw-inventory-sha256", request["raw_inventory_sha256"]], output / "replay-command")
    write(output / "result.json", {"status": "raw_only_process_recorded_not_task_acceptance",
        "actual_helper_process_exit_code": code, "new_product_queries": 0,
        "independent_request": request, "original_artifact_changed": False,
        "scope": "Original scorer replay and sealed-byte checks; not independent raw re-normalization."})
    return code

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("prepare-data", "execute", "replay"))
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--request", type=Path)
    parser.add_argument("--original", type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    need(not output.exists(), "new output required")
    p = paths(args.workspace.resolve())
    for input_path in (*p.values(), Path(__file__).resolve(), args.request, args.original):
        if input_path is not None:
            target = input_path.resolve()
            need(output != target and not output.is_relative_to(target) and not target.is_relative_to(output),
                 "output overlaps immutable input")
    output.mkdir(parents=True)
    try:
        if args.mode == "replay":
            need(args.request is not None and args.original is not None, "raw replay inputs")
            check_logic(args.workspace.resolve() / "controller", False)
            need(args.request.resolve() == (args.workspace.resolve() / "controller" / REQUEST).resolve(),
                 "raw replay request must be the declared data-only controller file")
        observed = check_logic(p["utility"], args.mode in ("execute", "replay"))
        helper = check_helpers(p["utility"])
        clean(p["source"], SOURCE)
        clean(p["upstream"], UPSTREAM)
        shared(p, helper, output / "unused-path")
        need(os.environ.get("RUSTUP_HOME") == "/home/runner/.rustup"
             and os.environ.get("CARGO_HOME") == "/home/runner/.cargo"
             and os.environ.get("RUSTUP_TOOLCHAIN") == "1.95.0", "fixed observed Rust routing")
        rust_probe = read(Path(os.environ["RUNNER_TEMP"]) / "ptv2-shared-rust-environment.json")
        need(rust_probe["status"] == "same_fixed_parent_and_child_rust_paths_verified"
             and rust_probe["new_product_queries"] == 0, "actual shared Rust environment probe")
        write(output / "shared-rust-environment.json", rust_probe)
        write(output / "workflow-inputs.json", {**observed, "mode": args.mode,
            "actual_driver": record(Path(__file__)), "package_run": PACKAGE_RUN,
            "package_manifest_sha256": PACKAGE_MANIFEST, "package_archive_sha256": PACKAGE_ARCHIVE,
            "inherited_rustup_home": os.environ.get("RUSTUP_HOME"),
            "inherited_cargo_home": os.environ.get("CARGO_HOME"),
            "rustup_toolchain": os.environ.get("RUSTUP_TOOLCHAIN")})
        if args.mode == "prepare-data":
            return prepare_data(p, helper, output)
        if args.mode == "execute":
            return execute(p, helper, output)
        need(args.request is not None and args.original is not None, "raw replay inputs")
        return replay(p, helper, output, args.request, args.original)
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        write(output / "workflow-failure.json", {"status": "invalid_or_incomplete", "error_type": type(error).__name__,
            "error": str(error), "task_acceptance": "not_decided",
            "automatic_retry": False, "dispatch_unknown_counts_as_consumed": True})
        print(json.dumps({"status": "invalid_or_incomplete", "error": str(error)}), file=sys.stderr)
        return 2

if __name__ == "__main__":
    status = main()
    raise SystemExit(status if status >= 0 else 128 - status)
