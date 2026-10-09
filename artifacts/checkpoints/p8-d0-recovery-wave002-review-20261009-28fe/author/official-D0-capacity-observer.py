#!/usr/bin/env python3
"""Measure capacity; optionally remove two unused SDKs on this hosted CI VM.

This is preparation of a new GitHub-hosted Ubuntu VM, never a shared or
self-hosted machine. It does not allocate storage or lower the registered
capacity requirement. No caller-supplied deletion path is accepted.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time

SCALES = (1000, 5000, 10000, 50000, 100000)
SDK_PATHS = (Path("/usr/local/lib/android"), Path("/usr/share/dotnet"))
CONTEXT_KEYS = ("GITHUB_REPOSITORY", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_JOB",
                "RUNNER_OS", "RUNNER_ENVIRONMENT", "RUNNER_NAME", "ImageOS", "ImageVersion",
                "GITHUB_WORKSPACE", "RUNNER_TEMP", "P8_EXPECTED_SOURCE")


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def required_bytes(scale):
    require(type(scale) is int and scale in SCALES, "unregistered scale")
    return scale * 256 * 1024 + 4 * 1024**3


def regular_directory(path):
    path = Path(path)
    require(path.is_absolute(), "absolute directory required")
    for parent in (path, *path.parents):
        metadata = parent.lstat()
        require(stat.S_ISDIR(metadata.st_mode) and not stat.S_ISLNK(metadata.st_mode),
                "directory or ancestor is not a real directory: " + str(parent))
    require(path.resolve(strict=True) == path, "directory resolution changed")
    return path


def hosted_context(environment):
    require(sys.platform == "linux" and environment.get("GITHUB_ACTIONS") == "true"
            and environment.get("GITHUB_REPOSITORY") == "jyqj/codecortex"
            and environment.get("RUNNER_ENVIRONMENT") == "github-hosted"
            and environment.get("RUNNER_OS") == "Linux"
            and environment.get("ImageOS") == "ubuntu24"
            and re.fullmatch(r"[1-9][0-9]*", environment.get("GITHUB_RUN_ID", ""))
            and re.fullmatch(r"[0-9a-f]{40}", environment.get("P8_EXPECTED_SOURCE", "")),
            "capacity preparation requires this repository's fresh GitHub-hosted Ubuntu 24 VM")
    workspace = regular_directory(environment.get("GITHUB_WORKSPACE", ""))
    temporary = regular_directory(environment.get("RUNNER_TEMP", ""))
    hosted_work = Path("/home/runner/work")
    require(hosted_work in workspace.parents and hosted_work in temporary.parents
            and workspace != temporary and workspace not in temporary.parents
            and temporary not in workspace.parents,
            "workspace and temporary directory are not separate hosted job paths")
    return workspace, temporary


def safe_sdk(path, protected):
    require(path in SDK_PATHS, "only the two fixed unused SDK directories are eligible")
    require(not path.is_symlink(), "SDK path is a symlink")
    if not path.exists():
        return False
    regular_directory(path)
    require(not path.is_mount(), "SDK directory is a mount")
    for other in protected:
        other = Path(other).resolve()
        require(path != other and path not in other.parents and other not in path.parents,
                "SDK overlaps source, temporary files, toolchain or evidence")
    return True


def disk(path):
    value = shutil.disk_usage(path)
    return {"total_bytes": value.total, "used_bytes": value.used, "free_bytes": value.free}


def save(path, value):
    temporary = path.with_suffix(".pending")
    temporary.write_text(json.dumps(value, sort_keys=True, indent=2) + "\n")
    temporary.replace(path)


def privileged_argv(operation):
    require(operation and operation[0] in ("du", "rm")
            and Path(operation[-1]) in SDK_PATHS, "unexpected SDK command")
    # The deadline owner has the same privileges as du/rm and signals their
    # complete group. Timing out only the caller's sudo PID would be inadequate.
    return ["sudo", "-n", "timeout", "--signal=TERM", "--kill-after=5s", "300s", *operation]


def command(operation, directory, label):
    argv = privileged_argv(operation)
    record = {"argv": argv, "operation": operation, "status": "started", "exit_code": None,
              "started_monotonic_ns": time.monotonic_ns(),
              "supervisor": "same-privilege GNU timeout process group",
              "deadline_seconds": 300, "kill_grace_seconds": 5, "outer_supervision_seconds": 320,
              "stdout": label + ".stdout", "stderr": label + ".stderr", "cleanup_complete": False}
    receipt = directory / (label + ".command.json")
    require(not receipt.exists(), "command receipt already exists")
    # Preserve the actual argv before spawning, including failed/timeout paths.
    save(receipt, record)
    try:
        with (directory / record["stdout"]).open("xb") as stdout, (directory / record["stderr"]).open("xb") as stderr:
            result = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                    timeout=320, check=False)
        record.update(exit_code=result.returncode, cleanup_complete=True,
                      status="completed" if result.returncode == 0 else
                      "deadline_exceeded" if result.returncode in (124, 137) else "command_failed")
    except subprocess.TimeoutExpired as error:
        # An unresponsive privileged supervisor cannot be declared reaped from
        # this unprivileged observer. Raw stays available but explicitly unsealed.
        record.update(status="outer_supervisor_timeout_cleanup_unconfirmed", error=str(error))
    except OSError as error:
        record.update(status="spawn_failed", cleanup_complete=True, error=str(error))
    finally:
        record["elapsed_ns"] = time.monotonic_ns() - record["started_monotonic_ns"]
        save(receipt, record)
    return record


def prepare(scale, output, allow_preparation=False):
    need = required_bytes(scale)
    workspace, temporary = hosted_context(os.environ)
    require(output.is_absolute() and not output.is_symlink(), "absolute owned evidence path required")
    require(output.parent.resolve(strict=True) == temporary and not output.exists(),
            "capacity evidence must be a new direct child of RUNNER_TEMP")
    output.mkdir()
    context = {key: os.environ.get(key) for key in CONTEXT_KEYS}
    report = {"schema_version": 1, "profile": "scale_capacity_v1", "scale": scale,
              "required_free_bytes": need, "context": context, "steps": [], "status": "running",
              "scope": "fresh hosted VM preparation only; no product measurement or storage allocation",
              "allow_preparation": allow_preparation, "initial": disk(workspace)}
    receipt = output / "receipt.json"
    try:
        script = Path(__file__).resolve(strict=True)
        require(script == workspace / "scripts/p8_runner_capacity.py", "capacity observer belongs to another checkout")
        head = subprocess.check_output(["git", "-C", str(workspace), "rev-parse", "HEAD"], text=True).strip()
        require(head == os.environ["P8_EXPECTED_SOURCE"], "capacity source head differs")
        committed = subprocess.check_output(["git", "-C", str(workspace), "show", head + ":scripts/p8_runner_capacity.py"])
        require(committed == script.read_bytes(), "capacity observer differs from its committed source")
        report.update(source_commit=head, observer_sha256=hashlib.sha256(committed).hexdigest())
        protected = [workspace, temporary, output]
        protected += [Path(os.environ[key]) for key in ("CARGO_HOME", "RUSTUP_HOME") if os.environ.get(key)]
        for number, target in enumerate(SDK_PATHS):
            current = disk(workspace)
            if current["free_bytes"] >= need:
                break
            require(allow_preparation and os.environ.get("P8_CAPACITY_PREPARE_ALLOWED") == "true",
                    "capacity insufficient; explicit hosted SDK preparation is disabled")
            step = {"target": str(target), "before": current, "existed": safe_sdk(target, protected)}
            report["steps"].append(step)
            save(receipt, report)
            if not step["existed"]:
                continue
            step["directory_size"] = command(["du", "-sx", "--block-size=1", "--", str(target)],
                                             output, f"sdk-{number}-size")
            require(step["directory_size"]["exit_code"] == 0, "SDK size observation failed")
            # Re-check the exact directory immediately before a fixed deletion.
            require(safe_sdk(target, protected), "SDK changed during preparation")
            step["removal"] = command(["rm", "-rf", "--one-file-system", "--preserve-root=all", "--", str(target)],
                                     output, f"sdk-{number}-remove")
            step["after"] = disk(workspace)
            save(receipt, report)
            require(step["removal"]["exit_code"] == 0 and not target.exists(), "unused SDK preparation failed")
        report["final"] = disk(workspace)
        require(report["final"]["free_bytes"] >= need, "registered capacity remains unavailable")
        require(Path(__file__).read_bytes() == committed, "capacity observer changed during preparation")
        require(subprocess.check_output(["git", "-C", str(workspace), "rev-parse", "HEAD"], text=True).strip() == head,
                "capacity source changed during preparation")
        report.update(status="capacity_available", exit_code=0)
    except Exception as error:
        report.update(status="not_run_capacity_unavailable", exit_code=2,
                      error=f"{type(error).__name__}: {error}")
    finally:
        cleanup = all(step[key].get("cleanup_complete") is True for step in report["steps"]
                      for key in ("directory_size", "removal") if key in step)
        report["artifact_state"] = "sealed_after_commands_finished" if cleanup else "unsealed_cleanup_unconfirmed"
        if cleanup:
            report["files"] = {path.name: {"bytes": path.stat().st_size,
                                          "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                               for path in sorted(output.iterdir()) if path.is_file() and path != receipt}
        else:
            report["unsealed_artifacts"] = [path.name for path in sorted(output.iterdir()) if path.is_file() and path != receipt]
        save(receipt, report)
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scale", required=True, type=int, choices=SCALES)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--prepare-ephemeral", action="store_true")
    args = parser.parse_args()
    try:
        result = prepare(args.scale, args.output, args.prepare_ephemeral)
        print(json.dumps({key: result.get(key) for key in ("status", "exit_code", "error", "required_free_bytes")}, indent=2))
        raise SystemExit(result["exit_code"])
    except (RuntimeError, OSError) as error:
        print(f"capacity preparation refused: {error}", file=sys.stderr)
        raise SystemExit(2)
