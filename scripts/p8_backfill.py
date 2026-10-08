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
from p8_cold_build import digest, new_directory, write_json
from p8_rollback import require


def artifact(messages, root):
    rows = [m for m in messages if m.get("reason") == "compiler-artifact"
            and m.get("target", {}).get("name") == "p7_worker_contention"
            and m.get("target", {}).get("kind") == ["test"] and m.get("executable")]
    require(len(rows) == 1, "exact original contention test executable is required")
    value = rows[0]
    profile = value.get("profile", {})
    require(value.get("features") == ["semantic"] and profile.get("test") is True
            and profile.get("opt_level") == "3" and profile.get("debug_assertions") is False,
            "contention execution requires the actual release semantic test profile")
    require(Path(value["manifest_path"]).resolve() == root / "crates/cc-eval/Cargo.toml"
            and Path(value["target"]["src_path"]).resolve()
            == root / "crates/cc-eval/tests/p7_worker_contention.rs",
            "contention Cargo event belongs to different source")
    require(any(m.get("reason") == "build-finished" and m.get("success") is True for m in messages),
            "contention build success event is missing")
    return value


def execute(root, output):
    root, out = root.resolve(strict=True), new_directory(output)
    before = source_snapshot(root)
    write_json(out / "source-before.json", before)
    command = ["cargo", "test", "--release", "--locked", "--offline", "-p", "cc-eval",
               "--no-default-features", "--features", "semantic", "--test", "p7_worker_contention", "--no-run",
               "--message-format=json-render-diagnostics"]
    record = dict(schema_version=1, status="running", source_before=before, build_command=command,
                  scope="original CodeIndex worker; fake provider in same process; no external provider",
                  paid_provider_requests=0, release_approval=False, task_complete=False)
    try:
        with (out / "build.jsonl").open("xb") as stdout, (out / "build.stderr").open("xb") as stderr:
            build = subprocess.run(command, cwd=root, stdout=stdout, stderr=stderr, timeout=4500)
        after = source_snapshot(root)
        record.update(build_exit_code=build.returncode, source_after=after,
                      cargo_log_sha256=digest(out / "build.jsonl"),
                      compiler=subprocess.check_output(["rustc", "--version", "--verbose"], text=True))
        require(before == after and build.returncode == 0, "contention build/source verification failed")
        messages = [json.loads(line) for line in (out / "build.jsonl").read_text().splitlines() if line.strip()]
        selected = artifact(messages, root)
        source_executable = Path(selected["executable"]).resolve(strict=True)
        executable = out / "p7_worker_contention"
        shutil.copy2(source_executable, executable)
        executable.chmod(0o555)
        expected = digest(source_executable)
        require(digest(executable) == expected, "contention executable copy differs")
        record.update(cargo_artifact=selected, executable_sha256=expected)
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
                      executable_sha256_after=digest(executable), source_final=source_snapshot(root))
        require(record["executable_sha256_after"] == expected and record["source_final"] == before,
                "contention binary or source changed during execution")
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
    return record


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = execute(args.source, args.output)
    print(json.dumps({k: result.get(k) for k in ("status", "exit_code", "error", "executable_sha256")}, indent=2))
    raise SystemExit(result["exit_code"])
