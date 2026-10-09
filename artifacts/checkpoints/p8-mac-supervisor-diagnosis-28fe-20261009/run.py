#!/usr/bin/env python3
"""Prepare/run two fixed causal probes; never execute an indexing/test binary.

This is not the original run_supervised regression, a performance sample, or a
quality waiver. The original helper is case 1; case 2 adds only builtin printf.
No retry, ready handshake, deadline change, Cargo invocation, or source write.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import stat
import subprocess
import sys
import threading
import time

HEAD = "a23bb72d3c954f385b99fe81ce9189885c208557"
TREE = "58147c952505c44da1f41eb4b9c31643f2303b96"
SOURCES = {
    "crates/cc-eval/src/benchmark/p8_scale.rs": {
        "bytes": 46139, "git_blob": "3f217d116e5fab8a1035cb643f0d74ea3c122568",
        "sha256": "fede948c535ca8b91a6b30d3752cd28204abc310423debfd169ff53541f3afbc"},
    "crates/cc-eval/tests/p8_scale.rs": {
        "bytes": 19815, "git_blob": "e4369e275bee0ddbc1cbea91e61bcc5735483e4e",
        "sha256": "e178777707b978b04918b8de38ce1beaca76f2bf0fb342928fca0f97657e0c6d"},
}
HELPER_SHA = "0e366f70d7deb927f196f6a0aeaedf6eda8665ff0a141df03148fab81fba31ae"
MAX_LOG = 64 * 1024
SAFE_ENV = ("PATH", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "RUSTUP_HOME", "CARGO_HOME", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET", "TMPDIR", "TMP", "TEMP")
IMAGE_KEYS = ("ImageOS", "ImageVersion", "RUNNER_OS", "RUNNER_ARCH")
REMOVED = ["CODECORTEX_CACHE_DIR", "CODECORTEX_DIRTY_PROPAGATION", "CODECORTEX_DIRTY_PROPAGATION_MAX_FILES", "CODECORTEX_MEMORY_BUDGET_FRACTION", "CODECORTEX_MAX_CONCURRENT_PARSE", "CODECORTEX_USE_DIRECT_WRITER"]


def now():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def identity(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
            "git_blob": hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()}


def new_bytes(path, data, mode=0o600):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    with os.fdopen(fd, "wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def new_json(path, value):
    new_bytes(path, (json.dumps(value, indent=2, sort_keys=True) + "\n").encode())


def error_record(error):
    # Avoid uncontrolled exceptions echoing inherited environment or URLs.
    return {"class": type(error).__name__, "errno": getattr(error, "errno", None)}


class BoundedPipe:
    def __init__(self, pipe):
        self.pipe = pipe
        self.data = bytearray()
        self.overflow = False
        self.error = None
        self.eof = False
        self.frozen = False
        self.lock = threading.Lock()
        self.thread = threading.Thread(target=self.read, daemon=True)
        self.thread.start()

    def read(self):
        try:
            while True:
                block = os.read(self.pipe.fileno(), 8192)
                with self.lock:
                    if self.frozen:
                        return
                    if not block:
                        self.eof = True
                        return
                    keep = min(len(block), MAX_LOG - len(self.data))
                    self.data.extend(block[:keep])
                    self.overflow |= keep < len(block)
        except BaseException as error:
            with self.lock:
                if not self.frozen:
                    self.error = error_record(error)

    def finish(self):
        # Never join an unfinished thread; it cannot mutate saved raw logs.
        if not self.thread.is_alive():
            self.thread.join()
        with self.lock:
            self.frozen = True
            return bytes(self.data), {"bytes_kept": len(self.data), "eof": self.eof,
                                      "overflow": self.overflow, "error": self.error,
                                      "reader_alive_at_freeze": self.thread.is_alive()}


def signal_group(pid):
    try:
        os.killpg(pid, signal.SIGKILL)
        return {"pgid": pid, "signal": "SIGKILL", "return": 0}
    except OSError as error:
        return {"pgid": pid, "signal": "SIGKILL", "error": error_record(error)}


def cleanup_worker(owner_path, observer_pid, case):
    """Only a record emitted by this running observer identifies an owned group."""
    if owner_path is None or not owner_path.is_file():
        return {"state": "unknown", "reason": "spawn ownership record unavailable"}
    try:
        raw = owner_path.read_bytes()
        if len(raw) > 4096:
            raise ValueError("oversized ownership record")
        record = json.loads(raw)
        pid = record.get("worker_pid")
        if (record.get("observer_pid") != observer_pid or record.get("case") != case
                or record.get("fresh_process_group_requested") is not True
                or type(pid) is not int or pid <= 1 or pid == observer_pid):
            raise ValueError("ownership mismatch")
        observed_pgid = record.get("observed_pgid")
        if (type(observed_pgid) is not int or not (
                observed_pgid == pid or (observed_pgid == -1 and record.get("getpgid_errno") == 3))):
            # Explicit contradictory PGID evidence must not authorize a signal.
            raise ValueError("observed process-group identity mismatch")
        return {"state": "signal_attempted_not_proof_of_reaping", "ownership_record": identity(raw),
                "result": signal_group(pid)}
    except BaseException as error:
        return {"state": "unknown", "error": error_record(error)}


def command(argv, directory, label, env, timeout, cwd=None, case=None):
    """Bound each stream, retain nonzero and timeout, never retry.

    Case containment signals at 1.8s to reserve 0.2s inside the original 2s
    assertion. Rust's measurement clock remains 250ms. A stuck OS spawn cannot
    be asserted to have returned on time; elapsed/unknown remain explicit.
    """
    start = time.monotonic()
    record = {"argv": [str(x) for x in argv], "started_at": now(), "timeout_seconds": timeout,
              "status": "starting", "exit_code": None, "case": case}
    new_json(directory / (label + ".start.json"), record)
    process = None
    readers = []
    raw = [b"", b""]
    try:
        process = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        record["pid"] = process.pid
        record["spawn_returned_after_seconds"] = time.monotonic() - start
        readers = [BoundedPipe(process.stdout), BoundedPipe(process.stderr)]
        signal_at = timeout - (0.2 if case else 0.5)
        while process.poll() is None:
            if any(reader.overflow for reader in readers):
                record["status"] = "output_budget_exhausted"
                break
            if time.monotonic() - start >= signal_at:
                record["status"] = "outer_containment_timeout"
                break
            time.sleep(0.005)
        if process.poll() is None:
            if case:
                record["worker_cleanup"] = cleanup_worker(directory / "run" / "owned-child.json", process.pid, case)
            record["observer_group_cleanup"] = signal_group(process.pid)
            remaining = max(0.0, timeout - (time.monotonic() - start))
            try:
                process.wait(timeout=remaining)
            except subprocess.TimeoutExpired:
                record["reaping"] = "unknown_at_outer_deadline"
        record["exit_code"] = process.poll()
        # Wait for local pipe EOF only within the same outer budget.
        while any(reader.thread.is_alive() for reader in readers) and time.monotonic() - start < timeout:
            time.sleep(0.001)
        if record["status"] == "starting":
            record["status"] = "returned" if process.returncode is not None else "unresolved"
    except BaseException as error:
        record["status"] = "command_error"
        record["error"] = error_record(error)
    finally:
        if process is not None and process.poll() is None:
            if case and "worker_cleanup" not in record:
                record["worker_cleanup"] = cleanup_worker(directory / "run" / "owned-child.json", process.pid, case)
            if "observer_group_cleanup" not in record:
                record["observer_group_cleanup"] = signal_group(process.pid)
        record["streams"] = []
        for i, reader in enumerate(readers):
            raw[i], stream_record = reader.finish()
            record["streams"].append(stream_record)
        for suffix, data in zip(("stdout", "stderr"), raw):
            new_bytes(directory / (label + "." + suffix), data)
        record["elapsed_seconds"] = time.monotonic() - start
        record["within_outer_bound"] = record["elapsed_seconds"] < timeout
        record["finished_at"] = now()
        record["complete"] = (record["status"] == "returned" and record["exit_code"] is not None
                              and record["within_outer_bound"] and len(record["streams"]) == 2
                              and all(s["eof"] and not s["overflow"] and s["error"] is None for s in record["streams"]))
        new_json(directory / (label + ".receipt.json"), record)
    return record, raw


def checked(argv, directory, label, env, cwd=None, timeout=15):
    record, raw = command(argv, directory, label, env, timeout, cwd)
    if not record["complete"] or record["exit_code"] != 0:
        raise RuntimeError("recorded command did not complete successfully")
    return raw[0]


def snapshot(source, directory, env):
    directory.mkdir()
    result = {"observed_at": now(), "expected_head": HEAD, "expected_tree": TREE, "files": {}, "passed": False}
    try:
        result["head"] = checked(["git", "rev-parse", "HEAD"], directory, "head", env, source).decode().strip()
        result["tree"] = checked(["git", "rev-parse", "HEAD^{tree}"], directory, "tree", env, source).decode().strip()
        status = checked(["git", "status", "--porcelain=v1", "--untracked-files=normal"], directory, "status", env, source)
        result["tracked_and_untracked_clean"] = status == b""
        for i, (path, expected) in enumerate(SOURCES.items()):
            disk = (source / path).read_bytes()
            original = checked(["git", "show", "HEAD:" + path], directory, "source-" + str(i), env, source)
            actual = identity(disk)
            result["files"][path] = {"actual": actual, "expected": expected, "git_bytes_equal": original == disk}
        result["passed"] = (result["head"] == HEAD and result["tree"] == TREE and result["tracked_and_untracked_clean"]
                            and all(v["actual"] == v["expected"] and v["git_bytes_equal"] for v in result["files"].values()))
    except BaseException as error:
        result["error"] = error_record(error)
    new_json(directory / "snapshot.json", result)
    return result


def extract_helper(source):
    raw = (source / "crates/cc-eval/tests/p8_scale.rs").read_text(encoding="utf-8")
    name = "fn subprocess_descendant_cannot_hold_stderr_past_worker_deadline() {"
    if raw.count(name) != 1:
        raise ValueError("function identity")
    body = raw.split(name, 1)[1].split("\n}\n", 1)[0]
    matches = re.findall(r'std::fs::write\(\s*&helper,\s*("(?:[^"\\]|\\.)*")\s*,?\s*\)', body)
    if len(matches) != 1:
        raise ValueError("helper extraction ambiguity")
    # This fixed Rust literal contains only JSON-compatible escapes; no eval.
    original = json.loads(matches[0]).encode("utf-8")
    if len(original) != 159 or hashlib.sha256(original).hexdigest() != HELPER_SHA:
        raise ValueError("original helper bytes mismatch")
    text = original.decode()
    replacements = [
        ("#!/bin/sh\n", "#!/bin/sh\nprintf '%s\\n' 'CCDIAG:parse_before' >&2\n"),
        ("done\nsleep 20 &\n", "done\nprintf '%s\\n' 'CCDIAG:parse_after' >&2\nprintf '%s\\n' 'CCDIAG:sleep_before' >&2\nsleep 20 &\nprintf '%s\\n' 'CCDIAG:sleep_after' >&2\n"),
        ("printf '{\"passed\":true}' > \"$out/worker-summary.json\"\n", "printf '{\"passed\":true}' > \"$out/worker-summary.json\"\nprintf '%s\\n' 'CCDIAG:summary_after' >&2\n"),
    ]
    for before, after in replacements:
        if text.count(before) != 1:
            raise ValueError("instrumentation anchor mismatch")
        text = text.replace(before, after, 1)
    return original, text.encode()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.is_symlink() or not args.output.is_dir() or any(args.output.iterdir()):
        raise ValueError("output must be precreated, empty, nonsymlink directory")
    source, out = args.source.resolve(strict=True), args.output.resolve(strict=True)
    if out == source or source in out.parents:
        raise ValueError("output must be outside original source")
    observer = Path(__file__).resolve().with_name("observe.rs")
    receipt = {
        "schema_version": 1, "status": "starting", "diagnostic_only": True,
        "certification": "not_run", "source": str(source), "output": str(out),
        "started_at": now(), "fixed_case_order": ["original", "instrumented"],
        "case_results": [], "errors": [], "source_expected": {"head": HEAD, "tree": TREE, "files": SOURCES},
        "platform": {"system": platform.system(), "release": platform.release(), "machine": platform.machine(), "python": platform.python_version()},
        "image": {key: os.environ.get(key) for key in IMAGE_KEYS},
        "sdkroot": os.environ.get("SDKROOT"),
        "environment_policy": {"inherited_allowlist": list(SAFE_ENV), "child_overrides": ["RAYON_NUM_THREADS=2", "TMPDIR=owned-tmp", "TMP=owned-tmp", "TEMP=owned-tmp"], "original_removed_keys": REMOVED,
                               "secrets_and_shell_startup_variables": "not inherited; no complete environment is recorded"},
        "limits": {"original_worker_ms": 250, "original_poll_ms": 10, "outer_case_seconds": 2,
                   "outer_signal_at_seconds": 1.8, "streams_max_bytes_each": MAX_LOG},
        "interpretation": ["causal observer is not original run_supervised or original regression acceptance",
                           "original case result is never replaced by instrumented case",
                           "both cases retain before-spawn clock; no ready handshake is excluded",
                           "instrumentation and added observations perturb timing; fixed case order may warm caches",
                           "marker times are pipe receipt upper bounds, not shell execution timestamps",
                           "different host/image/SDK and sanitized inherited environment limit attribution",
                           "a nonreturned spawn or unreaped child remains unknown; timeout is never success",
                           "no indexing worker, performance measurement, Cargo, or existing test binary is executed"],
    }
    # This first immutable receipt exists before any toolchain query or compile.
    new_json(out / "execution-start.json", receipt)
    env = {key: os.environ[key] for key in SAFE_ENV if key in os.environ}
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    env["GIT_OPTIONAL_LOCKS"] = "0"
    source_before = None
    try:
        receipt["controller"] = {"run.py": identity(Path(__file__).read_bytes()), "observe.rs": identity(observer.read_bytes())}
        source_before = snapshot(source, out / "source-before", env)
        receipt["source_before"] = source_before
        if not source_before["passed"]:
            raise RuntimeError("source identity failure")
        original, instrumented = extract_helper(source)
        receipt["helpers"] = {"original": identity(original), "instrumented": identity(instrumented)}
        tools_dir = out / "toolchain"
        tools_dir.mkdir()
        version = checked(["rustc", "+1.95.0", "-Vv"], tools_dir, "rustc-version", env).decode()
        receipt["rustc_version"] = version
        if not re.search(r"^release: 1\.95\.0$", version, re.M):
            raise RuntimeError("toolchain mismatch")
        receipt["rustc_sysroot"] = checked(["rustc", "+1.95.0", "--print", "sysroot"], tools_dir, "rustc-sysroot", env).decode().strip()
        if platform.system() == "Darwin":
            sdk_record, sdk_raw = command(["xcrun", "--show-sdk-path"], tools_dir, "sdk-path", env, 15)
            receipt["sdk_query"] = {"receipt": sdk_record, "stdout": sdk_raw[0].decode("utf-8", "replace")}
        binary = out / "observer"
        compile_record, _ = command(["rustc", "+1.95.0", "--edition=2021", str(observer), "-o", str(binary)], tools_dir, "compile", env, 120)
        receipt["compile"] = compile_record
        if not compile_record["complete"] or compile_record["exit_code"] != 0:
            raise RuntimeError("compile did not complete successfully")
        receipt["observer_binary"] = identity(binary.read_bytes())
        for index, (case, helper_bytes) in enumerate((("original", original), ("instrumented", instrumented)), 1):
            case_dir = out / ("case%02d-%s" % (index, case))
            case_dir.mkdir()
            helper = case_dir / "stderr-holder.sh"
            new_bytes(helper, helper_bytes, 0o700)
            # Case 2 runs once even when case 1 fails. No success replacement.
            record, _ = command([str(binary), str(helper), str(case_dir / "run"), case], case_dir, "observer", env, 2.0, case=case)
            result = {"case": case, "command": record, "helper_before": identity(helper_bytes), "helper_after": identity(helper.read_bytes()), "report": None, "report_error": None}
            report_path = case_dir / "run" / "report.json"
            try:
                report_raw = report_path.read_bytes()
                if len(report_raw) > 128 * 1024:
                    raise ValueError("report oversized")
                result["report_identity"] = identity(report_raw)
                result["report"] = json.loads(report_raw)
            except BaseException as error:
                result["report_error"] = error_record(error)
            report = result["report"] or {}
            result["diagnostic_assertions_satisfied"] = (
                record["complete"] and record["exit_code"] == 0 and record["elapsed_seconds"] < 2
                and report.get("diagnostic_only") is True and report.get("case") == case
                and report.get("exit_code") == 0 and report.get("worker_exit_code") == 0
                and report.get("stderr_complete") is True and report.get("summary_passed_exact_fixture") is True
                and report.get("certification") == "not_run" and result["helper_before"] == result["helper_after"])
            new_json(case_dir / "case-result.json", result)
            receipt["case_results"].append(result)
    except BaseException as error:
        receipt["errors"].append(error_record(error))
    finally:
        try:
            after = snapshot(source, out / "source-after", env)
            receipt["source_after"] = after
            receipt["source_unchanged"] = bool(source_before and source_before["passed"] and after["passed"]
                                              and source_before["files"] == after["files"])
        except BaseException as error:
            receipt["errors"].append(error_record(error))
            receipt["source_unchanged"] = False
        receipt["all_two_diagnostic_assertions_satisfied"] = (
            not receipt["errors"] and receipt["source_unchanged"] and len(receipt["case_results"]) == 2
            and all(r["diagnostic_assertions_satisfied"] for r in receipt["case_results"]))
        receipt["status"] = "diagnostic_completed" if receipt["all_two_diagnostic_assertions_satisfied"] else "diagnostic_failed"
        receipt["exit_code"] = 0 if receipt["all_two_diagnostic_assertions_satisfied"] else 2
        receipt["finished_at"] = now()
        new_json(out / "execution-receipt.json", receipt)
        files = []
        for path in sorted(out.rglob("*")):
            if path.is_file() and not path.is_symlink():
                info = path.stat()
                files.append({"path": path.relative_to(out).as_posix(), "mode": stat.S_IMODE(info.st_mode), **identity(path.read_bytes())})
        new_json(out / "output-manifest.json", {"files": files, "scope": "observed regular files; not a claim that timed-out processes are stopped"})
    print(json.dumps({"status": receipt["status"], "exit_code": receipt["exit_code"], "case_results": [
        {"case": r["case"], "diagnostic_assertions_satisfied": r["diagnostic_assertions_satisfied"], "command_exit": r["command"]["exit_code"], "report_status": (r["report"] or {}).get("status")} for r in receipt["case_results"]], "certification": "not_run"}, sort_keys=True))
    return receipt["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
