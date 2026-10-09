#!/usr/bin/env python3
"""Format fixed candidate byte copies only. No Cargo, tests or product execution."""
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import time

PREFIX = "artifacts/checkpoints/p8-candidate-rustfmt-20261009"
WORKFLOW = ".github/workflows/p8-candidate-rustfmt.yml"
BRANCH = "refs/heads/task/p8-candidate-rustfmt-20261009"
BASE = "94f6e1c36583bfed1d3b8e3c76591bc2a0b26ee5"
SCHEMA = "p8-candidate-rustfmt-frame-v1"
NAMES = ("formatted-sources.json", "commands.json", "stderr.log",
         "source-before.json", "source-after.json", "receipt.json")
MAX_FILE = 4 * 1024 * 1024
MAX_TOTAL = 12 * 1024 * 1024

def require(ok, message):
    if not ok:
        raise ValueError(message)

def stamp():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def identity(raw):
    return {"bytes": len(raw), "sha256": sha(raw),
            "blob": hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()}

def save(path, value):
    raw = (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
    with path.open("xb") as handle:
        handle.write(raw)

def clean_environment():
    # A formatter process never receives GitHub tokens, step credentials,
    # arbitrary Rust flags, or loader hooks.
    keys = ("PATH", "HOME", "RUSTUP_HOME", "CARGO_HOME", "LANG", "LC_ALL")
    return {key: os.environ[key] for key in keys if key in os.environ}

def git(root, *args):
    result = subprocess.run(["git", "--no-optional-locks", "-C", str(root), *args],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            env=clean_environment(), timeout=30, check=False)
    require(result.returncode == 0, "read-only Git inspection failed")
    return result.stdout

def snapshot(root, plan, bundle, candidates):
    head = os.environ["GITHUB_SHA"]
    require(git(root, "rev-parse", "HEAD").decode().strip() == head, "checkout identity changed")
    require(git(root, "rev-parse", "HEAD^").decode().strip() == BASE, "unexpected parent")
    require(not git(root, "status", "--porcelain", "--untracked-files=no"), "tracked checkout changed")
    files = {}
    for relative in plan["controller_paths"]:
        path = root / relative
        require(path.resolve(strict=True) == path and path.is_file(), "controller path differs")
        raw = path.read_bytes()
        require(raw == git(root, "show", head + ":" + relative), "controller bytes differ from Git")
        files[relative] = identity(raw)
    require(files[PREFIX + "/run.py"] == plan["runner"], "runner differs from fixed plan")
    require(files[PREFIX + "/candidate-sources.json"] == plan["candidate_bundle"],
            "candidate bundle differs from fixed plan")
    copies = {}
    for row in plan["candidates"]:
        relative = row["path"]
        path = candidates / relative
        require(path.resolve(strict=True) == path and path.is_file(), "candidate copy path differs")
        copies[relative] = identity(path.read_bytes())
        require(copies[relative] == {key: row[key] for key in ("bytes", "sha256", "blob")},
                "candidate input copy changed")
        require(identity(bundle[relative].encode()) == copies[relative], "bundle/copy mismatch")
    return {"controller_commit": head, "controller_parent": BASE, "controller_files": files,
            "candidate_inputs": copies, "new_measurements": 0}

def invoke(argv, cwd, commands, stderr_parts, input_bytes=None, timeout=60):
    item = {"argv": [str(arg) for arg in argv], "cwd": str(cwd), "started_utc": stamp(),
            "stdin": None if input_bytes is None else identity(input_bytes)}
    stdout = b""
    stderr = b""
    try:
        result = subprocess.run(item["argv"], input=input_bytes, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, env=clean_environment(),
                                cwd=cwd, timeout=timeout, check=False)
        stdout, stderr = result.stdout, result.stderr
        item["exit_code"] = result.returncode
    except subprocess.TimeoutExpired as error:
        stdout, stderr = error.stdout or b"", error.stderr or b""
        item["exit_code"] = None
        item["error"] = "formatter_command_timeout"
    except Exception as error:
        item["exit_code"] = None
        item["error"] = type(error).__name__ + ": " + str(error)
    item.update({"completed_utc": stamp(), "stdout": identity(stdout), "stderr": identity(stderr),
                 "stdout_base64": base64.b64encode(stdout).decode(),
                 "stderr_base64": base64.b64encode(stderr).decode()})
    commands.append(item)
    stderr_parts.append(("command-" + str(len(commands) - 1) + "\n").encode() + stderr + b"\n")
    return item, stdout

def emit_files(output):
    raws = [(name, (output / name).read_bytes()) for name in NAMES]
    require(all(len(raw) <= MAX_FILE for _, raw in raws), "framed file exceeds 4 MiB")
    require(sum(len(raw) for _, raw in raws) <= MAX_TOTAL, "framed total exceeds 12 MiB")
    for name, raw in raws:
        count = (len(raw) + 1023) // 1024
        common = {"schema": SCHEMA, "name": name}
        def emit(value):
            line = json.dumps(dict(common, **value), sort_keys=True, separators=(",", ":"))
            require(len(line.encode()) <= 2048, "frame exceeds 2 KiB")
            print(line, flush=True)
        emit({"kind": "header", "bytes": len(raw), "sha256": sha(raw),
              "count": count, "raw_chunk_bytes": 1024})
        for index in range(count):
            chunk = raw[index * 1024:(index + 1) * 1024]
            emit({"kind": "chunk", "index": index, "count": count, "bytes": len(chunk),
                  "sha256": sha(chunk), "base64": base64.b64encode(chunk).decode()})
        emit({"kind": "end", "bytes": len(raw), "sha256": sha(raw), "count": count})

def main():
    require(not sys.flags.optimize, "optimized Python forbidden")
    require(os.environ["GITHUB_REPOSITORY"] == "jyqj/codecortex", "repository differs")
    require(os.environ["GITHUB_EVENT_NAME"] == "push" and os.environ["GITHUB_REF"] == BRANCH,
            "wrong isolated formatter trigger")
    require(os.environ["GITHUB_RUN_ATTEMPT"] == "1", "only first attempt allowed")
    run_id = os.environ["GITHUB_RUN_ID"]
    require(run_id.isdecimal(), "invalid run identity")
    root = Path(os.environ["GITHUB_WORKSPACE"]).resolve(strict=True)
    require(Path(__file__).resolve() == root / PREFIX / "run.py", "wrong runner path")
    plan = json.loads((root / PREFIX / "plan.json").read_bytes())
    bundle_document = json.loads((root / PREFIX / "candidate-sources.json").read_bytes())
    require(plan["schema"] == "p8-candidate-rustfmt-plan-v1" and plan["edition"] == "2021"
            and plan["toolchain"] == "1.95.0" and plan["parent"] == BASE, "unexpected formatting plan")
    require(len(plan["candidates"]) == 15, "candidate population differs")
    bundle = bundle_document["sources"]
    expected_paths = [row["path"] for row in plan["candidates"]]
    require(len(set(expected_paths)) == 15 and set(bundle) == set(expected_paths),
            "candidate paths are missing, extra or duplicate")
    owned = Path(os.environ["RUNNER_TEMP"]).resolve(strict=True) / ("p8-candidate-rustfmt-" + run_id)
    owned.mkdir()
    output = owned / "output"
    output.mkdir()
    candidates = owned / "input"
    candidates.mkdir()
    formatted_dir = owned / "formatted"
    formatted_dir.mkdir()
    empty_config = owned / "empty-rustfmt.toml"
    empty_config.write_bytes(b"")
    commands = []
    stderr_parts = []
    before = None
    after = None
    tool = None
    tool_before = None
    success = False
    entries = [{"path": row["path"], "before": {key: row[key] for key in ("bytes", "sha256", "blob")},
                "status": "not_run"} for row in plan["candidates"]]
    receipt = {"schema": "p8-candidate-rustfmt-receipt-v1", "status": "failed",
               "controller_commit": os.environ["GITHUB_SHA"], "run_id": int(run_id), "attempt": 1,
               "started_utc": stamp(), "expected_candidates": 15, "new_measurements": 0,
               "TODO_closed": 0, "TODO_remaining": 29,
               "scope": "candidate-copy formatting only; no compilation, tests, CI certification or product execution"}
    try:
        for row in plan["candidates"]:
            relative = PurePosixPath(row["path"])
            require(str(relative) == row["path"] and relative.parts[0] == "crates"
                    and ".." not in relative.parts and relative.suffix == ".rs",
                    "invalid candidate path")
            raw = bundle[row["path"]].encode()
            require(identity(raw) == {key: row[key] for key in ("bytes", "sha256", "blob")},
                    "candidate body does not match its fixed identity")
            path = candidates / row["path"]
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as handle:
                handle.write(raw)
        before = snapshot(root, plan, bundle, candidates)
        rustup_argv = ["rustup", "which", "--toolchain", "1.95.0", "rustfmt"]
        located, raw_path = invoke(rustup_argv, owned, commands, stderr_parts)
        require(located["exit_code"] == 0, "fixed toolchain rustfmt lookup failed")
        tool = Path(raw_path.decode().strip()).resolve(strict=True)
        require(tool.is_file(), "rustfmt executable missing")
        tool_before = identity(tool.read_bytes())
        version, version_stdout = invoke([tool, "--version"], owned, commands, stderr_parts)
        require(version["exit_code"] == 0 and version_stdout.startswith(b"rustfmt "),
                "rustfmt version observation failed")
        rustc, rustc_stdout = invoke(["rustup", "run", "1.95.0", "rustc", "--version", "--verbose"],
                                    owned, commands, stderr_parts)
        require(rustc["exit_code"] == 0 and b"release: 1.95.0\n" in rustc_stdout,
                "fixed compiler toolchain identity differs")
        receipt["tool"] = {"path": str(tool), "identity_before": tool_before,
                           "rustfmt_version": version_stdout.decode(),
                           "rustc_version_verbose": rustc_stdout.decode(),
                           "empty_config": identity(empty_config.read_bytes()),
                           "edition": "2021", "input_mode": "stdin; no out-of-line module recursion"}
        all_formatted = True
        for row, entry in zip(plan["candidates"], entries, strict=True):
            raw = (candidates / row["path"]).read_bytes()
            argv = [tool, "--edition", "2021", "--emit", "stdout", "--config-path", empty_config]
            command, stdout = invoke(argv, owned, commands, stderr_parts, raw)
            command["candidate_path"] = row["path"]
            entry.update({"command_index": len(commands) - 1, "exit_code": command["exit_code"],
                          "stdout_base64": base64.b64encode(stdout).decode(),
                          "after": identity(stdout)})
            try:
                entry["content"] = stdout.decode("utf-8")
            except UnicodeDecodeError:
                entry["content"] = None
            okay = command["exit_code"] == 0 and entry["content"] is not None and bool(stdout)
            entry["status"] = "formatted" if okay else "formatter_failed"
            all_formatted = all_formatted and okay
            destination = formatted_dir / row["path"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            with destination.open("xb") as handle:
                handle.write(stdout)
        success = all_formatted
    except BaseException as error:
        receipt["error"] = type(error).__name__ + ": " + str(error)
    finally:
        try:
            after = snapshot(root, plan, bundle, candidates)
            require(before == after, "source inputs or controller changed")
            require(empty_config.read_bytes() == b"", "formatter configuration changed")
            if tool_before is not None:
                require(identity(tool.read_bytes()) == tool_before, "formatter executable changed")
            receipt["source_inputs_and_controller_unchanged"] = True
        except BaseException as error:
            success = False
            receipt["source_after_error"] = type(error).__name__ + ": " + str(error)
            receipt["source_inputs_and_controller_unchanged"] = False
        save(output / "formatted-sources.json", {"schema": "p8-formatted-candidate-sources-v1",
                                                 "files": entries, "semantics_executed": False})
        save(output / "commands.json", {"commands": commands, "environment": "explicit token-free allowlist"})
        with (output / "stderr.log").open("xb") as handle:
            handle.write(b"".join(stderr_parts))
        save(output / "source-before.json", before)
        save(output / "source-after.json", after)
        receipt["formatted_count"] = sum(entry["status"] == "formatted" for entry in entries)
        receipt["failed_or_not_run_count"] = 15 - receipt["formatted_count"]
        receipt["status"] = "passed" if success and receipt["formatted_count"] == 15 else "failed"
        receipt["completed_utc"] = stamp()
        receipt["files_before_receipt"] = {name: identity((output / name).read_bytes()) for name in NAMES[:-1]}
        save(output / "receipt.json", receipt)
    emit_files(output)
    return 0 if receipt["status"] == "passed" else 2

if __name__ == "__main__":
    raise SystemExit(main())
