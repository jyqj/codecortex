#!/usr/bin/env python3
"""Retain bounded byte prefixes around one independently registered task cell.

Only the task driver, its native result and the complete task aggregate establish
measurement success. This transport neither repairs missing bytes nor validates
a partial measurement. The original diagnostic module remains byte-identical.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import time

SCHEMA = "p8-profile-task-descriptive-v1"
SCOPE = "profile_task_descriptive_v1"
PROFILES = ("no_op", "body", "api", "config", "batch_1", "batch_10", "batch_100", "batch_1000", "fanout")
SCALES = (1000, 5000, 10000, 50000, 100000)
FANOUTS = (1, 4, 16, 64, 128)

# A private module instance keeps the old CLI and its tests' module globals
# unchanged. Reuse only the existing byte-copy/ACK/schedule/source algorithms.
_spec = importlib.util.spec_from_file_location(
    "_p8_task_profile_capture_core", Path(__file__).with_name("p8_scale_diagnostic_capture.py")
)
core = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(core)
core.OBSERVERS = (
    "scripts/p8_scale_diagnostic_capture.py",
    "scripts/p8_task_profile_capture.py",
    "scripts/tests/test_p8_task_profile_capture.py",
    "scripts/p8_task_profile_matrix.py",
    ".github/workflows/p8-task-profile.yml",
)
core.STREAMS = (
    "shard/native-shard/native/raw.jsonl",
    "shard/native-shard/native/worker.stderr",
    "shard/native-shard/supervisor.stdout",
    "shard/native-shard/supervisor.stderr",
    "driver.stdout", "driver.stderr", "wrapper.stdout", "wrapper.stderr",
)
core.METADATA = (
    "configuration.json", "supervisor-launch.json", "supervisor-started.json", "supervisor-terminal.json",
    "observer-before.json", "observer-after.json", "observer-after-error.json",
    "shard/task-shard.json",
    "shard/native-shard/registered-plan.json",
    "shard/native-shard/source-before.json", "shard/native-shard/source-after.json",
    "shard/native-shard/disk-preflight.json", "shard/native-shard/disk-after.json",
    "shard/native-shard/shard.json",
    "shard/native-shard/native/plan.json", "shard/native-shard/native/report.json",
    "shard/native-shard/native/worker-summary.json",
)
require = core.require


def launch(root, build, directory, expected_source, run_id, attempt, profile, scale, fanout=None):
    require(profile in PROFILES and type(scale) is int and scale in SCALES, "unknown task cell")
    require((profile == "fanout" and scale == 1000 and type(fanout) is int and fanout in FANOUTS)
            or (profile != "fanout" and fanout is None), "task fanout selection differs")
    require(isinstance(run_id, str) and run_id.isascii() and run_id.isdecimal()
            and 0 < int(run_id) < 2 ** 64 and type(attempt) is int and 0 < attempt < 2 ** 32,
            "official task run/attempt is required")
    root, build = Path(root).resolve(strict=True), Path(build).resolve(strict=True)
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    observed = core.observer_snapshot(root)
    require(observed["source"] == expected_source, "checkout differs from fixed task source")
    core.write_new(directory / "observer-before.json", observed)
    command = [sys.executable, str(root / "scripts/p8_task_profile_matrix.py"), "run",
               "--root", str(root), "--build", str(build), "--scale", str(scale),
               "--shard-index", "0", "--mutation-profile", profile,
               "--run-id", run_id, "--attempt", str(attempt),
               "--output", str(directory / "shard")]
    if fanout is not None:
        command.extend(["--fanout", str(fanout)])
    configuration = dict(
        schema="p8-task-cell-prefix-capture-v1", task_protocol=SCHEMA, stage_scope=SCOPE,
        source=expected_source, source_root=str(root), run_id=run_id, run_attempt=attempt,
        cell=dict(mutation_profile=profile, scale=scale, fanout=fanout, repetition=0),
        observer_before_sha256=core.sha(directory / "observer-before.json"),
        task_driver_sha256=core.sha(root / "scripts/p8_task_profile_matrix.py"),
        task_build_receipt_sha256=core.sha(build / "task-build.json"),
        command=command, started_monotonic=time.monotonic(),
        checkpoint_interval_seconds=core.INTERVAL_SECONDS, checkpoint_count=core.CHECKPOINTS,
        outer_job_seconds=core.JOB_SECONDS, diagnostic_only=True,
        native_budget="18000000ms / 536870912 output bytes; unchanged native supervisor and driver tail",
        task_completion_credit=False,
        observer_scope="Original byte prefixes only; complete task-shard artifacts remain separately required",
    )
    core.write_new(directory / "configuration.json", configuration)
    with (directory / "wrapper.stdout").open("xb") as stdout, (directory / "wrapper.stderr").open("xb") as stderr:
        child = subprocess.Popen(
            [sys.executable, str(Path(__file__).resolve()), "supervise", "--directory", str(directory)],
            stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, start_new_session=True
        )
    core.write_new(directory / "supervisor-launch.json",
                   dict(supervisor_pid=child.pid, started_monotonic=configuration["started_monotonic"]))
    return configuration


def supervise(directory, command, wall_seconds=core.JOB_SECONDS):
    """Own the driver child and observe the task's different native-report path."""
    directory = Path(directory)
    started = time.monotonic()
    result = dict(
        schema="p8-task-capture-driver-exit-v1", task_protocol=SCHEMA, command=command,
        driver_returncode=None, driver_signal=None, native_exit_code=None,
        native_worker_exit_code=None, wrapper_timeout=False,
        native_process_cleanup="owned by original Rust supervisor; not independently established",
        success_is_not_certification=True,
    )
    try:
        with (directory / "driver.stdout").open("xb") as stdout, (directory / "driver.stderr").open("xb") as stderr:
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                     start_new_session=True)
            core.write_new(directory / "supervisor-started.json",
                           dict(driver_pid=child.pid, driver_pgid=child.pid, started_monotonic=started,
                                wall_limit_seconds=wall_seconds, native_worker_has_separate_process_group=True))
            try:
                code = child.wait(timeout=wall_seconds)
            except subprocess.TimeoutExpired:
                result["wrapper_timeout"] = True
                child.kill()
                code = child.wait(timeout=10)
            result.update(driver_returncode=code, driver_signal=-code if code < 0 else None)
    except (OSError, subprocess.SubprocessError) as error:
        result["wrapper_error"] = str(error)
    report = directory / "shard/native-shard/native/report.json"
    if report.is_file():
        try:
            original = core.read_json(report)
            result.update(native_report_sha256=core.sha(report), native_report_status=original.get("status"),
                          native_exit_code=original.get("exit_code"),
                          native_worker_exit_code=original.get("worker_exit_code"))
        except (OSError, ValueError) as error:
            result["native_report_read_error"] = str(error)
    result["wall_seconds"] = time.monotonic() - started
    # The terminal receipt is published only after observing the nested native
    # report, so checkpoints cannot mistake an intermediate receipt for this one.
    core.write_new(directory / "supervisor-terminal.json", result)
    return result


def verdict(directory):
    directory = Path(directory)
    try:
        if (directory / "observer-after-error.json").exists():
            return 1
        if core.read_json(directory / "observer-before.json") != core.read_json(directory / "observer-after.json"):
            return 1
        terminal = core.read_json(directory / "supervisor-terminal.json")
        if terminal.get("wrapper_timeout") or terminal.get("wrapper_error") or terminal.get("driver_returncode") != 0:
            code = terminal.get("driver_returncode")
            return code if type(code) is int and 0 < code < 126 else 1
        outer = core.read_json(directory / "shard/task-shard.json")
        inner = core.read_json(directory / "shard/native-shard/shard.json")
        report = directory / "shard/native-shard/native/report.json"
        original = core.read_json(report)
        return 0 if (
            outer.get("schema") == SCHEMA and outer.get("stage_scope") == SCOPE
            and outer.get("status") == "passed" and outer.get("passed") is True
            and inner.get("status") == "passed" and inner.get("exit_code") == 0
            and terminal.get("native_report_sha256") == core.sha(report)
            and original.get("status") == terminal.get("native_report_status") == "measurement_complete"
            and original.get("exit_code") == terminal.get("native_exit_code") == 0
        ) else 1
    except (OSError, ValueError, KeyError, TypeError):
        return 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["launch", "supervise", "checkpoint", "finish", "verdict"])
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--root", type=Path)
    parser.add_argument("--build", type=Path)
    parser.add_argument("--expected-source")
    parser.add_argument("--run-id")
    parser.add_argument("--attempt", type=int)
    parser.add_argument("--mutation-profile", choices=PROFILES)
    parser.add_argument("--scale", type=int, choices=SCALES)
    parser.add_argument("--fanout", type=int, choices=FANOUTS)
    parser.add_argument("--index", type=int, default=0)
    args = parser.parse_args()
    if args.mode == "launch":
        launch(args.root, args.build, args.directory, args.expected_source, args.run_id, args.attempt,
               args.mutation_profile, args.scale, args.fanout)
    elif args.mode == "supervise":
        configuration = core.read_json(args.directory / "configuration.json")
        require(configuration.get("task_protocol") == SCHEMA and configuration.get("stage_scope") == SCOPE,
                "another protocol's capture configuration")
        supervise(args.directory, configuration["command"])
    elif args.mode in ("checkpoint", "finish"):
        core.checkpoint(args.directory, args.index,
                        os.environ.get("PREVIOUS_ARTIFACT_ID", ""),
                        os.environ.get("PREVIOUS_ARTIFACT_DIGEST", ""), final=args.mode == "finish")
    else:
        return verdict(args.directory)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
