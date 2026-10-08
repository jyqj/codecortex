#!/usr/bin/env python3
"""Bind and execute the original real-worker contention test with fake vectors."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

from p7_build_identity import source_snapshot
from p7_stdio_build_receipt import toolchain_identity
from p8_cold_build import digest, new_directory, write_json
from p8_rollback import require
from p8_runtime_build import (OBSERVER_FILES, observer_snapshot, private_build_environment,
                              seal_output, verify_output, verify_toolchain)


def artifact(messages, root, target):
    rows = [m for m in messages if m.get("reason") == "compiler-artifact"
            and m.get("target", {}).get("name") == "p7_worker_contention"
            and m.get("target", {}).get("kind") == ["test"] and m.get("executable")]
    require(len(rows) == 1, "exact original contention test executable is required")
    value = rows[0]
    profile = value.get("profile", {})
    require(value.get("features") == ["semantic"] and profile.get("test") is True
            and profile.get("opt_level") == "3" and profile.get("debug_assertions") is False,
            "contention execution requires the actual release semantic test profile")
    require(value.get("fresh") is False, "contention test was reused instead of built in the private target")
    require(Path(value["manifest_path"]).resolve() == root / "crates/cc-eval/Cargo.toml"
            and Path(value["target"]["src_path"]).resolve()
            == root / "crates/cc-eval/tests/p7_worker_contention.rs",
            "contention Cargo event belongs to different source")
    executable = Path(value["executable"])
    require(executable.is_absolute() and executable.is_file() and not executable.is_symlink()
            and executable.resolve(strict=True).is_relative_to(target),
            "contention Cargo executable is outside its private target")
    require(any(m.get("reason") == "build-finished" and m.get("success") is True for m in messages),
            "contention build success event is missing")
    return value


def execute(root, output):
    root, out = root.resolve(strict=True), new_directory(output)
    target = Path(os.environ.get("CARGO_TARGET_DIR", out.with_name(out.name + "-cargo-target"))).absolute().resolve()
    command = ["cargo", "test", "--release", "--locked", "--offline", "-p", "cc-eval",
               "--no-default-features", "--features", "semantic", "--test", "p7_worker_contention", "--no-run",
               "--message-format=json-render-diagnostics", "--target-dir", str(target)]
    record = dict(schema_version=1, status="running", build_command=command, target_dir=str(target),
                  scope="original CodeIndex worker; fake provider in same process; no external provider",
                  paid_provider_requests=0, release_approval=False, task_complete=False)
    try:
        require(not target.is_relative_to(out), "contention Cargo target must be outside retained evidence")
        environment, overrides = private_build_environment(root, target)
        before, observer_before = source_snapshot(root), observer_snapshot(root)
        record.update(source_before=before, observer_before=observer_before, compiler_environment=overrides,
                      target_initially_absent=True, build_dir=str(target),
                      toolchain_before=toolchain_identity(root, environment))
        write_json(out / "source-before.json", before)
        for relative in OBSERVER_FILES:
            destination = out / "observer-source" / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(root / relative, destination)
        with (out / "build.jsonl").open("xb") as stdout, (out / "build.stderr").open("xb") as stderr:
            build = subprocess.run(command, cwd=root, env=environment, stdout=stdout, stderr=stderr, timeout=4500)
        after, observer_after = source_snapshot(root), observer_snapshot(root)
        write_json(out / "source-after.json", after)
        record.update(build_exit_code=build.returncode, source_after=after,
                      observer_after=observer_after, toolchain_after=toolchain_identity(root, environment),
                      cargo_log_sha256=digest(out / "build.jsonl"),
                      stderr_sha256=digest(out / "build.stderr"),
                      compiler=record["toolchain_before"]["rustc"]["version"])
        require(before == after and observer_before == observer_after and build.returncode == 0,
                "contention build/source/observer verification failed")
        verify_toolchain(record)
        messages = [json.loads(line) for line in (out / "build.jsonl").read_text().splitlines() if line.strip()]
        selected = artifact(messages, root, target)
        source_executable = Path(selected["executable"]).resolve(strict=True)
        executable = out / "p7_worker_contention"
        shutil.copy2(source_executable, executable)
        executable.chmod(0o555)
        expected = digest(source_executable)
        require(digest(executable) == expected, "contention executable copy differs")
        record.update(cargo_artifact=selected, executable_sha256=expected,
                      copy_source=dict(path=str(source_executable), sha256=expected,
                                       bytes=source_executable.stat().st_size))
        env = {key: value for key, value in os.environ.items()
               if key in ("PATH", "HOME", "LANG", "LC_ALL", "TMPDIR", "SYSTEMROOT")}
        env["CODECORTEX_WORKER_CONTENTION_EVIDENCE_DIR"] = str(out / "raw")
        args = [str(executable), "--exact",
                "real_slow_backfill_preserves_local_progress_and_records_every_request", "--nocapture"]
        record["execution_command"] = args
        started = time.monotonic_ns()
        with (out / "execution.stdout").open("xb") as stdout, (out / "execution.stderr").open("xb") as stderr:
            result = subprocess.run(args, cwd=root, env=env, stdout=stdout, stderr=stderr, timeout=300)
        record.update(execution_exit_code=result.returncode, execution_elapsed_ns=time.monotonic_ns() - started,
                      executable_sha256_after=digest(executable), source_final=source_snapshot(root),
                      observer_final=observer_snapshot(root), toolchain_final=toolchain_identity(root, environment))
        require(record["executable_sha256_after"] == digest(source_executable) == expected
                and record["source_final"] == before and record["observer_final"] == observer_before
                and record["toolchain_final"] == record["toolchain_before"],
                "contention binary, source, observer or compiler changed during execution")
        require(result.returncode == 0, "original contention test failed; all existing raw records retained")
        text = (out / "execution.stdout").read_text()
        require("1 passed; 0 failed; 0 ignored" in text, "original selected test did not execute")
        require(sorted(p.name for p in (out / "raw").iterdir()) == ["seed-19", "seed-43", "seed-7"],
                "contention execution did not preserve all three original seeds")
        record.update(status="passed_observation", exit_code=0)
    except Exception as error:
        record.update(status="failed", exit_code=2, error=f"{type(error).__name__}: {error}")
    finally:
        record["files"] = {p.relative_to(out).as_posix(): digest(p) for p in sorted(out.rglob("*"))
                           if p.is_file() and p.name != "receipt.json"}
        write_json(out / "receipt.json", record)
        seal_output(out)
        verify_output(out)
    return record


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "verify":
        parser = argparse.ArgumentParser(description="Verify every retained contention artifact without changing its result")
        parser.add_argument("--output", type=Path, required=True)
        args = parser.parse_args(sys.argv[2:])
        seal = verify_output(args.output)
        receipt = json.loads((args.output / "receipt.json").read_text())
        print(json.dumps(dict(status="sealed_raw_verified", observation_status=receipt["status"],
                              files=len(seal["artifact_inventory"]))))
        raise SystemExit(0)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = execute(args.source, args.output)
    print(json.dumps({k: result.get(k) for k in ("status", "exit_code", "error", "executable_sha256")}, indent=2))
    raise SystemExit(result["exit_code"])
