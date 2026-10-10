#!/usr/bin/env python3
"""Independent diagnostic transport around the unchanged P8 scale driver.

This does not validate or certify a shard. Captured streams are byte prefixes,
including an optional partial JSON line, never invented EOF or completed events.
Only a successful upload-artifact output acknowledges remotely retained bytes.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

INTERVAL_SECONDS = 15 * 60
CHECKPOINTS = 20
JOB_SECONDS = 350 * 60
OBSERVERS = (".github/workflows/p8-scale-100k-diagnostic.yml",
             "scripts/p8_scale_diagnostic_capture.py",
             "scripts/tests/test_p8_scale_diagnostic_capture.py")
STREAMS = (
    "shard/native/raw.jsonl", "shard/native/worker.stderr",
    "shard/supervisor.stdout", "shard/supervisor.stderr",
    "driver.stdout", "driver.stderr",
)
METADATA = (
    "configuration.json", "supervisor-started.json", "supervisor-terminal.json",
    "observer-before.json", "observer-after.json", "observer-after-error.json",
    "shard/registered-plan.json", "shard/source-before.json", "shard/source-after.json",
    "shard/disk-preflight.json", "shard/disk-after.json", "shard/shard.json",
    "shard/native/plan.json", "shard/native/report.json", "shard/native/worker-summary.json",
)


def require(ok, message):
    if not ok:
        raise ValueError(message)


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def write_new(path, value):
    path = Path(path)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.writing")
    with temporary.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")
    # Publish a complete receipt atomically, without replacing an existing one.
    try:
        os.link(temporary, path)
    finally:
        temporary.unlink()


def save_state(path, value):
    temporary = Path(str(path) + ".new")
    write_new(temporary, value)
    os.replace(temporary, path)


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(65536), b""):
            digest.update(block)
    return digest.hexdigest()


def observer_snapshot(root):
    root = Path(root)
    head = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                          check=True, capture_output=True, text=True).stdout.strip()
    inputs = {}
    for relative in OBSERVERS:
        path = root / relative
        require(path.is_file() and not path.is_symlink(), "observer is not a regular file")
        data = path.read_bytes()
        oid = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        original = subprocess.run(["git", "-C", str(root), "ls-tree", "HEAD", "--", relative],
                                  check=True, capture_output=True, text=True).stdout.strip()
        fields = original.split()
        require(len(fields) == 4 and fields[1] == "blob" and fields[2] == oid and fields[3] == relative,
                "observer bytes differ from actual Git source")
        actual_mode = "100755" if path.stat().st_mode & 0o111 else "100644"
        require(actual_mode == fields[0], "observer executable mode differs from Git")
        inputs[relative] = {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                            "git_blob": oid, "git_mode": fields[0]}
    return {"schema": "p8-diagnostic-observer-source-v1", "source": head, "inputs": inputs,
            "scope": "observer identity only; original product and native output domains unchanged"}


def observer_after(directory):
    directory = Path(directory)
    try:
        configuration = read_json(directory / "configuration.json")
        after = observer_snapshot(configuration["source_root"])
        before = read_json(directory / "observer-before.json")
        write_new(directory / "observer-after.json", after)
        require(after == before, "observer source changed during diagnostic")
        return True
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        write_new(directory / "observer-after-error.json", {"error": str(error)})
        return False


def emit_outputs(**values):
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as stream:
            for key, value in values.items():
                require("\n" not in str(value), "invalid action output")
                stream.write(f"{key}={value}\n")


def supervise(directory, command, wall_seconds=JOB_SECONDS):
    """Own only the driver child. The unchanged Rust supervisor owns its worker."""
    directory = Path(directory)
    started = time.monotonic()
    result = {"schema": "p8-diagnostic-driver-exit-v1", "command": command,
              "driver_returncode": None, "driver_signal": None,
              "native_exit_code": None, "native_worker_exit_code": None,
              "native_process_cleanup": "owned by original Rust supervisor; not independently established",
              "wrapper_timeout": False, "success_is_not_certification": True}
    try:
        with (directory / "driver.stdout").open("xb") as stdout, (directory / "driver.stderr").open("xb") as stderr:
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                                     start_new_session=True)
            write_new(directory / "supervisor-started.json",
                      {"driver_pid": child.pid, "driver_pgid": child.pid,
                       "started_monotonic": started, "wall_limit_seconds": wall_seconds,
                       "native_worker_has_separate_process_group": True})
            try:
                code = child.wait(timeout=wall_seconds)
            except subprocess.TimeoutExpired:
                result["wrapper_timeout"] = True
                # Kill exactly our unreaped child, never the caller/shared PGID.
                # This is NOT evidence that the native worker's separate PGID is gone.
                child.kill()
                code = child.wait(timeout=10)
            result["driver_returncode"] = code
            result["driver_signal"] = -code if code < 0 else None
    except (OSError, subprocess.SubprocessError) as error:
        result["wrapper_error"] = str(error)
    report = directory / "shard/native/report.json"
    if report.is_file():
        try:
            original = read_json(report)
            result["native_report_sha256"] = sha(report)
            result["native_report_status"] = original.get("status")
            result["native_exit_code"] = original.get("exit_code")
            result["native_worker_exit_code"] = original.get("worker_exit_code")
        except (OSError, ValueError) as error:
            result["native_report_read_error"] = str(error)
    result["wall_seconds"] = time.monotonic() - started
    write_new(directory / "supervisor-terminal.json", result)
    return result


def launch(root, build, directory, expected_source):
    root, build = Path(root).resolve(strict=True), Path(build).resolve(strict=True)
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    actual = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                            check=True, capture_output=True, text=True).stdout.strip()
    require(actual == expected_source, "checkout differs from fixed source")
    observed = observer_snapshot(root)
    require(observed["source"] == expected_source, "observer checkout changed during launch")
    write_new(directory / "observer-before.json", observed)
    command = [sys.executable, str(root / "scripts/p8_scale_matrix.py"), "run",
               "--root", str(root), "--build", str(build), "--scale", "100000",
               "--shard-index", "0", "--shard-count", "30", "--repetitions", "30",
               "--capacity-profile", "scale_wide_dirty_v1", "--output", str(directory / "shard")]
    configuration = {"schema": "p8-independent-100k-diagnostic-v1", "source": actual,
                     "source_root": str(root),
                     "observer_before_sha256": sha(directory / "observer-before.json"),
                     "run_id": os.environ.get("GITHUB_RUN_ID"),
                     "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
                     "command": command, "started_monotonic": time.monotonic(),
                     "checkpoint_interval_seconds": INTERVAL_SECONDS, "checkpoint_count": CHECKPOINTS,
                     "outer_job_seconds": JOB_SECONDS, "diagnostic_only": True,
                     "original_driver_sha256": sha(root / "scripts/p8_scale_matrix.py"),
                     "original_build_receipt_sha256": sha(build / "build.json"),
                     "original_native_budget_unchanged": "18000000ms / 512MiB; driver retains its original +120s timeout",
                     "registered_N_unchanged": 30, "whole_cohort_completion_credit": False}
    write_new(directory / "configuration.json", configuration)
    with (directory / "wrapper.stdout").open("xb") as stdout, (directory / "wrapper.stderr").open("xb") as stderr:
        supervisor = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "supervise",
                                       "--directory", str(directory)], stdin=subprocess.DEVNULL,
                                      stdout=stdout, stderr=stderr, start_new_session=True)
    write_new(directory / "supervisor-launch.json", {"supervisor_pid": supervisor.pid,
                                                     "started_monotonic": configuration["started_monotonic"]})
    # RUNNER_TRACKING_ID is not removed; normal runner job cleanup still applies.
    return configuration


def empty_state():
    return {"streams": {}, "chunks": [], "acknowledged_chunks": [], "uploads": []}


def acknowledge(directory, state, index, artifact_id, digest):
    if not artifact_id:
        return
    require(str(artifact_id).isdigit() and int(artifact_id) > 0, "invalid upload artifact ID")
    require(len(digest) == 64 and all(c in "0123456789abcdef" for c in digest), "invalid upload digest")
    manifest = read_json(Path(directory) / "checkpoints" / f"{index:02d}" / "manifest.json")
    require(manifest["index"] == index, "upload checkpoint mismatch")
    known = {chunk["name"] for chunk in state["chunks"]}
    names = manifest["included_chunk_names"]
    require(set(names) <= known, "upload refers to unknown local chunks")
    state["acknowledged_chunks"] = sorted(set(state["acknowledged_chunks"]) | set(names))
    state["uploads"].append({"checkpoint": index, "artifact_id": str(artifact_id),
                             "artifact_digest": digest, "manifest_sha256": sha(Path(directory) / "checkpoints" / f"{index:02d}" / "manifest.json"),
                             "included_chunk_names": names})


def snapshot(directory, index, previous_id="", previous_digest=""):
    directory = Path(directory)
    state_path = directory / "capture-state.json"
    state = read_json(state_path) if state_path.exists() else empty_state()
    if index > 0:
        acknowledge(directory, state, index - 1, previous_id, previous_digest)
    bundle = directory / "checkpoints" / f"{index:02d}"
    bundle.mkdir(parents=True, exist_ok=False)
    chunks = directory / "chunks"
    chunks.mkdir(exist_ok=True)
    faults = []
    for relative in STREAMS:
        source = directory / relative
        if not source.exists():
            continue
        require(source.is_file() and not source.is_symlink(), "capture source is not a regular file")
        with source.open("rb") as incoming:
            stat = os.fstat(incoming.fileno())
            prior = state["streams"].get(relative, {"end": 0, "device": stat.st_dev, "inode": stat.st_ino})
            if (stat.st_dev, stat.st_ino) != (prior["device"], prior["inode"]) or stat.st_size < prior["end"]:
                faults.append({"path": relative, "reason": "stream_replaced_or_truncated", "prior": prior, "size": stat.st_size})
                continue
            start, end = prior["end"], stat.st_size
            if start == end:
                continue
            name = relative.replace("/", "_") + f".{start:012d}-{end:012d}.bin"
            incoming.seek(start)
            digest = hashlib.sha256()
            remaining = end - start
            with (chunks / name).open("xb") as outgoing:
                while remaining:
                    block = incoming.read(min(remaining, 65536))
                    require(bool(block), "source shortened while copying frozen byte interval")
                    outgoing.write(block)
                    digest.update(block)
                    remaining -= len(block)
            state["chunks"].append({"name": name, "source": relative, "start": start, "end": end,
                                     "bytes": end - start, "sha256": digest.hexdigest()})
            state["streams"][relative] = {"end": end, "device": stat.st_dev, "inode": stat.st_ino}
    acknowledged = set(state["acknowledged_chunks"])
    pending = [c for c in state["chunks"] if c["name"] not in acknowledged]
    # A failed upload does not advance custody: the next artifact includes those chunks again.
    for chunk in pending:
        os.link(chunks / chunk["name"], bundle / chunk["name"])
    metadata = []
    for relative in METADATA:
        source = directory / relative
        if source.is_file():
            require(not source.is_symlink(), "metadata source is not a regular owned file")
            target = bundle / (relative.replace("/", "_") + ".snapshot")
            # These are raw point-in-time bytes, not asserted atomic JSON or a completed receipt.
            shutil.copyfile(source, target)
            metadata.append({"source": relative, "file": target.name, "bytes": target.stat().st_size,
                             "sha256": sha(target)})
    prefix = {}
    for relative in STREAMS:
        position = 0
        for chunk in sorted((c for c in state["chunks"] if c["source"] == relative), key=lambda c: c["start"]):
            if chunk["start"] != position or chunk["name"] not in acknowledged:
                break
            position = chunk["end"]
        prefix[relative] = position
    manifest = {"schema": "p8-diagnostic-byte-checkpoint-v1", "index": index,
                "configuration_sha256": sha(directory / "configuration.json"),
                "all_captured_chunks": state["chunks"], "included_chunk_names": [c["name"] for c in pending],
                "acknowledged_uploads_before_this_upload": state["uploads"],
                "acknowledged_prefix_bytes_before_this_upload": prefix,
                "metadata_snapshots": metadata, "capture_faults": faults,
                "supervisor_terminal_observed": (directory / "supervisor-terminal.json").is_file(),
                "prefix_may_end_inside_JSON_record": True, "native_EOF_claimed": False,
                "lost_runner_tail": "bytes after last successful acknowledged artifact are unknown, not zero",
                "diagnostic_only": True}
    write_new(bundle / "manifest.json", manifest)
    save_state(state_path, state)
    return bundle, manifest


def wait_until(directory, target):
    directory = Path(directory)
    configuration = read_json(directory / "configuration.json")
    limit = configuration["started_monotonic"] + configuration["outer_job_seconds"]
    while not (directory / "supervisor-terminal.json").exists() and time.monotonic() < min(target, limit):
        time.sleep(min(1, max(0, min(target, limit) - time.monotonic())))
    return (directory / "supervisor-terminal.json").is_file()


def checkpoint(directory, index, previous_id="", previous_digest="", final=False):
    configuration = read_json(Path(directory) / "configuration.json")
    require(0 <= index <= CHECKPOINTS, "checkpoint outside finite schedule")
    target = configuration["started_monotonic"] + (configuration["outer_job_seconds"] if final else (index + 1) * INTERVAL_SECONDS)
    terminal = wait_until(directory, target)
    if final:
        observer_after(directory)
    bundle, manifest = snapshot(directory, index, previous_id, previous_digest)
    emit_outputs(ready="true", running="false" if terminal else "true", bundle=str(bundle))
    return manifest


def verdict(directory):
    directory = Path(directory)
    if (directory / "observer-after-error.json").exists():
        return 1
    before, after = directory / "observer-before.json", directory / "observer-after.json"
    if not before.is_file() or not after.is_file() or read_json(before) != read_json(after):
        return 1
    path = Path(directory) / "supervisor-terminal.json"
    if not path.is_file():
        return 1
    terminal = read_json(path)
    if terminal.get("wrapper_timeout") or terminal.get("wrapper_error") or terminal.get("driver_returncode") != 0:
        code = terminal.get("driver_returncode")
        return code if isinstance(code, int) and 0 < code < 126 else 1
    # A live supervisor, exit-0 wrapper, or raw prefix alone is never native success.
    shard = Path(directory) / "shard/shard.json"
    if not shard.is_file():
        return 1
    original = read_json(shard)
    return 0 if (original.get("status") == "passed" and original.get("exit_code") == 0
                 and terminal.get("native_report_status") == "measurement_complete"
                 and terminal.get("native_exit_code") == 0) else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["launch", "supervise", "checkpoint", "finish", "verdict"])
    parser.add_argument("--directory", required=True)
    parser.add_argument("--root")
    parser.add_argument("--build")
    parser.add_argument("--expected-source")
    parser.add_argument("--index", type=int, default=0)
    args = parser.parse_args()
    if args.mode == "launch":
        launch(args.root, args.build, args.directory, args.expected_source)
    elif args.mode == "supervise":
        supervise(args.directory, read_json(Path(args.directory) / "configuration.json")["command"])
    elif args.mode in ("checkpoint", "finish"):
        checkpoint(args.directory, args.index, os.environ.get("PREVIOUS_ARTIFACT_ID", ""),
                   os.environ.get("PREVIOUS_ARTIFACT_DIGEST", ""), final=args.mode == "finish")
    else:
        return verdict(args.directory)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
