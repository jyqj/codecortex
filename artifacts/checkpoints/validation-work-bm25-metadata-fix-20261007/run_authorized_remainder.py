#!/usr/bin/env python3
"""Run the explicitly authorized remaining default and five semantic-http gates."""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
EXPECTED_HEAD = "65eb87d70bd7bfd10d251b5d1cbb25850958196b"


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def sources():
    names = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z",
         "--", "crates", "Cargo.toml", "Cargo.lock"], cwd=ROOT,
    ).decode().split("\0")
    return {p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest()
            for p in sorted(set(names) - {""})}


def main():
    assert git("rev-parse", "HEAD") == EXPECTED_HEAD
    before = sources()
    assert len(before) == 768
    source_path = OUT / "continued-validation-source-sha256.json"
    source_path.write_text(json.dumps(before, indent=2) + "\n")
    env = os.environ.copy()
    settings = {
        "CARGO_PROFILE_DEV_DEBUG": "0", "CARGO_PROFILE_TEST_DEBUG": "0",
        "CARGO_INCREMENTAL": "0", "CARGO_BUILD_JOBS": "2", "CARGO_TERM_COLOR": "never",
        "CARGO_TARGET_DIR": "/workspace/scratch/71bc431c4e4f/target-validation-cost",
    }
    env.update(settings)
    for name in ["P7_SCOPE_REVIEW_EVIDENCE_DIR", "P7_BM25_PACKING_DIAGNOSTIC_DIR",
                 "P5E_FIX_REVIEW_EVIDENCE", "P5E_PRIORITY_EVIDENCE"]:
        env.pop(name, None)
    semantic_targets = [
        "artifact_cache", "attempt_lease_contract", "bounded_parallel",
        "manifest_exact_integration", "p7_cache_allocation_independent_review",
        "publish_cas", "query_cache", "queue_worker", "retry_worker_layering",
        "semantic_degrade", "space_switch",
    ]
    common = ["cargo", "+1.95.0", "test", "--offline", "--locked", "-j2"]
    commands = [
        ("workspace-no-fail-fast", common + ["--workspace", "--all-targets", "--exclude", "cc-semantic", "--no-fail-fast"]),
        ("semantic-lib", common + ["-p", "cc-semantic", "--lib"]),
        ("semantic-normal-paths", common + ["-p", "cc-semantic"]
         + [item for target in semantic_targets for item in ["--test", target]]),
    ]
    feature_targets = ["p7_v05_all_lane_scope", "p7_v05_scope_public", "p7_v16_exact_oracle",
                       "p7_v16_oracle_independent_review", "p7_acceptance_matrix"]
    commands.append(("semantic-http-p7", common + ["--message-format=json-render-diagnostics", "-p", "cc-server", "--features", "semantic-http"]
                     + [item for target in feature_targets for item in ["--test", target]]))
    receipt = {
        "source_commit": EXPECTED_HEAD,
        "source_tree": git("rev-parse", "HEAD^{tree}"),
        "source_inputs": len(before),
        "source_manifest": source_path.name,
        "source_manifest_sha256": hashlib.sha256(source_path.read_bytes()).hexdigest(),
        "workflow_sha256": hashlib.sha256((ROOT / ".github/workflows/ci.yml").read_bytes()).hexdigest(),
        "environment": settings,
        "semantic_normal_targets": semantic_targets,
        "scope": "Parent-authorized continuation: unchanged workspace default scope with --no-fail-fast to retain known local sampler failure and run every remaining target; original semantic lib/normal paths; five explicit cc-server semantic-http P7 targets. Offline/local profile added.",
        "prior_attempt": "full-default-validation.json",
        "feature_targets": feature_targets,
        "not_enabled": ["ignored tests", "semantic_runtime", "historical process-fault/GC-WAL targets", "unlisted feature HTTP/MCP gates", "source guards"],
        "counting": "Only these four commands are totaled as a continuation run. Earlier failed default attempt, isolated sampler diagnosis, and narrow tests overlap and are never added to this total. Every nonzero command remains a failure.",
        "disk_free_bytes_before": shutil.disk_usage(ROOT).free,
        "commands": [],
    }
    for name, command in commands:
        log_path = OUT / f"continued-{name}.log"
        assert not log_path.exists(), f"immutable command log: {log_path}"
        print(json.dumps({"starting": name, "command": command}), flush=True)
        started = time.monotonic()
        with log_path.open("wb") as log:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        content = log_path.read_text()
        target = None
        summaries = []
        for line in content.splitlines():
            match = re.search(r"^\s+Running (.+?) \((.+)\)$", line)
            if match:
                target = {"target": match.group(1), "executable": match.group(2)}
            match = re.search(r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", line)
            if match:
                summaries.append({**(target or {}), "status": match.group(1), **dict(zip(
                    ["passed", "failed", "ignored", "measured", "filtered_out"], map(int, match.groups()[1:])
                ))})
        compiler_artifacts = []
        for line in content.splitlines():
            if not line.startswith("{"):
                continue
            try:
                artifact = json.loads(line)
            except json.JSONDecodeError:
                continue
            if artifact.get("reason") == "compiler-artifact" and artifact.get("executable"):
                target_name = artifact.get("target", {}).get("name")
                if target_name == "codecortex" or target_name in feature_targets:
                    artifact_path = Path(artifact["executable"])
                    compiler_artifacts.append({"cargo_artifact": artifact,
                                               "sha256": hashlib.sha256(artifact_path.read_bytes()).hexdigest(),
                                               "bytes": artifact_path.stat().st_size})
        row = {
            "name": name, "command": command, "exit_code": result.returncode,
            "elapsed_seconds": round(time.monotonic() - started, 3),
            "log": log_path.name, "log_sha256": hashlib.sha256(log_path.read_bytes()).hexdigest(),
            "summaries": summaries,
            "compiler_artifacts": compiler_artifacts,
        }
        binary = Path(settings["CARGO_TARGET_DIR"]) / "debug/codecortex"
        if binary.exists():
            row["stdio_binary_after_command"] = {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
        receipt["commands"].append(row)
        (OUT / "continued-validation.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(json.dumps({"finished": name, "exit_code": result.returncode,
                          "elapsed_seconds": row["elapsed_seconds"], "targets": len(summaries),
                          "passed": sum(s["passed"] for s in summaries),
                          "failed": sum(s["failed"] for s in summaries),
                          "ignored": sum(s["ignored"] for s in summaries)}), flush=True)
        if result.returncode:
            print(content[-6500:], flush=True)
            print(json.dumps({"continuing_after_recorded_failure": name}), flush=True)
    after = sources()
    receipt["source_inputs_unchanged"] = before == after
    receipt["source_mismatches"] = sorted(p for p in before.keys() | after.keys() if before.get(p) != after.get(p))
    receipt["head_at_finish"] = git("rev-parse", "HEAD")
    receipt["disk_free_bytes_after"] = shutil.disk_usage(ROOT).free
    binary = Path(settings["CARGO_TARGET_DIR"]) / "debug/codecortex"
    receipt["stdio_binary_at_finish"] = {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    receipt["observed"] = {key: sum(s[key] for row in receipt["commands"] for s in row["summaries"])
                           for key in ["passed", "failed", "ignored", "measured", "filtered_out"]}
    focus = {"p7_v05_v11_independent_review", "diag_p5e_fix_review_boundary_20261003", "packing_validation_work",
             "p7_validation_work", "python_inventory_capture", "python_inventory_revalidation"}
    receipt["focus_target_subset"] = [s for row in receipt["commands"] for s in row["summaries"]
                                     if Path(s.get("target", "")).stem in focus]
    receipt["passed"] = len(receipt["commands"]) == 4 and all(row["exit_code"] == 0 for row in receipt["commands"])
    receipt["passed"] = receipt["passed"] and before == after and receipt["head_at_finish"] == EXPECTED_HEAD
    (OUT / "continued-validation.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"authorized_continuation_all_passed": receipt["passed"], "observed": receipt["observed"],
                      "source_inputs_unchanged": receipt["source_inputs_unchanged"]}), flush=True)
    raise SystemExit(0 if receipt["passed"] else 1)


if __name__ == "__main__":
    main()
