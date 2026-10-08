#!/usr/bin/env python3
"""Ordinary Actions orchestration for a separately fixed P8 release candidate.

This utility driver is not the candidate source and does not edit its checkout.
All benchmark protocol budgets stay in the fixed original input template.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time

RECOVERY = "artifacts/benchmarks/p8-external-recovery-20261008/revision-2/recover_external.py"
BUILD_FILES = ("build-receipt.json", "build-failure.json", "source-inputs.json",
               "cargo-build.jsonl", "cargo-build.stderr.log")
SCOPED_EXTRA = (RECOVERY, "artifacts/benchmarks/p8-external-recovery-20261008/manifest.json")
EXPECTED_TESTS = {"wrapper": 39, "profile": 4, "scanner": 5, "watcher": 2, "readiness-unit": 5, "readiness-real": 2}

def need(value, label):
    if not value:
        raise ValueError(label)

def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()

def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x") as stream:
        json.dump(value, stream, ensure_ascii=True, sort_keys=True, indent=2)
        stream.write("\n")

def git(root, *args):
    return subprocess.check_output(["git", "-C", str(root), *args],
                                   stdin=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=120)

def clean(root, expected):
    need(git(root, "rev-parse", "HEAD").decode().strip() == expected, "candidate HEAD")
    need(not git(root, "status", "--porcelain", "--untracked-files=all").strip(), "dirty candidate")
    need(not git(root, "submodule", "status", "--recursive").strip(), "unexpected submodules")

def command(public, label, argv, root, environment, timeout, accepted=(0,)):
    begin = time.time()
    raw = public / "commands" / (label + ".log")
    raw.parent.mkdir(parents=True, exist_ok=True)
    timed_out = False
    with raw.open("xb") as output:
        with subprocess.Popen([str(a) for a in argv], cwd=root, env=environment,
                              stdin=subprocess.DEVNULL, stdout=output, stderr=subprocess.STDOUT,
                              start_new_session=True) as process:
            try:
                code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                code = process.wait()
    result = {"argv": [str(a) for a in argv], "cwd": str(root),
              "started_unix": begin, "finished_unix": time.time(), "process_exit_code": code,
              "timed_out": timed_out, "timeout_seconds": timeout,
              "stdout_stderr_sha256": sha(raw), "stdout_stderr_bytes": raw.stat().st_size}
    write(public / "commands" / (label + ".json"), result)
    print(json.dumps({"step": label, "process_exit_code": code, "timed_out": timed_out}), flush=True)
    need(code in accepted and not timed_out, label + " failed; original command log retained")
    return result, raw

def require_python_tests(raw, count):
    text = raw.read_text(errors="replace")
    found = re.findall(r"Ran (\d+) tests? in [^\r\n]+", text)
    need(found == [str(count)] and len(re.findall(r"(?m)^OK[ \t]*\r?$", text)) == 1,
         "actual Python test inventory/verdict differs")

def require_rust_tests(raw, count):
    text = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", raw.read_text(errors="replace"))
    found = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", text)
    need(found == [(str(count), "0", "0")], "actual Rust test inventory/verdict differs")

def preserve_builds(builds, public):
    for role in ("product", "runner"):
        root = builds / role
        if not root.is_dir():
            continue
        for name in BUILD_FILES:
            source = root / name
            if source.is_file() and not source.is_symlink():
                target = public / "build-evidence" / role / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
                need(sha(source) == sha(target), "build evidence copy drift")
        inv = root / "invocation"
        if inv.is_dir():
            for name in ("execution.json", "stdout.log", "stderr.log"):
                source = inv / name
                if source.is_file() and not source.is_symlink():
                    target = public / "build-evidence" / role / "invocation" / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(source, target)
                    need(sha(source) == sha(target), "runner invocation copy drift")

def run(args):
    root = args.source_root.resolve(strict=True)
    need(re.fullmatch(r"[0-9a-f]{40}", args.expected_source) is not None, "full candidate source required")
    clean(root, args.expected_source)
    output = args.output_root.absolute()
    need(not output.exists() and not output.is_symlink(), "new owned output required")
    need(not (output == root or output.is_relative_to(root) or root.is_relative_to(output)),
         "output and candidate must be separate")
    output.mkdir(parents=True)
    public = output / "public"
    public.mkdir()
    builds = output / "builds"
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", GIT_TERMINAL_PROMPT="0",
                       GIT_NO_LAZY_FETCH="1", CODECORTEX_BENCH_PROCESS_PROBE="0")
    result = {"schema_version": 1, "started_at_utc": datetime.now(timezone.utc).isoformat(),
              "expected_source": args.expected_source, "utility_driver_sha256": sha(Path(__file__)),
              "candidate_root": str(root), "status": "started", "steps": {},
              "ranking": "not_run", "P8_003_done": False, "release_certified": False}
    code = 2
    try:
        source_tree = git(root, "rev-parse", "HEAD^{tree}").decode().strip()
        extra = {}
        for name in SCOPED_EXTRA:
            current = (root / name).read_bytes()
            need(current == git(root, "show", args.expected_source + ":" + name), "extra validation source drift")
            extra[name] = {"bytes": len(current), "sha256": hashlib.sha256(current).hexdigest()}
        write(public / "source-binding.json", {"source_commit": args.expected_source, "source_tree": source_tree,
              "extra_validation_domain": extra, "candidate_git_status": "",
              "driver": {"path": str(Path(__file__).resolve()), "sha256": sha(Path(__file__))},
              "workflow_run_id": os.environ.get("GITHUB_RUN_ID"), "workflow_sha": os.environ.get("GITHUB_SHA"),
              "toolchain_selection": os.environ.get("RUSTUP_TOOLCHAIN")})
        item, _ = command(public, "rustfmt-check", ["cargo", "fmt", "--all", "--", "--check"],
                          root, environment, 120)
        result["steps"]["rustfmt-check"] = item
        for label, pattern in (("wrapper", "test_p8_compat.py"), ("profile", "test_p7_stdio_build_profile.py")):
            item, raw = command(public, label,
                [sys.executable, "-m", "unittest", "discover", "-s", "scripts/tests", "-p", pattern, "-v"],
                root, environment, 180)
            require_python_tests(raw, EXPECTED_TESTS[label])
            result["steps"][label] = item
        item, _ = command(public, "build-product",
            [sys.executable, "scripts/p7_stdio_build_receipt.py", "--package-kind", "default",
             "--output-dir", builds / "product", "--release"], root, environment, 3600)
        result["steps"]["build-product"] = item
        target = builds / "product" / "cargo-target"
        item, _ = command(public, "build-runner",
            [sys.executable, "scripts/p8_candidate_execution.py", "build-runner",
             "--output", builds / "runner", "--target-dir", target, "--release"], root, environment, 1900)
        result["steps"]["build-runner"] = item
        test_environment = dict(environment, CARGO_TARGET_DIR=str(target), CARGO_BUILD_BUILD_DIR=str(target),
                                CODECORTEX_BENCH_BINARY=str(builds / "product" / "codecortex"))
        tests = [
            ("scanner", ["-p", "cc-index", "--test", "explicit_text_admission"], []),
            ("watcher", ["-p", "cc-server", "--lib", "watcher::explicit_text_admission_tests"], []),
            ("readiness-unit", ["-p", "cc-eval", "--lib", "inventory_tests"], []),
            ("readiness-real", ["-p", "cc-eval", "--test", "p8_mcp_readiness"], ["--ignored"]),
        ]
        for label, selectors, harness in tests:
            item, raw = command(public, label,
                ["cargo", "test", "--locked", "--no-default-features", "--release", *selectors,
                 "--", *harness, "--test-threads=1"], root, test_environment, 1800)
            require_rust_tests(raw, EXPECTED_TESTS[label])
            result["steps"][label] = item
        clean(root, args.expected_source)
        item, _ = command(public, "package-candidate",
            [sys.executable, "scripts/p8_external_candidate.py", "--source-root", root,
             "--expected-source", args.expected_source, "--product-build", builds / "product",
             "--runner-build", builds / "runner", "--output", public / "candidate-package"],
            root, environment, 300)
        result["steps"]["package-candidate"] = item
        package = public / "candidate-package"
        manifest = package / "manifest.json"
        package_receipt = json.loads((package / "package-receipt.json").read_bytes())
        need(package_receipt["manifest"]["sha256"] == sha(manifest), "manifest differs from fresh receipt")
        result["ranking"] = "attempted; read external/result.json for actual profile outcomes"
        item, _ = command(public, "external-revision2",
            [sys.executable, RECOVERY, "--manifest", manifest, "--manifest-sha256", sha(manifest),
             "--expected-source", args.expected_source, "--archive", package / "candidate-tools.zip",
             "--work", output / "private-external", "--public-output", public / "external"],
            root, environment, 12000, accepted=(0, 1, 2, 3))
        result["steps"]["external-revision2"] = item
        external = json.loads((public / "external" / "result.json").read_bytes())
        need(external["exit_code"] == item["process_exit_code"], "external process/receipt exit mismatch")
        if external["exit_code"] == 0:
            need(external["phase"] == "measured_and_packaged", "external execution not complete")
            need(external["ranking"]["attempted_profiles"] == 4
                 and external["ranking"]["verified_complete_profiles"] == 4
                 and external["ranking"]["observed_measured_rows"] == 1200
                 and external["ranking"]["not_run_profiles"] == 0,
                 "actual external profile/row inventory differs")
            result["status"] = "actual_release_external_engineering_run_complete"
        else:
            result["status"] = "external_" + external["phase"]
        result["ranking"] = external["ranking"]
        clean(root, args.expected_source)
        code = external["exit_code"]
    except Exception as exc:
        result.update(status="failed", exception=type(exc).__name__, reason=str(exc))
    finally:
        try:
            preserve_builds(builds, public)
        except Exception as exc:
            result.update(status="failed", evidence_preservation_error=type(exc).__name__ + ": " + str(exc))
            code = 2
        external_result = public / "external" / "result.json"
        if external_result.is_file():
            external = json.loads(external_result.read_bytes())
            result["actual_external_result"] = {
                key: external.get(key) for key in ("phase", "exit_code", "ranking", "run_exit_codes",
                                                   "measurement_observations", "raw_package")}
        result.update(exit_code=code, finished_at_utc=datetime.now(timezone.utc).isoformat())
        write(public / "driver-result.json", result)
    print(json.dumps({"status": result["status"], "exit_code": code,
                      "public_output": str(public), "P8_003_done": False}), flush=True)
    return code

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--expected-source", required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    return run(parser.parse_args())

if __name__ == "__main__":
    raise SystemExit(main())
