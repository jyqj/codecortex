#!/usr/bin/env python3
"""P8-011: bounded process/SQLite/deletion recovery against owned stdio fixtures.

The original local mode retains its bounded process/SQLite/deletion scope.
The explicit full matrix also runs active HTTP/cache/replacement faults and
original production-linked persistence tests at one exact source revision.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import shutil
import re
import signal
import socket
import sqlite3
import subprocess
import sys
import threading
import time

from p8_cold_build import digest, new_directory, observed_operation, write_json
from p8_rollback import (FIXTURE_FILES, MARKER, Product, binary_identity, db_snapshot,
                         file_manifest, require, source_manifest)
from resource_harness.runtime import Journal


DELETED = "p8_recovery_deleted_symbol"
ADDED = "p8_recovery_after_restart"
UNLOCKED = "p8_recovery_after_unlock"
CASES = ("kill_restart", "database_busy", "deleted_source")
ACTIVE_SEEDS = (223, 227, 229)
FAULT_TESTS = (
    ("cc-semantic", "p7_crash_preparation", "isolated_sigkill_reopen_replay_preparation"),
    ("cc-semantic", "crash_independent_review", "independently_observed_sigkill_boundaries"),
    ("cc-db", None, "index_db_rebuild::fault_tests::owned_process_kill_between_sidecar_cleanup_rename_and_reopen_preserves_database"),
    ("cc-db", None, "index_db_rebuild::fault_tests::writer_mutex_spans_rename_reopen_and_connection_installation"),
    ("cc-semantic", "artifact_cache", "meta_tampering_is_detected_as_corrupt"),
    ("cc-semantic", "artifact_cache", "different_spaces_never_cross_hit"),
    ("cc-semantic", "artifact_cache", "namespaces_are_isolated"),
)


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
    with closing(sqlite3.connect(Path(database).as_uri() + "?mode=ro", uri=True, timeout=1)) as connection:
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
        with closing(sqlite3.connect(database, timeout=0)) as second:
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
                  observer_binding="legacy_direct_engineering_scope",
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


def active_stdio_faults(binary, output, seed, expected_binary_sha256):
    """Real HTTP disconnect, active artifact corruption and held-worker DB swap."""
    import p7_fault_lifecycle_stdio as p7

    class DisconnectProvider(p7.Provider):
        def respond(self, handler):
            with self.lock:
                disconnect = self.mode == "disconnect"
                phase = self.phase
            if not disconnect:
                return super().respond(handler)
            require(handler.client_address[0] == "127.0.0.1" and handler.path == "/v1/embeddings",
                    "only the owned loopback embedding endpoint is admitted")
            require(handler.headers.get("Authorization") == "Bearer synthetic-p7-faults",
                    "only the synthetic credential is admitted")
            body = json.loads(handler.rfile.read(int(handler.headers["Content-Length"])))
            require(body.get("model") == f"fake/p7-faults/{seed}", "wrong synthetic model")
            inputs = body.get("input")
            require(isinstance(inputs, list) and inputs and all(isinstance(value, str)
                    and (f"survivor_{seed}" in value or f"removed_{seed}" in value) for value in inputs),
                    "network inputs must be generated fixture text")
            with self.lock:
                row = dict(request_number=len(self.rows) + 1, seed=seed, phase=phase,
                           mode="disconnect", request=body, document_inputs=len(inputs),
                           received_at=time.time(), response_status=None,
                           response_write_completed=False, actual_paid_cost=None)
                self.rows.append(row)
            self.evidence.event("http_request", **row)
            # Receive a complete real request, then close its TCP connection
            # before any response bytes. No provider return value is injected.
            handler.connection.shutdown(socket.SHUT_RDWR)
            handler.connection.close()
            handler.close_connection = True
            self.evidence.event("http_disconnect", request_number=row["request_number"],
                                seed=seed, response_bytes_sent=0)
            self.entered.set()

    binary = Path(binary).resolve(strict=True)
    require(digest(binary) == expected_binary_sha256, "active fault binary differs from its exact build")
    output = new_directory(output)
    evidence = p7.Evidence(output)
    root, cache, home = output / "project", output / "semantic-cache", output / "home"
    root.mkdir()
    home.mkdir()
    source = root / "keep.rs"
    text = f"pub fn survivor_{seed}() -> u32 {{ {seed} }}\n"
    source.write_text(text)
    provider = DisconnectProvider(seed, evidence)
    config = {"auto_index": {"enabled": False}, "query": {"strategy": "local"},
              "semantic": {"enabled": True, "network_opt_in": True,
                           "allow_query_network": True, "allow_http": True,
                           "endpoint": provider.endpoint, "model_id": f"fake/p7-faults/{seed}",
                           "dimensions": 2, "max_input_tokens": 8192, "max_batch_items": 16,
                           "api_key_ref": "env:P7_FAULTS_DUMMY", "breaker_failure_threshold": 100,
                           "retry_max_attempts": 1, "worker_lease_secs": 2,
                           "gc_min_retention_secs": 3600}}
    write_json(root / ".codecortex.json", config)
    children, cases = [], []
    result = dict(seed=seed, status="failed", cases=cases, paid_cost=None,
                  binary_sha256=expected_binary_sha256,
                  network_scope="owned loopback HTTP and generated fixture only")

    def spawn(label):
        product = p7.Product(binary, root, cache, home, label, evidence)
        children.append(product)
        return product

    def semantic(product):
        value = product.tool("search", {"query": f"survivor_{seed}", "retrieval_strategy": "semantic"})
        lanes = value["evidence_summary"]["retrieval"]["lanes"]
        selected = [lane for lane in lanes if lane["lane_id"] == "semantic"]
        require(len(selected) == 1, "public result must retain exactly one semantic lane")
        return value, selected[0]

    try:
        provider.set_mode("disconnect", "active_disconnect")
        product = spawn("active-faults")
        product.index(full=True)
        require(provider.entered.wait(5), "real HTTP disconnect was not reached")
        failed = wait_until(lambda: (status if (status := product.status())["query_pins"] == 0
                            and status["semantic_pending"] > 0 else None), 15,
                            "retryable durable work after provider disconnect")
        require(failed["semantic_state"] == "backfilling" and failed["dense_state"] == "partial"
                and failed["dense_published"] < failed["dense_desired"],
                "failed document request must not become globally ready")
        before = provider.count()
        p7.verify_local(product, seed, text)
        require(provider.count() == before, "local fallback must not spend another provider request")
        failed_db = p7.database_snapshot(root, evidence, seed, "disconnect_pending")
        require(any(row["state"] == "pending" and row["attempt_count"] > 0
                    for row in failed_db["outbox"]), "disconnect did not retain durable retryable work")
        provider.set_mode("success", "disconnect_recovered_new_source")
        text = f"pub fn survivor_{seed}() -> u32 {{ {seed + 1000} }}\n"
        source.write_text(text)
        product.index(full=True)
        recovered = product.wait_state("ready", 1)
        p7.verify_local(product, seed, text)
        cases.append(dict(id="active_provider_network_disconnect", status="passed", before=failed,
                          after=recovered, failure_provider_calls=before, recovery_provider_calls=provider.count() - before,
                          recovery="new authored source supersedes failed work; durable retry clocks unchanged"))

        _, complete = semantic(product)
        require(complete["status"] == "complete" and complete["coverage"]["complete"] is True,
                "active semantic read control is not complete")
        payloads = sorted(cache.rglob("*.bin"))
        require(len(payloads) == 1, "one real published document cache payload is required")
        payload = payloads[0]
        original = payload.read_bytes()
        require(original, "published cache payload is empty")
        backup = output / "original-vector.bin"
        backup.write_bytes(original)
        payload.write_bytes(bytes([original[0] ^ 0xFF]) + original[1:])
        before_corrupt_query = provider.count()
        value, partial = semantic(product)
        require(partial["status"] == "partial" and partial["coverage"]["complete"] is False
                and partial["truncation_reason"] == "semantic_artifact_unavailable",
                "corrupt artifact must yield incomplete query coverage")
        require(provider.count() == before_corrupt_query, "cached query unexpectedly caused another provider call")
        payload.write_bytes(original)
        _, restored = semantic(product)
        require(restored["status"] == "complete" and restored["coverage"]["complete"] is True,
                "restored actual payload did not restore query coverage")
        cases.append(dict(id="active_semantic_cache_corruption", status="passed",
                          artifact=str(payload.relative_to(cache)), original_sha256=digest(backup),
                          corrupt_query=value, restored_lane=restored,
                          additional_provider_requests=provider.count() - before_corrupt_query,
                          global_status_scope="manifest readiness is distinct from per-query artifact coverage"))

        # An unsupported cache format must be rejected by the enabled reader,
        # not merely placed in an unused sibling directory.
        metadata = payload.with_suffix(".meta.json")
        require(metadata.is_file(), "published cache metadata is absent")
        original_meta = metadata.read_bytes()
        (output / "original-vector.meta.json").write_bytes(original_meta)
        changed = json.loads(original_meta)
        changed["format_version"] = 999
        metadata.write_text(json.dumps(changed))
        _, unsupported = semantic(product)
        require(unsupported["status"] == "partial" and unsupported["coverage"]["complete"] is False,
                "active reader interpreted an unsupported vector format as complete")
        metadata.write_bytes(original_meta)
        _, restored_format = semantic(product)
        require(restored_format["status"] == "complete", "original cache format did not recover")
        cases.append(dict(id="active_cache_reader_rejects_unknown_format", status="passed",
                          injected_format=999, original_metadata_sha256=digest(output / "original-vector.meta.json"),
                          rejected_lane=unsupported, restored_lane=restored_format))

        removed = root / "remove.rs"
        removed.write_text(f"pub fn removed_{seed}() -> u32 {{ {seed + 1} }}\n")
        product.index()
        product.wait_state("ready", 2)
        provider.set_mode("hold", "old_database_worker_held")
        source.write_text(f"pub fn survivor_{seed}() -> u32 {{ {seed + 2000} }}\n")
        product.index()
        require(provider.entered.wait(5), "old-incarnation document request did not reach owned HTTP barrier")
        held = product.status()
        require(held["query_pins"] > 0, "old database worker was not physically held")
        removed.unlink()
        text = f"pub fn survivor_{seed}() -> u32 {{ {seed + 3000} }}\n"
        source.write_text(text)
        product.index(full=True)
        swapped = product.status()
        require(swapped["generation"]["incarnation"] != held["generation"]["incarnation"],
                "real full index did not replace the database incarnation")
        require(swapped["semantic_state"] != "ready", "held old result falsely made replacement ready")
        p7.verify_local(product, seed, text)
        provider.set_mode("success", "replacement_current_source")
        settled = product.wait_state("ready", 1)
        p7.verify_local(product, seed, text)
        snapshot = p7.database_snapshot(root, evidence, seed, "replacement_settled")
        require(len(snapshot["manifest"]) == 1, "deleted document was resurrected in semantic manifests")
        db = root / ".codecortex/index.sqlite3"
        with closing(sqlite3.connect(db.as_uri() + "?mode=ro", uri=True)) as connection:
            require(connection.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
                    and not connection.execute("PRAGMA foreign_key_check").fetchall(),
                    "replacement database is not integral")
            invalid = connection.execute("SELECT COUNT(*) FROM semantic_manifest s LEFT JOIN document_manifest d "
                                         "ON d.doc_key=s.doc_key AND d.doc_version=s.doc_version WHERE d.doc_key IS NULL").fetchone()[0]
            require(invalid == 0, "late old-incarnation result published a stale document version")
        product.finish()
        reopened = spawn("replacement-reopened")
        reopened.index()
        reopened.wait_state("ready", 1)
        p7.verify_local(reopened, seed, text)
        reopened.finish()
        cases.append(dict(id="concurrent_database_replacement", status="passed", held=held,
                          swapped=swapped, settled=settled, persisted_manifest=snapshot["manifest"],
                          current_source_sha256=digest(source), deleted_source_absent=not removed.exists()))
        result.update(status="passed", observed_provider_requests=provider.count(),
                      provider_calls=provider.snapshot())
    except BaseException as error:
        result["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        provider.release.set()
        for child in children:
            if child.exit_receipt is None:
                child.finish(cleanup=True)
        provider.close()
        result["product_exits"] = [child.exit_receipt for child in children]
        result["binary_sha256_after"] = digest(binary)
        if result["binary_sha256_after"] != expected_binary_sha256:
            result.update(status="failed", binary_integrity_failure="product changed during active faults")
        write_json(output / "active-faults.json", result)
    require(result["status"] == "passed", "active fault product integrity did not pass")
    return result


def retained_command(command, root, output, env, timeout=1800):
    output = new_directory(output)
    record = dict(command=list(map(str, command)), status="failed", timeout_seconds=timeout)
    started = time.monotonic()
    with (output / "stdout.log").open("x") as stdout, (output / "stderr.log").open("x") as stderr:
        process = subprocess.Popen(command, cwd=root, env=env, stdout=stdout, stderr=stderr,
                                   start_new_session=True)
        record["pid"] = process.pid
        try:
            record["exit_code"] = process.wait(timeout=timeout)
        except BaseException as error:
            record["error"] = f"{type(error).__name__}: {error}"
            os.killpg(process.pid, signal.SIGKILL)
            record["exit_code"] = process.wait(timeout=10)
        record["wall_seconds"] = time.monotonic() - started
    record["logs_sha256"] = {name: digest(output / name) for name in ("stdout.log", "stderr.log")}
    record["status"] = "passed" if record["exit_code"] == 0 and "error" not in record else "failed"
    write_json(output / "command.json", record)
    require(record["status"] == "passed", f"command failed; raw logs retained at {output}")
    return record


def current_product(root, output, env, package):
    from p7_build_identity import source_snapshot
    from p8_cold_build import json_bytes
    output = new_directory(output)
    before = source_snapshot(root)
    (output / "source-inputs.json").write_bytes(json_bytes(before["inputs"]))
    command = [shutil.which("cargo") or "cargo", "build", "-p", "cc-server", "--bin", "codecortex",
               "--locked", "--offline", "--no-default-features", "--message-format=json-render-diagnostics"]
    if package != "default":
        command.extend(["--features", package])
    observed = retained_command(command, root, output / "build", env)
    after = source_snapshot(root)
    require(before == after, "product source changed during build")
    rows = [json.loads(line) for line in (output / "build/stdout.log").read_text().splitlines() if line.strip()]
    artifacts = [row for row in rows if row.get("reason") == "compiler-artifact"
                 and row.get("target", {}).get("name") == "codecortex" and row.get("executable")]
    require(len(artifacts) == 1, "exactly one compiled product is required")
    artifact = artifacts[0]
    require(any(row.get("reason") == "build-finished" and row.get("success") is True for row in rows),
            "product artifact has no successful Cargo completion event")
    expected = {"default": [], "semantic": ["semantic"], "semantic-http": ["semantic", "semantic-http"]}[package]
    require(sorted(artifact.get("features", [])) == expected and artifact.get("target", {}).get("kind") == ["bin"],
            "compiled product feature identity differs")
    target_directory = Path(env["CARGO_TARGET_DIR"]).resolve(strict=True)
    produced = Path(artifact["executable"])
    require(artifact["manifest_path"] == str(root / "crates/cc-server/Cargo.toml")
            and artifact.get("target", {}).get("src_path") == str(root / "crates/cc-server/src/main.rs")
            and produced.resolve(strict=True) == produced
            and produced == target_directory / "debug/codecortex",
            "compiled product belongs to a different source or target")
    profile = artifact.get("profile", {})
    require(profile.get("test") is False and profile.get("opt_level") == "0"
            and profile.get("debug_assertions") is True, "compiled product dev profile differs")
    original = dict(path=str(produced), bytes=produced.stat().st_size, sha256=digest(produced))
    binary = output / "codecortex"
    shutil.copy2(artifact["executable"], binary)
    binary.chmod(0o555)
    require(digest(binary) == original["sha256"] and digest(produced) == original["sha256"]
            and binary.stat().st_size == original["bytes"] and produced.stat().st_size == original["bytes"],
            "copied product differs from Cargo artifact or original changed during copy")
    source = {key: value for key, value in before.items() if key != "inputs"}
    source["source_root"] = str(root)
    record = dict(schema_version=1, status="passed", package_kind=package, build_exit_code=0,
                  stop_reason=None, build_command=command, cargo_artifact=artifact,
                  binary_path=str(binary), binary_sha256=digest(binary), source_before=source,
                  source_after=source, source_manifest="source-inputs.json", build_profile="dev",
                  copy_source=original, binary_bytes=binary.stat().st_size,
                  target_directory=str(target_directory),
                  cold_build_claim=False, release_certified=False, build_observation=observed)
    write_json(output / "build-receipt.json", record)
    return record


def production_fault_test(root, output, env, package, target, test_name):
    from p7_build_identity import source_snapshot
    output = new_directory(output)
    before = source_snapshot(root)
    command = [shutil.which("cargo") or "cargo", "test", "-p", package, "--locked", "--offline",
               "--no-run", "--message-format=json-render-diagnostics"]
    command.extend(["--test", target] if target else ["--lib"])
    build = retained_command(command, root, output / "build", env)
    rows = [json.loads(line) for line in (output / "build/stdout.log").read_text().splitlines() if line.strip()]
    name = target or package.replace("-", "_")
    artifacts = [row for row in rows if row.get("reason") == "compiler-artifact"
                 and row.get("target", {}).get("name") == name and row.get("executable")
                 and row.get("profile", {}).get("test") is True]
    require(len(artifacts) == 1, "exactly one source-bound fault test executable is required")
    artifact = artifacts[0]
    built = Path(artifact["executable"]).resolve(strict=True)
    require(built.is_relative_to(Path(env["CARGO_TARGET_DIR"]).resolve())
            and Path(artifact["manifest_path"]).resolve() == root / "crates" / package / "Cargo.toml",
            "fault test executable escaped the selected source/target")
    require(any(row.get("reason") == "build-finished" and row.get("success") is True for row in rows),
            "fault test artifact has no successful Cargo completion event")
    executable = output / "fault-test"
    shutil.copy2(built, executable)
    executable.chmod(0o555)
    before_sha = digest(executable)
    require(before_sha == digest(built), "retained fault executable differs from its Cargo artifact")
    observed = retained_command([str(executable), "--exact", test_name, "--nocapture", "--test-threads=1"],
                                root, output / "execution", env, timeout=300)
    raw = (output / "execution/stdout.log").read_text()
    require(re.search(r"test result: ok\. 1 passed; 0 failed; 0 ignored;", raw),
            "fault test did not execute exactly one passing nonignored test")
    require(digest(executable) == before_sha and source_snapshot(root) == before,
            "fault executable or complete product source changed during execution")
    record = dict(status="passed", package=package, target=target, test_name=test_name,
                  source=before, cargo_artifact=artifact, executable_sha256=before_sha,
                  retained_executable=str(executable),
                  build=build, execution=observed,
                  scope="original production-linked fault test, not an external installed binary or power-loss claim")
    write_json(output / "test-receipt.json", record)
    return record


def run_full_matrix(root, output, expected_commit, jobs=2, previous_root=None, previous_commit=None):
    """Execute missing fault domains and rollback gates at one exact source."""
    from p7_build_identity import source_snapshot
    from p8_rollback import run_drill, run_version_pair
    root = Path(root).resolve(strict=True)
    source = source_snapshot(root)
    require(source["source_commit"] == expected_commit, "full recovery source differs from requested commit")
    report = dict(schema_version=1, tasks=["P8-011", "P8-016"], status="failed", source=source,
                  observer_binding="legacy_direct_engineering_scope",
                  executions=[], dependency_acceptance="separate original task gates",
                  release_certified=False, actual_paid_cost=None,
                  scope="owned stdio/HTTP fixtures, original production fault suites, and bounded rollback")
    runner_names = ("p8_recovery.py", "p8_rollback.py", "p8_cold_build.py", "p7_build_identity.py",
                    "p7_fault_lifecycle_stdio.py", "resource_harness/runtime.py")
    runner_dir = new_directory(output / "runner-source")
    report["runner_files_sha256"] = {}
    for name in runner_names:
        destination = runner_dir / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(Path(__file__).parent / name, destination)
        report["runner_files_sha256"][name] = digest(destination)
    write_json(output / "source-inputs.json", source["inputs"])
    env = dict(os.environ)
    target = output / "cargo-target"
    require(not target.exists(), "full recovery target must be newly owned")
    env.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS=str(jobs), CARGO_INCREMENTAL="0",
               CARGO_PROFILE_DEV_DEBUG="0", CARGO_PROFILE_TEST_DEBUG="0",
               P7_CRASH_EVIDENCE_DIR=str(output / "p7-crash-raw"),
               CRASH_INDEPENDENT_EVIDENCE_DIR=str(output / "independent-crash-raw"))
    try:
        products = {}
        for package in ("default", "semantic", "semantic-http"):
            products[package] = current_product(root, output / f"product-{package}", env, package)
        report["products"] = products
        default = binary_identity(products["default"]["binary_path"], output / "product-default/build-receipt.json", "default")
        semantic = binary_identity(products["semantic"]["binary_path"], output / "product-semantic/build-receipt.json", "semantic")
        local = run(default, new_directory(output / "local-recovery"))
        require(local["status"] == "passed_limited_local_recovery", "original local recovery matrix failed")
        report["local_recovery"] = local
        report["active_stdio"] = []
        for seed in ACTIVE_SEEDS:
            report["active_stdio"].append(active_stdio_faults(Path(products["semantic-http"]["binary_path"]),
                                                             output / f"active-stdio-{seed}", seed,
                                                             products["semantic-http"]["binary_sha256"]))
        for number, case in enumerate(FAULT_TESTS):
            report["executions"].append(production_fault_test(root, output / f"fault-test-{number:02}", env, *case))
        rollback = run_drill(default, semantic, new_directory(output / "rollback"))
        require(rollback["status"] == "passed_limited_local_drill", "bounded database/config/package rollback failed")
        report["rollback"] = rollback
        report["actual_source_version_pair"] = dict(status="not_run", reason="no previous source checkout requested")
        if previous_root is not None:
            previous_root = Path(previous_root).resolve(strict=True)
            previous_source = source_snapshot(previous_root)
            require(previous_source["source_commit"] == previous_commit,
                    "previous rollback source differs from requested historical commit")
            previous_env = dict(env, CARGO_TARGET_DIR=str(target / "previous-source"))
            previous_product = current_product(previous_root, output / "product-previous", previous_env, "default")
            products["previous-default"] = previous_product
            previous = binary_identity(previous_product["binary_path"], output / "product-previous/build-receipt.json", "default")
            pair = run_version_pair(default, previous, new_directory(output / "actual-version-pair"))
            report["actual_source_version_pair"] = pair
            require(pair["status"] == "passed_actual_source_version_pair", "actual previous/current source rollback failed")
            require(source_snapshot(previous_root) == previous_source, "original previous version source changed")
        require(source_snapshot(root) == source, "full recovery source changed")
        report.update(status="passed_declared_fault_matrix", executed_active_seeds=list(ACTIVE_SEEDS),
                      coverage={"local_process_kill_busy_delete": "3 original stdio cases",
                                "active_disconnect_cache_corruption_database_replacement": "3 seeds of actual HTTP product stdio",
                                "internal_crash_and_swap_windows": "original independently observed production fault tests",
                                "cache_format_and_space_isolation": "active stdio unknown-format rejection plus original artifact-cache tests",
                                "database_config_package_rollback": "controlled future-schema injection with exact current default/semantic binaries",
                                "actual_historical_schema_rollback": report["actual_source_version_pair"]["status"]},
                      limitations=["public source revisions and controlled schema injection are not a released-package pair",
                                   "no power-loss, physical-device failure or real paid provider claim",
                                   "global manifest readiness and per-query artifact coverage are verified separately"])
    except BaseException as error:
        report["error"] = f"{type(error).__name__}: {error}"
    finally:
        report["evidence_files_sha256"] = full_evidence_manifest(output)
        write_json(output / "full-recovery.json", report)
    return report


def full_evidence_manifest(output):
    """Do not open disposable compiler objects or traverse their symlinks."""
    output = Path(output)
    result = {}
    for directory, folders, names in os.walk(output, followlinks=False):
        here = Path(directory)
        if here == output:
            folders[:] = [name for name in folders if name != "cargo-target"]
        require(not any((here / name).is_symlink() for name in folders), "symlink evidence directory")
        for name in names:
            path = here / name
            relative = path.relative_to(output).as_posix()
            if relative == "full-recovery.json":
                continue
            require(not path.is_symlink() and path.is_file(), "nonregular evidence file")
            result[relative] = digest(path)
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--full-matrix", action="store_true")
    parser.add_argument("--source-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--expected-commit")
    parser.add_argument("--previous-source-root", type=Path)
    parser.add_argument("--previous-commit")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--binary", type=Path)
    source_witness = parser.add_mutually_exclusive_group()
    source_witness.add_argument("--build-receipt", type=Path)
    source_witness.add_argument("--engineering-witness", type=Path)
    parser.add_argument("--package-kind", choices=("default", "semantic"))
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--deny-network-wrapper", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.full_matrix:
            if not args.expected_commit or args.jobs < 1:
                parser.error("full matrix requires --expected-commit and positive --jobs")
            if bool(args.previous_source_root) != bool(args.previous_commit):
                parser.error("previous source checkout and exact commit must be supplied together")
            output = new_directory(args.output_dir)
            report = observed_operation(args.source_root, output, "full", "full-recovery.json",
                lambda: run_full_matrix(args.source_root, output, args.expected_commit, args.jobs,
                                        args.previous_source_root, args.previous_commit), full_evidence_manifest)
            print(json.dumps(dict(status=report["status"], error=report.get("error"), receipt=str(output / "full-recovery.json"))))
            return 0 if report["status"] == "passed_declared_fault_matrix" else 1
        if not args.binary or not args.package_kind or not (args.build_receipt or args.engineering_witness):
            parser.error("local recovery requires --binary, --package-kind and a build receipt or engineering witness")
        identity = (engineering_identity(args.binary, args.engineering_witness, args.package_kind)
                    if args.engineering_witness else binary_identity(args.binary, args.build_receipt, args.package_kind))
        output = new_directory(args.output_dir)
        report = (run(identity, output, args.deny_network_wrapper) if args.engineering_witness else
                  observed_operation(Path(__file__).resolve().parents[1], output, "full", "recovery.json",
                      lambda: run(identity, output, args.deny_network_wrapper), file_manifest))
        print(json.dumps(dict(status=report["status"], counts=report.get("counts"), receipt=str(output / "recovery.json"))))
        return 0 if report["status"] == "passed_limited_local_recovery" else 3 if report["status"] == "cancelled" else 1
    except (OSError, ValueError, KeyError) as error:
        print(f"P8 recovery refused: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
