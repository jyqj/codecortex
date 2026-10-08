#!/usr/bin/env python3
"""Build the exact union source using the retained dependency cache.

The preceding recorded cargo clean invalidates every workspace package.
This observer requires every resulting workspace artifact to be non-fresh.
It retains the actual Cargo streams and freezes a copy for stdio controls.
"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

sys.dont_write_bytecode = True
BASE = Path("/workspace/scratch/031390cf22eb")
ROOT = BASE / "codecortex-union-validation"
OUT = BASE / "round5-validation/product"
SOURCE = "1ed3c7df574db6d450f79ef29d5148f68556b3db"
TREE = "d16aa044e56c9347e6b4ff75d783cc4317f15087"
CARGO = BASE / "toolchains/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo"
TARGET = BASE / "target-scale"
sys.path.insert(0, str(ROOT / "scripts"))
from p7_build_identity import file_sha256, json_bytes, source_snapshot, verify_cargo_artifact
from p7_stdio_build_receipt import compiler_environment, toolchain_identity
from p8_candidate_execution import artifact_from_log


def write(path, value):
    path.write_bytes(json_bytes(value))


def main():
    OUT.mkdir(exist_ok=False)
    before = source_snapshot(ROOT)
    assert before["source_commit"] == SOURCE and before["source_tree"] == TREE
    clean_path = BASE / "round5-validation/union-workspace-cache-clean.json"
    clean = json.loads(clean_path.read_text())
    assert clean["exit_code"] == 0 and clean["source_unchanged"]
    assert clean["source_identity"]["head"] == SOURCE
    environment, compiler_overrides = compiler_environment(ROOT)
    assert Path(environment["CARGO_TARGET_DIR"]) == TARGET
    assert Path(environment["CARGO_BUILD_BUILD_DIR"]) == TARGET
    tools_before = toolchain_identity(ROOT, environment)
    observer_before = file_sha256(__file__)
    command = [str(CARGO), "build", "-p", "cc-server", "--bin", "codecortex",
               "--no-default-features", "--locked", "--message-format=json-render-diagnostics"]
    write(OUT / "source-inputs.json", before["inputs"])
    receipt = {
        "schema_version": 1,
        "source_before": {k: v for k, v in before.items() if k != "inputs"},
        "source_manifest": "source-inputs.json",
        "build_command": command,
        "cwd": str(ROOT),
        "build_profile": "dev",
        "target_dir": str(TARGET),
        "target_initially_absent": False,
        "workspace_clean_receipt": str(clean_path),
        "workspace_clean_receipt_sha256": file_sha256(clean_path),
        "toolchain": tools_before,
        "compiler_overrides": compiler_overrides,
        "environment": {key: environment.get(key) for key in (
            "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN", "RUSTC",
            "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTFLAGS",
            "CARGO_TARGET_DIR", "CARGO_BUILD_BUILD_DIR", "CARGO_BUILD_JOBS",
            "CARGO_INCREMENTAL", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG")},
        "observer_sha256": observer_before,
        "started_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "status": "running",
        "scope": "Exact-source default dev binary for functional stdio regressions. Third-party dependency cache reused after invalidating workspace packages. No optimized-release, performance or quality certification.",
    }
    path = OUT / "build-receipt.json"
    write(path, receipt)
    started = time.monotonic()
    with (OUT / "cargo-build.jsonl").open("wb") as stdout, (OUT / "cargo-build.stderr.log").open("wb") as stderr:
        process = subprocess.run(command, cwd=ROOT, env=environment, stdout=stdout, stderr=stderr)
    receipt.update({
        "build_exit_code": process.returncode,
        "elapsed_seconds": round(time.monotonic() - started, 6),
        "finished_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "cargo_stdout_sha256": file_sha256(OUT / "cargo-build.jsonl"),
        "cargo_stderr_sha256": file_sha256(OUT / "cargo-build.stderr.log"),
    })
    after = source_snapshot(ROOT)
    receipt["source_after"] = {k: v for k, v in after.items() if k != "inputs"}
    receipt["source_unchanged"] = before == after
    if process.returncode:
        receipt["status"] = "failed"
        write(path, receipt)
        print(json.dumps({"status": "failed", "exit_code": process.returncode, "receipt": str(path)}))
        return process.returncode
    try:
        assert before == after, "source changed during build"
        assert tools_before == toolchain_identity(ROOT, environment), "toolchain changed during build"
        assert observer_before == file_sha256(__file__), "observer changed during build"
        artifact = artifact_from_log(OUT / "cargo-build.jsonl", "codecortex")
        executable = verify_cargo_artifact(ROOT, artifact, "default")
        assert executable.is_relative_to(TARGET), "product outside the selected cache"
        assert artifact.get("fresh") is False, "workspace product was not compiled after cache invalidation"
        messages = [json.loads(line) for line in (OUT / "cargo-build.jsonl").read_text().splitlines()]
        workspace = [m for m in messages if m.get("reason") == "compiler-artifact"
                     and Path(m.get("manifest_path", "/")).parent.parent == ROOT / "crates"]
        packages = {Path(m["manifest_path"]).parent.name for m in workspace}
        assert packages == {"cc-model", "cc-db", "cc-parsers", "cc-index", "cc-search", "cc-server"}, packages
        assert all(m.get("fresh") is False for m in workspace), "stale workspace library artifact"
        snapshot = OUT / "codecortex"
        shutil.copy2(executable, snapshot)
        assert file_sha256(snapshot) == file_sha256(executable), "copied product bytes changed"
        snapshot.chmod(0o555)
        receipt.update({
            "status": "passed",
            "cargo_artifact": artifact,
            "actual_cargo_profile": artifact["profile"],
            "workspace_cargo_artifacts": workspace,
            "workspace_package_count": len(packages),
            "workspace_artifact_count": len(workspace),
            "all_workspace_artifacts_fresh_false": True,
            "binary_path": str(snapshot),
            "binary_sha256": file_sha256(snapshot),
            "binary_bytes": snapshot.stat().st_size,
            "toolchain_unchanged": True,
            "observer_unchanged": True,
        })
    except Exception as error:
        receipt.update(status="identity_validation_failed", error=repr(error))
        write(path, receipt)
        raise
    write(path, receipt)
    print(json.dumps({"status": receipt["status"], "binary": receipt["binary_path"],
                      "binary_sha256": receipt["binary_sha256"], "source": SOURCE,
                      "profile": receipt["actual_cargo_profile"],
                      "workspace_artifacts_fresh_false": len(workspace),
                      "receipt": str(path), "receipt_sha256": file_sha256(path)}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
