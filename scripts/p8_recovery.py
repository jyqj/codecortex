#!/usr/bin/env python3
"""P8-011: bounded process/SQLite/deletion recovery against owned stdio fixtures.

Uses the existing Product and StdioRPC implementation. Scope is explicit:
SIGKILL after a stopped pending request, a real SQLite writer lock, and persisted
deletion. Active provider/cache and transaction-internal crash points stay not_run.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import re
import signal
import sqlite3
import subprocess
import sys
import threading
import time

from p8_cold_build import digest, new_directory, write_json
from p8_rollback import (FIXTURE_FILES, MARKER, Product, binary_identity, db_snapshot,
                         file_manifest, require, source_manifest)
from resource_harness.runtime import Journal


DELETED = "p8_recovery_deleted_symbol"
ADDED = "p8_recovery_after_restart"
UNLOCKED = "p8_recovery_after_unlock"
CASES = ("kill_restart", "database_busy", "deleted_source")


def engineering_identity(binary, witness_path, package):
    """Validate the separate post-build witness; never relax a cold-build gate."""
    binary, witness_path = Path(binary).resolve(strict=True), Path(witness_path).resolve(strict=True)
    witness = json.loads(witness_path.read_text())
    require(witness.get("schema_version") == 1
            and witness.get("receipt_kind") == "post_build_source_and_binary_witness",
            "an explicit post-build engineering witness is required")
    require(witness.get("cold_build_claim") is False and witness.get("release_certified") is False,
            "engineering witness cannot claim cold-build or release certification")
    require(witness.get("build_exit_code") == 0 and witness.get("package_kind") == package,
            "engineering build exit/package mismatch")
    require(witness.get("binary_sha256") == digest(binary), "engineering product binary digest mismatch")
    command = witness.get("build_command", [])
    require("build" in command and "--locked" in command and "--offline" in command,
            "engineering command must preserve its actual locked/offline build")
    artifact = witness.get("cargo_artifact", {})
    require(artifact.get("target", {}).get("name") == "codecortex"
            and artifact.get("target", {}).get("kind") == ["bin"]
            and artifact.get("fresh") is False,
            "engineering witness does not bind a freshly emitted product artifact")
    require(artifact.get("features") == ([] if package == "default" else ["semantic"]),
            "engineering artifact feature mismatch")
    source_commit = witness.get("source_commit", "")
    require(re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", source_commit), "engineering source commit is missing")
    manifest_name = witness.get("source_manifest", "")
    require(isinstance(manifest_name, str) and Path(manifest_name).name == manifest_name,
            "engineering manifest must be a sibling filename")
    manifest_path = witness_path.parent / manifest_name
    require(digest(manifest_path) == witness.get("source_manifest_sha256"), "engineering source manifest digest mismatch")
    manifest = json.loads(manifest_path.read_text())
    require(isinstance(manifest, dict) and len(manifest) == witness.get("source_input_count"),
            "engineering source manifest count mismatch")
    root = Path(artifact["manifest_path"]).resolve(strict=True).parents[2]
    entries = subprocess.check_output(["git", "ls-tree", "-r", "-z", source_commit, "--",
                                       "Cargo.toml", "Cargo.lock", "crates"], cwd=root)
    algorithm = subprocess.check_output(["git", "rev-parse", "--show-object-format"], cwd=root, text=True).strip()
    names = set()
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        meta, name_bytes = entry.split(b"\t", 1)
        mode, kind, expected_blob = meta.decode().split()
        name = os.fsdecode(name_bytes)
        path = root / name
        require(kind == "blob" and mode in ("100644", "100755")
                and path.resolve() == path and path.is_file() and not path.is_symlink(),
                "engineering source input is not a bound regular file")
        data = path.read_bytes()
        blob = hashlib.new(algorithm, b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        require(blob == expected_blob and hashlib.sha256(data).hexdigest() == manifest.get(name),
                f"engineering committed source bytes changed: {name}")
        names.add(name)
    require(names == set(manifest) and {"Cargo.toml", "Cargo.lock", "crates/cc-server/Cargo.toml"} <= names,
            "engineering manifest does not exactly enumerate committed Cargo/crate inputs")
    require(set(witness.get("logs", {})) == {"product-build.jsonl", "product-build.stderr"},
            "engineering witness must bind both actual build logs")
    related = []
    for name, sha in witness["logs"].items():
        path = witness_path.parent / name
        require(digest(path) == sha, "engineering build log digest mismatch")
        related.append(str(path))
    messages = [json.loads(line) for line in (witness_path.parent / "product-build.jsonl").read_text().splitlines() if line.strip()]
    products = [row for row in messages if row.get("reason") == "compiler-artifact"
                and row.get("target", {}).get("name") == "codecortex" and row.get("executable")]
    require(products == [artifact] and any(row.get("reason") == "build-finished" and row.get("success") is True
                                           for row in messages), "engineering build log/artifact success mismatch")
    return dict(binary_path=str(binary), binary_sha256=digest(binary), package_kind=package,
                receipt_path=str(witness_path), receipt_sha256=digest(witness_path),
                source=dict(source_commit=source_commit, source_root=str(root), input_count=len(manifest),
                            manifest_sha256=digest(manifest_path)), source_manifest_path=str(manifest_path),
                witness_related_files=related, cold_build_claim=False, release_certified=False,
                claim="post-build source/binary witness; dependencies reused; not a cold-build receipt or compiler attestation")


class AsyncCall:
    """Only schedules an existing Product operation; no second RPC transport."""
    def __init__(self, function):
        self.done = threading.Event()
        self.outcome = None
        def run():
            try:
                self.outcome = dict(kind="result", value=function())
            except Exception as error:
                self.outcome = dict(kind="error", type=type(error).__name__, message=str(error),
                                    event=getattr(error, "event", None))
            finally:
                self.done.set()
        self.thread = threading.Thread(target=run, name="p8-owned-rpc", daemon=True)
        self.thread.start()

    def collect(self, timeout):
        require(self.done.wait(timeout), "owned request did not finish within its observation deadline")
        self.thread.join(timeout=1)
        require(not self.thread.is_alive(), "owned request thread did not join")
        return self.outcome


def wait_until(predicate, seconds, description):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.01)
    raise TimeoutError(f"timed out observing {description}")


def process_is_stopped(process):
    # Popen/waitpid may expose a different PID view from this container's
    # mounted /proc (observed in the executor). Wait on the child we own;
    # never infer its state from an unrelated /proc or ps entry with that ID.
    pid, status = os.waitpid(process.pid, os.WUNTRACED | os.WNOHANG)
    if pid == 0:
        return False
    require(pid == process.pid and os.WIFSTOPPED(status), "owned product exited instead of stopping")
    require(os.WSTOPSIG(status) == signal.SIGSTOP, "observed a different stop signal")
    return True


def pending_request(product):
    with product.transport.pending.lock:
        requests = list(product.transport.pending.pending)
    return requests[-1] if requests else None


def query_rows(value):
    rows = value if isinstance(value, list) else value.get("results", value.get("hits"))
    require(isinstance(rows, list), "search response has no structured result list")
    require(all(isinstance(row, dict) for row in rows), "search result is not an object")
    return rows


def verify_symbol(project, response, name, *, present, deleted_path=None):
    rows = query_rows(response)
    matched = [row for row in rows if row.get("name") == name or row.get("qname") == name]
    if not present:
        require(not matched, "deleted symbol was resurrected in public results")
        if deleted_path:
            require(all(row.get("file_path") != deleted_path for row in rows),
                    "public results returned the deleted source path")
        return dict(expected_present=False, matching_symbols=0, returned_rows=len(rows))
    require(matched, "public query did not return the expected authored symbol")
    checked = []
    for row in matched:
        relative = row.get("file_path")
        require(isinstance(relative, str) and not Path(relative).is_absolute(), "public source path is not relative")
        path = project / relative
        require(path.resolve().is_relative_to(project.resolve()) and path.is_file() and not path.is_symlink(),
                "returned source path is missing or outside the owned project")
        lines = path.read_text().splitlines()
        start, end = row.get("start_line"), row.get("end_line")
        require(isinstance(start, int) and isinstance(end, int) and 1 <= start <= end <= len(lines),
                "returned source span is invalid")
        require(f"fn {name}(" in "\n".join(lines[start-1:end]), "returned source span does not define the symbol")
        checked.append(dict(file_path=relative, source_sha256=digest(path), start_line=start, end_line=end))
    return dict(expected_present=True, matching_symbols=len(matched), checked=checked)


def search(product, name, *, present=True, deleted_path=None):
    value = product.tool("search", dict(query=name, mode="symbol", top_k=8,
                                        project_path=str(product.project)), timeout=15)
    check = verify_symbol(product.project, value, name, present=present, deleted_path=deleted_path)
    return dict(response=value, check=check)


def checked_capability(product):
    value = product.tool("status", dict(aspect="capabilities", project_path=str(product.project)), timeout=15)
    retrieval = value.get("retrieval", {})
    require(value.get("has_index") is True and value.get("capabilities", {}).get("search") is True,
            "recovery did not restore an available local index")
    require(retrieval.get("local_state") == "available"
            and retrieval.get("identity_validation") == "checked_at_observation_boundary",
            "local availability is not bound to a checked database snapshot")
    require(retrieval.get("semantic_state") == "not_configured" and retrieval.get("dense_state") == "disabled",
            "disabled semantic recovery made a false ready claim")
    require(retrieval.get("query_coverage", {}).get("state") == "not_measured",
            "global status unexpectedly claimed per-query coverage")
    product.transport.pending.raise_if_terminal()
    return value


def fixture(case_dir):
    project = new_directory(case_dir / "fixture")
    (project / "src").mkdir()
    for name, text in FIXTURE_FILES.items():
        (project / name).write_text(text)
    (project / "src/deleted.rs").write_text(f"pub fn {DELETED}() -> u32 {{ 31 }}\n")
    write_json(project / ".codecortex.json", dict(semantic=dict(enabled=False),
                                                 auto_index=dict(enabled=False)))
    return project


def database_symbol_count(database, name):
    with sqlite3.connect(Path(database).as_uri() + "?mode=ro", uri=True, timeout=1) as connection:
        return connection.execute("SELECT COUNT(*) FROM symbols WHERE name=?", (name,)).fetchone()[0]


def assert_database(database):
    state = db_snapshot(database)
    require(state["integrity"] == "ok" and state["foreign_key_errors"] == 0,
            "recovered database failed SQLite or foreign-key integrity")
    require(state["indexed_files"] > 0, "recovered database has no indexed files")
    return state


def classify_killed_request(observation):
    require(observation.get("stop_observed") is True, "SIGSTOP was not actually observed")
    require(isinstance(observation.get("pending_request_id"), int), "no pending RPC was observed")
    require(observation.get("exit_code") == -signal.SIGKILL, "product did not exit by the injected SIGKILL")
    outcome = observation.get("request", {})
    require(outcome.get("kind") == "error", "killed pending request was incorrectly reported successful")
    event = outcome.get("event") or {}
    require(event.get("kind") in ("process_exit", "stdout_eof", "write_error"),
            "pending request did not report an actual process/pipe terminal event")


def kill_restart(identity, case_dir, journal, wrapper):
    project = fixture(case_dir)
    before = source_manifest(project)
    product = Product(identity, project, case_dir / "before-kill", case_dir / "semantic-cache", wrapper)
    killed = False
    try:
        product.verify_local()
        search(product, DELETED)
        journal.emit("injection_begin", kind="SIGSTOP", pid=product.process.pid,
                     stage="after_committed_index_before_next_request_processing")
        product.process.send_signal(signal.SIGSTOP)
        wait_until(lambda: process_is_stopped(product.process), 3, "owned product SIGSTOP state")
        observation = dict(stop_observed=True, pid=product.process.pid)
        observation["stop_observation_method"] = "waitpid_owned_child_WUNTRACED_SIGSTOP"
        with (project / "src/lib.rs").open("a") as source:
            source.write(f"pub fn {ADDED}() -> u32 {{ 47 }}\n")
        expected = source_manifest(project)
        call = AsyncCall(lambda: product.tool("index", dict(path=str(project), full=False), timeout=15))
        observation["pending_request_id"] = wait_until(lambda: pending_request(product), 3, "pending index request")
        require(not call.done.is_set(), "index request completed while product was stopped")
        journal.emit("injection", kind="SIGKILL", pid=product.process.pid,
                     pending_request_id=observation["pending_request_id"],
                     stage="RPC_pending_while_process_stopped_not_transaction_internal")
        product.process.kill()
        killed = True
        observation["exit_code"] = product.process.wait(timeout=5)
        observation["request"] = call.collect(5)
        write_json(case_dir / "kill-observation.json", observation)
        classify_killed_request(observation)
    finally:
        if not killed and product.process.poll() is None:
            product.process.send_signal(signal.SIGCONT)
        product.close(expected_exit_code=-signal.SIGKILL if killed else 0)
    journal.emit("recovery_begin", stage="new_process_full_rebuild_from_owned_source")
    restarted = Product(identity, project, case_dir / "after-restart", case_dir / "semantic-cache", wrapper)
    try:
        restarted.verify_local()
        result = dict(kill=observation, query=search(restarted, ADDED),
                      capabilities=checked_capability(restarted),
                      database=assert_database(project / ".codecortex/index.sqlite3"))
    finally:
        restarted.close()
    require(source_manifest(project) == expected, "restart changed authored source beyond the explicit mutation")
    result.update(source_before=before, expected_source_after=expected,
                  stage="pending request at a stopped process; no transaction-midpoint claim")
    return result


def is_busy_error(outcome):
    return (outcome.get("kind") == "error" and
            any(word in outcome.get("message", "").lower() for word in ("busy", "locked")))


def incremental_after_busy(product, journal):
    for attempt in range(1, 4):
        try:
            result = product.tool("index", dict(path=str(product.project), full=False), timeout=15)
            journal.emit("incremental_result", attempt=attempt, result=result)
            return result
        except Exception as error:
            outcome = dict(kind="error", message=str(error))
            journal.emit("incremental_error", attempt=attempt, error=outcome)
            if not is_busy_error(outcome) or attempt == 3:
                raise
            time.sleep(0.2)
    raise AssertionError("bounded retry loop exhausted")


def database_busy(identity, case_dir, journal, wrapper):
    project = fixture(case_dir)
    before = source_manifest(project)
    database = project / ".codecortex/index.sqlite3"
    product = Product(identity, project, case_dir / "lock-window", case_dir / "semantic-cache", wrapper)
    lock = None
    try:
        product.verify_local()
        lock = sqlite3.connect(database, timeout=1)
        lock.execute("BEGIN IMMEDIATE")
        lock_started = time.monotonic()
        journal.emit("injection", kind="sqlite_writer_lock", stage="writer_lock_before_source_update")
        with sqlite3.connect(database, timeout=0) as second:
            try:
                second.execute("BEGIN IMMEDIATE")
            except sqlite3.OperationalError as error:
                require("locked" in str(error).lower(), "second connection failed for a reason other than SQLITE_BUSY")
                lock_probe = dict(blocked=True, error=str(error), sqlite_errorcode=getattr(error, "sqlite_errorcode", None))
            else:
                second.rollback()
                raise ValueError("SQLite writer lock was not exclusive")
        with (project / "src/lib.rs").open("a") as source:
            source.write(f"pub fn {UNLOCKED}() -> u32 {{ 59 }}\n")
        expected = source_manifest(project)
        call = AsyncCall(lambda: product.tool("index", dict(path=str(project), full=False), timeout=15))
        # Hold longer than the product's declared 5s busy timeout. It must
        # remain pending or explicitly fail busy, never complete the update.
        completed_while_locked = call.done.wait(6)
        during = call.outcome if completed_while_locked else dict(kind="pending")
        write_json(case_dir / "busy-observation.json", dict(lock_probe=lock_probe,
                   request_while_locked=during, completed_while_locked=completed_while_locked))
        require(not completed_while_locked or is_busy_error(during),
                "index succeeded or failed without an explicit busy indication while the writer lock was held")
        require(database_symbol_count(database, UNLOCKED) == 0, "new symbol committed while an exclusive writer lock was held")
        observation = dict(lock_probe=lock_probe, hold_budget_seconds=6,
                           held_seconds=time.monotonic() - lock_started,
                           request_while_locked=during, new_symbol_rows_while_locked=0,
                           scope="WAL readers may see the previous committed snapshot; update completion is checked separately")
        write_json(case_dir / "busy-observation.json", observation)
        lock.rollback()
        lock.close()
        lock = None
        journal.emit("injection_released", kind="sqlite_writer_lock")
        completed = call.collect(15)
        require(completed["kind"] == "result" or is_busy_error(completed),
                "request did not finish or return a bounded busy error after unlock")
        incremental = incremental_after_busy(product, journal)
        result = dict(busy=observation, first_request_after_unlock=completed,
                      incremental=incremental, query=search(product, UNLOCKED),
                      capabilities=checked_capability(product), database=assert_database(database))
        require(database_symbol_count(database, UNLOCKED) > 0, "recovered symbol is absent from the persisted index")
        require(source_manifest(project) == expected, "busy recovery changed authored source")
        result.update(source_before=before, expected_source_after=expected)
        return result
    finally:
        if lock is not None:
            lock.rollback()
            lock.close()
        product.close()


def deleted_source(identity, case_dir, journal, wrapper):
    project = fixture(case_dir)
    before = source_manifest(project)
    database = project / ".codecortex/index.sqlite3"
    deleted = project / "src/deleted.rs"
    product = Product(identity, project, case_dir / "warm-before-delete", case_dir / "semantic-cache", wrapper)
    try:
        product.verify_local()
        warm = [search(product, DELETED), search(product, DELETED)]
        journal.emit("injection", kind="source_delete", relative_path="src/deleted.rs",
                     stage="after_two_successful_queries_in_the_same_process")
        deleted.unlink()
        expected = source_manifest(project)
        incremental = incremental_after_busy(product, journal)
        after = search(product, DELETED, present=False, deleted_path="src/deleted.rs")
        require(database_symbol_count(database, DELETED) == 0, "deleted symbol remains in the persisted symbols table")
        first = dict(warm_queries=warm, incremental=incremental, deleted_query=after,
                     retained_query=search(product, MARKER), capabilities=checked_capability(product))
    finally:
        product.close()
    journal.emit("recovery_begin", stage="reopen_with_auto_index_disabled_and_no_explicit_index")
    reopened = Product(identity, project, case_dir / "reopen-without-index", case_dir / "semantic-cache", wrapper)
    try:
        reopened.initialize()
        second = dict(deleted_query=search(reopened, DELETED, present=False, deleted_path="src/deleted.rs"),
                      retained_query=search(reopened, MARKER), capabilities=checked_capability(reopened),
                      database=assert_database(database), explicit_index_called=False)
    finally:
        reopened.close()
    require(not deleted.exists() and source_manifest(project) == expected,
            "deleted source was recreated or other authored source changed")
    require(database_symbol_count(database, DELETED) == 0, "deleted symbol reappeared after reopen")
    return dict(after_incremental=first, after_reopen=second,
                source_before=before, expected_source_after=expected, deleted_source_recreated=False)


def run(identity, output, network_wrapper=None):
    report = dict(schema_version=1, task="P8-011", status="failed", product=identity,
                  source_scope="exact supplied product receipt, not inferred from this runner checkout",
                  runner_files_sha256={name: digest(Path(__file__).parent / name) for name in
                                       ("p8_recovery.py", "p8_rollback.py", "p8_cold_build.py", "resource_harness/runtime.py")},
                  release_certified=False, complete_P8_011=False, cases=[])
    witness = new_directory(output / "build-witness")
    shutil.copyfile(identity["receipt_path"], witness / "build-receipt.json")
    shutil.copyfile(identity["source_manifest_path"], witness / Path(identity["source_manifest_path"]).name)
    for related in identity.get("witness_related_files", []):
        shutil.copyfile(related, witness / Path(related).name)
    if network_wrapper:
        target = output / "network-wrapper.py"
        shutil.copyfile(network_wrapper, target)
        network_wrapper = target
        report["network_policy"] = dict(kind="inherited_process_seccomp", wrapper_sha256=digest(target))
    else:
        report["network_policy"] = dict(kind="disabled_config_only", enforcement="not_measured")
    for name in CASES:
        case_dir = new_directory(output / name)
        row = dict(id=name, status="failed", started_ns=time.monotonic_ns())
        with (case_dir / "stages.jsonl").open("x") as stream:
            journal = Journal(stream)
            try:
                if name == "kill_restart" and not all(hasattr(signal, sig) for sig in ("SIGSTOP", "SIGCONT", "SIGKILL")):
                    row.update(status="not_run", reason="required POSIX signal observations unavailable")
                else:
                    row["result"] = globals()[name](identity, case_dir, journal, network_wrapper)
                    row["status"] = "passed"
            except BaseException as error:
                row["error"] = f"{type(error).__name__}: {error}"
                journal.emit("case_failed", error=row["error"])
                if isinstance(error, (KeyboardInterrupt, SystemExit)):
                    row["status"] = "cancelled"
            row["finished_ns"] = time.monotonic_ns()
            write_json(case_dir / "receipt.json", row)
            report["cases"].append(row)
            write_json(output / "recovery.json", report)
            if row["status"] == "cancelled":
                break
    reached = {case["id"] for case in report["cases"]}
    report["cases"].extend(dict(id=name, status="not_run", reason="not_started_after_cancellation")
                           for name in CASES if name not in reached)
    report["cases"].extend([
        dict(id="transaction_internal_crash_points", status="not_run",
             reason="SIGKILL stage is observed pending/stopped process, not an internal transaction hook"),
        dict(id="active_provider_network_disconnect", status="not_run",
             reason="no enabled provider in this disabled/offline product profile"),
        dict(id="active_semantic_cache_corruption", status="not_run",
             reason="disabled profile does not read semantic vectors; no fake active-cache claim"),
        dict(id="concurrent_database_replacement", status="not_run",
             reason="no concurrent incarnation-swap injection in this limited submatrix")])
    counts = {state: sum(case["status"] == state for case in report["cases"])
              for state in ("passed", "failed", "not_run", "cancelled")}
    report["counts"] = counts
    report["status"] = ("cancelled" if counts["cancelled"] else "failed" if counts["failed"] else
                        "passed_limited_local_recovery" if counts["passed"] == len(CASES) else "incomplete")
    require(digest(identity["binary_path"]) == identity["binary_sha256"], "product bytes changed during recovery run")
    report["evidence_files_sha256"] = {name: sha for name, sha in file_manifest(output).items()
                                       if name != "recovery.json" and "/fixture/.codecortex/" not in name}
    write_json(output / "recovery.json", report)
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    source_witness = parser.add_mutually_exclusive_group(required=True)
    source_witness.add_argument("--build-receipt", type=Path)
    source_witness.add_argument("--engineering-witness", type=Path)
    parser.add_argument("--package-kind", choices=("default", "semantic"), required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--deny-network-wrapper", type=Path)
    args = parser.parse_args(argv)
    try:
        identity = (engineering_identity(args.binary, args.engineering_witness, args.package_kind)
                    if args.engineering_witness else binary_identity(args.binary, args.build_receipt, args.package_kind))
        output = new_directory(args.output_dir)
        report = run(identity, output, args.deny_network_wrapper)
        print(json.dumps(dict(status=report["status"], counts=report["counts"], receipt=str(output / "recovery.json"))))
        return 0 if report["status"] == "passed_limited_local_recovery" else 3 if report["status"] == "cancelled" else 1
    except (OSError, ValueError, KeyError) as error:
        print(f"P8 recovery refused: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
