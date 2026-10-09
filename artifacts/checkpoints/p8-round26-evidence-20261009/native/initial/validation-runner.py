#!/usr/bin/env python3
"""Run fixed PR184 validation commands in the owned exact-G worktree, preserving full streams."""
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
    ("source-guard", ["/usr/bin/python3", "scripts/verify_reviewed_source_v15.py", "--source-version", "p8-completion-source-20261009-v15"], 1200),
    ("historical-integrations", ["/usr/bin/python3", "scripts/verify_historical_integrations_v2.py"], 1200),
    ("fmt", [CARGO, "+1.95.0", "fmt", "--all", "--", "--check"], 300),
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
    results = ROOT / "validation-results"
    results.mkdir(exist_ok=False)
    env = dict(os.environ)
    env.update({"PATH": "/Users/jin/.cargo/bin:" + env.get("PATH", ""), "CARGO_TARGET_DIR": str(ROOT / "cargo-target"), "CARGO_TERM_COLOR": "never", "CARGO_BUILD_JOBS": "4", "RUSTFLAGS": "-D warnings", "PYTHONDONTWRITEBYTECODE": "1"})
    baseline = source_state()
    versions = {}
    for key, argv in [
        ("python", ["/usr/bin/python3", "--version"]),
        ("cargo", [CARGO, "+1.95.0", "--version"]),
        ("rustc", ["/Users/jin/.cargo/bin/rustc", "+1.95.0", "-vV"]),
        ("rustfmt", ["/Users/jin/.cargo/bin/rustfmt", "+1.95.0", "--version"]),
        ("clippy", [CARGO, "+1.95.0", "clippy", "--version"]),
        ("git", ["git", "--version"]),
    ]:
        done = subprocess.run(argv, cwd=WORKTREE, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
        versions[key] = {"argv": argv, "exit_code": done.returncode, "stdout": done.stdout.decode("utf-8"), "stderr": done.stderr.decode("utf-8")}
        if done.returncode:
            raise RuntimeError("tool version failed: " + key)
    manifest = {"schema": "p8-pr184-native-validation-v1", "source": baseline, "platform": sys.platform, "versions": versions, "runner": digest(Path(__file__)), "started_at_utc": stamp(), "commands": []}
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
    manifest.update({"status": "passed", "completed_at_utc": stamp(), "source_after": source_state(), "all_seven_commands_executed": True})
    save()
    print(json.dumps({"validation_finished": manifest}), flush=True)
    return 0
if __name__ == "__main__":
    sys.exit(main())
