#!/usr/bin/env python3
"""Continue only Clippy and DB21 with an explicit installed SDK, preserving the failed first execution."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root")
WORKTREE = ROOT / "worktree"
EXPECTED_HEAD = "ae033369f015c416cd7995709c1853497773cf6a"
EXPECTED_TREE = "1b393dc8582e0ad5193c671124bea94beb38e061"
CARGO = "/Users/jin/.cargo/bin/cargo"
COMMANDS = [
    ("clippy", [CARGO, "+1.95.0", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"], 2400),
    ("direct-writer", [CARGO, "+1.95.0", "test", "--locked", "-p", "cc-db", "--lib", "direct_writer::tests::", "--", "--nocapture"], 1200),
    ("index-migrate", [CARGO, "+1.95.0", "test", "--locked", "-p", "cc-db", "--lib", "index_migrate::tests::", "--", "--nocapture"], 1200),
    ("schema-guard", [CARGO, "+1.95.0", "test", "--locked", "-p", "cc-db", "--test", "ci_schema_guard_contract", "--", "--nocapture"], 1200),
]
def stamp():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()
def digest(path):
    raw = path.read_bytes()
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}
def git(args):
    return subprocess.check_output(["git", *args], cwd=WORKTREE).decode("utf-8")
def source_state():
    head, tree = git(["rev-parse", "HEAD", "HEAD^{tree}"]).splitlines()
    status = git(["status", "--porcelain=v1", "--untracked-files=normal"])
    if head != EXPECTED_HEAD or tree != EXPECTED_TREE or status:
        raise RuntimeError("exact-G worktree is not clean: " + json.dumps({"head":head,"tree":tree,"status":status}))
    return {"head": head, "tree": tree, "status": status}
def main():
    results = ROOT / "validation-sdk26-results"
    results.mkdir(exist_ok=False)
    env = dict(os.environ)
    env.update({"PATH": "/Users/jin/.cargo/bin:" + env.get("PATH", ""), "CARGO_TARGET_DIR": str(ROOT / "cargo-target"), "CARGO_TERM_COLOR": "never", "CARGO_BUILD_JOBS": "4", "RUSTFLAGS": "-D warnings", "PYTHONDONTWRITEBYTECODE": "1", "SDKROOT": "/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk"})
    baseline = source_state()
    prior_path = ROOT / "validation-results" / "manifest.json"
    prior = json.loads(prior_path.read_text(encoding="utf-8"))
    if prior["source"] != baseline or prior["status"] != "failed":
        raise RuntimeError("unexpected prior source or terminal status")
    if [(x["name"], x["status"], x["exit_code"]) for x in prior["commands"]] != [("source-guard", "passed", 0), ("historical-integrations", "passed", 0), ("fmt", "passed", 0), ("clippy", "failed", 101)]:
        raise RuntimeError("unexpected prior command sequence")
    for previous in prior["commands"]:
        if previous["source_before"] != baseline or previous["source_after"] != baseline or not previous["source_unchanged"]:
            raise RuntimeError("prior source changed")
        for stream in ["stdout", "stderr"]:
            if digest(prior_path.parent / (previous["name"] + "." + stream + ".log")) != previous[stream]:
                raise RuntimeError("prior raw stream changed")
    sdk_file = Path(env["SDKROOT"]) / "usr/lib/libSystem.B.tbd"
    if not sdk_file.is_file() or "arm64e.x1" in sdk_file.read_text(encoding="utf-8"):
        raise RuntimeError("selected SDK is absent or still has unsupported arm64e.x1")
    versions = {}
    for key, argv in [
        ("python", ["/usr/bin/python3", "--version"]),
        ("cargo", [CARGO, "+1.95.0", "--version"]),
        ("rustc", ["/Users/jin/.cargo/bin/rustc", "+1.95.0", "-vV"]),
        ("rustfmt", ["/Users/jin/.cargo/bin/rustfmt", "+1.95.0", "--version"]),
        ("clippy", [CARGO, "+1.95.0", "clippy", "--version"]),
        ("git", ["git", "--version"]),
        ("clang", ["/Library/Developer/CommandLineTools/usr/bin/clang", "--version"]),
        ("ld", ["/Library/Developer/CommandLineTools/usr/bin/ld", "-version_details"]),
    ]:
        done = subprocess.run(argv, cwd=WORKTREE, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
        versions[key] = {"argv": argv, "exit_code": done.returncode, "stdout": done.stdout.decode("utf-8"), "stderr": done.stderr.decode("utf-8")}
        if done.returncode:
            raise RuntimeError("tool version failed: " + key)
    manifest = {"schema": "p8-pr184-native-sdk-continuation-v1", "source": baseline, "platform": sys.platform, "versions": versions, "runner": digest(Path(__file__)), "started_at_utc": stamp(), "commands": []}
    manifest.update({"prior_execution_manifest": {"path": str(prior_path), **digest(prior_path)}, "prior_status_preserved": "failed", "reused_successful_commands": prior["commands"][:3], "sdk": {"path": env["SDKROOT"], "libSystem_tbd": digest(sdk_file)}, "environment_overrides": {key: env[key] for key in ["PATH", "CARGO_TARGET_DIR", "CARGO_TERM_COLOR", "CARGO_BUILD_JOBS", "RUSTFLAGS", "PYTHONDONTWRITEBYTECODE", "SDKROOT"]}, "reason": "The first Clippy attempt failed while linking dependency build scripts with the default MacOSX27.0 SDK; only the remaining original commands are continued with the installed 26.5 SDK. No global developer setting or source file is changed."})
    def save():
        (results / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    save()
    for name, argv, timeout in COMMANDS:
        before = source_state()
        entry = {"name": name, "argv": argv, "cwd": str(WORKTREE), "timeout_seconds": timeout, "source_before": before, "started_at_utc": stamp(), "status": "running"}
        manifest["commands"].append(entry)
        save()
        print(json.dumps({"command_started": name, "argv": argv, "source": before}), flush=True)
        stdout_path, stderr_path = results / (name + ".stdout.log"), results / (name + ".stderr.log")
        started = time.monotonic()
        with stdout_path.open("xb") as stdout, stderr_path.open("xb") as stderr:
            try:
                done = subprocess.run(argv, cwd=WORKTREE, env=env, stdout=stdout, stderr=stderr, timeout=timeout)
                entry.update({"exit_code": done.returncode, "timed_out": False})
            except subprocess.TimeoutExpired:
                entry.update({"exit_code": None, "timed_out": True})
        entry.update({"completed_at_utc": stamp(), "elapsed_seconds": time.monotonic() - started, "stdout": digest(stdout_path), "stderr": digest(stderr_path)})
        try:
            entry["source_after"] = source_state()
            entry["source_unchanged"] = entry["source_after"] == before
        except Exception as exc:
            entry["source_unchanged"] = False
            entry["source_error"] = str(exc)
        entry["status"] = "passed" if entry["exit_code"] == 0 and not entry["timed_out"] and entry["source_unchanged"] else "failed"
        save()
        print(json.dumps({"command_completed": entry}), flush=True)
        if entry["status"] != "passed":
            manifest.update({"status": "failed", "completed_at_utc": stamp(), "remaining_commands_not_run": [x[0] for x in COMMANDS[len(manifest["commands"]):]]})
            save()
            return 1
    manifest.update({"status": "passed", "completed_at_utc": stamp(), "source_after": source_state(), "all_four_continuation_commands_executed": True})
    save()
    print(json.dumps({"validation_finished": manifest}), flush=True)
    return 0
if __name__ == "__main__":
    sys.exit(main())
