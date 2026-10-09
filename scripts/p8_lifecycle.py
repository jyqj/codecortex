#!/usr/bin/env python3
"""Measure real stdio lifecycle strata and replay through the existing Rust owners.

Every fixture, process and output belongs to this run. Exact public cache-counter
deltas establish hits/misses. OS page cache is not cleared. All attempts, failed
runs and incomplete tail intervals are retained; no performance win is implied.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import signal
import sqlite3
import stat
import subprocess
import sys
import time

from p7_build_identity import source_snapshot, verify_release_profile
from p8_cold_build import digest, new_directory, write_json
from p8_resources import OwnedProcessProbe, self_usage
from p8_rollback import Product, binary_identity, db_snapshot, require, source_manifest


CONFIG = {"auto_index": {"enabled": False}, "semantic": {"enabled": False},
          "indexing": {"max_concurrent_parse": 2},
          "query": {"strategy": "local", "deadline_ms": 30000}}
COUNTERS = ("graph_hits", "graph_misses", "result_hits", "result_misses")
FTS_TABLES = ("chunks_fts", "symbols_fts", "literal_fts", "files_fts", "file_paths_fts")
OBSERVER_FILES = ("p8_lifecycle.py", "p8_resources.py", "p8_rollback.py", "p8_cold_build.py",
                  "p7_build_identity.py", "resource_harness/__init__.py", "resource_harness/runtime.py")


def observer_digests():
    root = Path(__file__).resolve().parent
    return {str(root / name): digest(root / name) for name in OBSERVER_FILES}


def verify_current_source(identity):
    """Bind the observed product and every reused observer module to this commit."""
    root = Path(__file__).resolve().parents[1]
    current = source_snapshot(root)
    require({key: value for key, value in current.items() if key != "inputs"} == identity["source"],
            "lifecycle product receipt is not the current fixed source")
    for filename, observed in observer_digests().items():
        relative = Path(filename).relative_to(root).as_posix()
        committed = subprocess.check_output(["git", "show", current["source_commit"] + ":" + relative],
                                            cwd=root, stderr=subprocess.PIPE)
        require(hashlib.sha256(committed).hexdigest() == observed,
                "lifecycle observer module differs from the fixed source: " + relative)


def evaluator_identity(binary, receipt_path, product, *, release):
    """Consume the original Cargo build log, fixed source and retained artifact."""
    binary, receipt_path = Path(binary).resolve(strict=True), Path(receipt_path).resolve(strict=True)
    require(receipt_path.stat().st_size <= 1024 * 1024, "replay build receipt exceeds byte bound")
    receipt = json.loads(receipt_path.read_text())
    require(receipt.get("schema_version") == 1 and receipt.get("build_exit_code") == 0,
            "replay build did not succeed")
    require(receipt.get("source_before") == receipt.get("source_after") == product["source"],
            "replay build source differs from fixed product")
    require(receipt.get("binary_sha256") == digest(binary), "replay artifact digest differs")
    command = receipt.get("build_command", [])
    require(isinstance(command, list) and all(isinstance(arg, str) for arg in command)
            and "--locked" in command and command.count("--bin") == 1 and command.count("-p") == 1
            and command.count("--target-dir") == 1,
            "replay Cargo invocation is not explicit/locked")
    require(command[command.index("--bin") + 1:command.index("--bin") + 2] == ["p8-measurements"]
            and command[command.index("-p") + 1:command.index("-p") + 2] == ["cc-eval"],
            "replay Cargo invocation selects another executable")
    logs = {}
    for key in ("build_stdout", "build_stderr"):
        item = receipt.get(key, {})
        name = item.get("name")
        require(isinstance(name, str) and Path(name).name == name, "replay build log is not a sibling filename")
        path = receipt_path.parent / name
        require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 64 * 1024 * 1024,
                "replay build log missing or over byte bound")
        require(digest(path) == item.get("sha256"), "original replay Cargo log changed")
        logs[key] = path
    messages = [json.loads(line) for line in logs["build_stdout"].read_text().splitlines() if line]
    require(messages and messages[-1] == {"reason": "build-finished", "success": True},
            "original replay Cargo completion missing")
    artifacts = [row for row in messages if row.get("reason") == "compiler-artifact"
                 and row.get("target", {}).get("name") == "p8-measurements"]
    require(len(artifacts) == 1 and artifacts[0] == receipt.get("artifact"),
            "original replay Cargo artifact missing, duplicated or changed")
    artifact = artifacts[0]
    target = artifact.get("target", {})
    require(target.get("kind") == ["bin"] and artifact.get("features") in ([], ["default"])
            and artifact.get("profile", {}).get("test") is False,
            "replay artifact is not the default standalone binary")
    root = Path(__file__).resolve().parents[1]
    for actual, wanted in ((artifact.get("manifest_path"), root / "crates/cc-eval/Cargo.toml"),
                           (target.get("src_path"), root / "crates/cc-eval/src/bin/p8-measurements.rs")):
        require(isinstance(actual, str) and Path(actual).is_absolute()
                and Path(actual) == wanted and Path(actual).resolve(strict=True) == wanted,
                "replay artifact source paths differ from this fixed checkout")
    executable = artifact.get("executable")
    copied = receipt.get("copy_source", {})
    require(isinstance(executable, str) and Path(executable).is_absolute()
            and copied.get("path") == executable, "original replay executable/copy source missing or different")
    original = Path(executable)
    require(original.is_file() and not original.is_symlink(), "original replay executable missing or nonregular")
    index = command.index("--target-dir")
    require(len(command) > index + 1 and Path(command[index + 1]).resolve(strict=True) in original.resolve(strict=True).parents,
            "original replay executable outside recorded Cargo target")
    require(original.name == "p8-measurements" and digest(original) == copied.get("sha256") == digest(binary),
            "retained replay bytes differ from actual Cargo executable/copy source")
    if release:
        require("--release" in command, "replay invocation did not request release")
        verify_release_profile(artifact["profile"])
    return {"receipt_path": str(receipt_path), "receipt_sha256": digest(receipt_path),
            "binary_path": str(binary), "binary_sha256": digest(binary),
            "source": product["source"], "artifact": artifact, "build_command": command,
            "copy_source": copied,
            "build_stdout": receipt["build_stdout"], "build_stderr": receipt["build_stderr"],
            "status": "original_cargo_artifact_and_source_verified"}


def artifact_inventory(root):
    """Seal all retained raw bytes, including sources, SQLite and protocol logs."""
    inventory = {}
    for path in sorted(Path(root).rglob("*")):
        mode = path.lstat().st_mode
        require(not stat.S_ISLNK(mode) and (stat.S_ISREG(mode) or stat.S_ISDIR(mode)),
                "nonregular lifecycle artifact")
        if stat.S_ISREG(mode) and path != Path(root) / "receipt.json":
            inventory[path.relative_to(root).as_posix()] = {"bytes": path.stat().st_size, "sha256": digest(path)}
    return inventory


def verify_artifact_inventory(root, receipt):
    require(receipt.get("artifact_inventory") == artifact_inventory(root),
            "lifecycle raw artifact inventory changed after sealing")


def validate_plan(plan):
    require(plan["profile"] in ("smoke", "release"), "unknown lifecycle profile")
    for key in ("query_samples", "cold_samples", "files", "request_timeout_seconds", "run_timeout_seconds"):
        require(type(plan[key]) is int and plan[key] > 0, f"invalid positive {key}")
    require(plan["query_samples"] <= 1000 and plan["cold_samples"] <= 200
            and 2 <= plan["files"] <= 256, "lifecycle sample/file bound exceeded")
    require(plan["request_timeout_seconds"] <= 300 and plan["run_timeout_seconds"] <= 7200,
            "lifecycle deadline bound exceeded")
    require(1024 * 1024 <= plan["artifact_budget_bytes"] <= 1024**3, "invalid artifact byte bound")
    if plan["profile"] == "release":
        require(plan["query_samples"] >= 200 and plan["cold_samples"] >= 30,
                "release profile needs at least 200 homogeneous query samples and 30 cold builds")
    return plan


def symbol(number):
    return f"p8_lifecycle_{number:06d}"


def source_path(number, files):
    return f"source_{number % files:03d}.py"


def make_fixture(root, files, queries):
    root.mkdir()
    content = {f"source_{number:03d}.py": [] for number in range(files)}
    for number in range(max(queries + 2, files)):
        content[source_path(number, files)].append(
            f"def {symbol(number)}(value):\n    return value + 17\n\n")
    for name, functions in content.items():
        (root / name).write_text("".join(functions))
    write_json(root / ".codecortex.json", CONFIG)
    require(not (root / ".codecortex").exists(), "cold fixture already has an index/cache")
    return source_manifest(root)


def index_identity(status):
    freshness = status.get("resolution_freshness", {})
    generation = status.get("diagnostics", {}).get("retrieval", {}).get("generation")
    require(type(status.get("indexed_files")) is int and status["indexed_files"] >= 0,
            "missing public indexed file count")
    require(freshness.get("complete") is True and freshness.get("status") == "ready",
            "public resolution is not complete")
    require(type(freshness.get("index_epoch")) is int and isinstance(generation, dict),
            "public index generation is missing")
    return {"indexed_files": status["indexed_files"], "index_epoch": freshness["index_epoch"],
            "generation": generation}


def cache_observation(before, after):
    """Exactly one public query on an otherwise idle owned session.

    Top-level graph-result cache wins when used; an underlying result miss is
    not mislabeled as a graph hit. Missing, reset or multiple lookups are unknown.
    """
    old = before.get("diagnostics", {}).get("search_cache")
    new = after.get("diagnostics", {}).get("search_cache")
    require(isinstance(old, dict) and isinstance(new, dict), "cache counters missing")
    require(index_identity(before) == index_identity(after), "index changed during the query")
    delta = {}
    for key in COUNTERS:
        require(type(old.get(key)) is int and type(new.get(key)) is int
                and 0 <= old[key] <= new[key], "cache counter missing/reset")
        delta[key] = new[key] - old[key]
    graph = delta["graph_hits"], delta["graph_misses"]
    result = delta["result_hits"], delta["result_misses"]
    if graph == (1, 0) and result == (0, 0):
        return "hit", {"owner": "graph_result_cache", "delta": delta}
    if graph == (0, 1) and result in ((0, 0), (0, 1), (1, 0)):
        return "miss", {"owner": "graph_result_cache", "delta": delta}
    if graph == (0, 0) and result in ((1, 0), (0, 1)):
        return "hit" if result[0] else "miss", {"owner": "result_cache", "delta": delta}
    raise ValueError("cache counters do not isolate exactly one current query lookup")


def usage_delta(before, after):
    if not isinstance(before, dict) or not isinstance(after, dict) or before.get("pid") != after.get("pid"):
        return None
    result = {"pid": before.get("pid"), "method": "same-session native SELF cumulative-counter difference"}
    for field in ("user_cpu_ns", "system_cpu_ns"):
        a, b = before.get(field), after.get(field)
        result[field] = b - a if type(a) is int and type(b) is int and 0 <= a <= b else None
    io = {}
    for field in ("read_bytes", "write_bytes", "rchar", "wchar", "read_syscalls", "write_syscalls"):
        a, b = (before.get("io") or {}).get(field), (after.get("io") or {}).get(field)
        io[field] = b - a if type(a) is int and type(b) is int and 0 <= a <= b else None
    result["io"] = io
    result["scope"] = "bracketing status diagnostics add observation work; not pure query service CPU or IO"
    return result


def disk_objects(project):
    """Physical byte totals and logical FTS page attribution are separate."""
    database = project / ".codecortex/index.sqlite3"
    database_state = db_snapshot(database)
    logical = {"status": "unavailable", "tables": None,
               "scope": "logical SQLite pages within the shared DB; do not add to physical byte totals"}
    try:
        with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=1)) as connection:
            rows = list(connection.execute("SELECT name, SUM(pgsize) FROM dbstat GROUP BY name"))
            logical = {**logical, "status": "observed", "tables": dict(rows),
                       "fts_logical_bytes": {name: (sum(size for table, size in rows
                                                       if table == name or table.startswith(name + "_"))
                                                    if any(table == name or table.startswith(name + "_")
                                                           for table, _ in rows) else None)
                                             for name in FTS_TABLES},
                       "page_size": connection.execute("PRAGMA page_size").fetchone()[0],
                       "page_count": connection.execute("PRAGMA page_count").fetchone()[0],
                       "free_pages": connection.execute("PRAGMA freelist_count").fetchone()[0]}
    except sqlite3.Error as error:
        logical["reason"] = str(error)
    # SQLite read probes can create SHM/WAL files. Only enumerate after every
    # probe has closed; logical pages are never added again as physical FTS.
    started = time.monotonic_ns()
    objects = []
    for base in (project / ".codecortex", project / ".cache"):
        if not base.exists():
            continue
        for path in sorted(base.rglob("*")):
            require(not path.is_symlink(), "owned storage symlink is not admitted")
            if not path.is_file():
                continue
            metadata = path.stat()
            require(len(objects) < 10000, "storage inventory bound exceeded")
            component = "shared" if path.name.startswith("index.sqlite3") else "other"
            objects.append({"storage_id": f"device:{metadata.st_dev}:inode:{metadata.st_ino}",
                            "component": component, "bytes": metadata.st_size,
                            "path": str(path), "allocated_bytes": getattr(metadata, "st_blocks", 0) * 512
                            if hasattr(metadata, "st_blocks") else None})
    return {"objects": objects, "logical_sqlite": logical, "database": database_state,
            "physical_snapshot": {"started_monotonic_ns": started, "finished_monotonic_ns": time.monotonic_ns(),
                                  "scope": "closed product fixture; after closing SQLite observation connections"}}


class Driver:
    def __init__(self, identity, evaluator, output, plan, network_wrapper=None, evaluator_build=None):
        self.identity, self.evaluator, self.output, self.plan = identity, evaluator, output, plan
        self.network_wrapper = network_wrapper
        self.evaluator_build = evaluator_build
        self.deadline = time.monotonic() + plan["run_timeout_seconds"]
        self.samples, self.raw_queries, self.resources, self.operations = [], [], [], []
        self.sessions, self.storage, self.failures = [], [], []
        self.disk_partitions = []
        self.cancelled = False
        self.persisted_identity = None
        self.events = (output / "events.jsonl").open("x")
        self.evaluator_digest = digest(evaluator)
        self.runner_digests = observer_digests()
        (output / "sessions").mkdir()
        (output / "fixtures").mkdir()
        (output / "semantic-cache").mkdir()

    def event(self, kind, **fields):
        self.events.write(json.dumps({"kind": kind, "monotonic_ns": time.monotonic_ns(), **fields}, sort_keys=True) + "\n")
        self.events.flush()

    def guard(self):
        require(not self.cancelled, "measurement cancelled")
        require(time.monotonic() < self.deadline, "lifecycle run deadline reached")
        size = sum(path.stat().st_size for path in self.output.rglob("*") if path.is_file())
        require(size <= self.plan["artifact_budget_bytes"], "lifecycle evidence/fixture byte budget reached")

    def start(self, name, project):
        self.guard()
        product = Product(self.identity, project, self.output / "sessions" / name,
                          self.output / "semantic-cache" / name, self.network_wrapper)
        try:
            product.initialize()
        except BaseException:
            product.close()
            raise
        self.sessions.append(product.record)
        self.event("session_started", session=name, pid=product.process.pid, project=str(project))
        return product

    def close(self, product):
        product.close()
        self.event("session_closed", session=product.output.name, process=product.record)

    def status(self, product):
        status = product.tool("status", {"aspect": "index"}, timeout=self.plan["request_timeout_seconds"])
        diagnostics = status.get("diagnostics", {})
        require(diagnostics.get("auto_index_enabled") is False
                and diagnostics.get("semantic_configured") is False,
                "profile requires observed auto-index and semantic disabled")
        require(diagnostics.get("retrieval", {}).get("semantic_state") == "not_configured"
                and diagnostics.get("retrieval", {}).get("dense_state") == "disabled",
                "disabled semantic capability state is inconsistent")
        return status

    def resource(self, product, status, phase):
        native = status.get("diagnostics", {}).get("process_resources")
        if native is not None:
            require(native.get("pid") == product.process.pid, "stdio SELF resource PID differs from owned child")
        own = self_usage()
        tree = OwnedProcessProbe(product.process.pid, self.identity["binary_path"]).snapshot()
        current = (own.get("native") or {}).get("resident_bytes")
        record = {"stage": phase, "runner_pid": os.getpid(), "server_pid": product.process.pid,
                  "runner_rss_bytes": None, "runner_native_rss_bytes": current,
                  "server_rss_bytes": (native or {}).get("resident_bytes"),
                  "server_tree_rss_bytes": tree["tree"].get("resident_bytes"),
                  "external_service_rss_bytes": None,
                  "method": "runner SELF and server SELF reported through owned stdio; separate validated tree probe; stage snapshots"}
        self.resources.append(record)
        self.event("resource", session=product.output.name, phase=phase, server_native=native,
                   runner_native=own, owned_tree=tree, ledger=record)

    def query(self, product, number, phase, *, first=False, reopened=False, warm=True, record=True):
        self.guard()
        before = self.status(product)
        expected = index_identity(before)
        require(expected["indexed_files"] == self.plan["files"], "query fixture is not fully indexed")
        require(expected == self.persisted_identity, "query process did not reopen the same persisted generation")
        arguments = {"query": symbol(number), "mode": "hybrid", "top_k": 1}
        started = time.monotonic_ns()
        response, error, after, cache, control = None, None, None, "unknown", None
        try:
            response = product.tool("search", arguments, timeout=self.plan["request_timeout_seconds"])
        except Exception as failure:
            error = {"type": type(failure).__name__, "message": str(failure), "event": getattr(failure, "event", None)}
        finished = time.monotonic_ns()
        try:
            after = self.status(product)
            cache, control = cache_observation(before, after)
            required_cache = "hit" if phase == "cache_hit" else "miss"
            require(cache == required_cache, f"observed {cache}, expected {required_cache} in {phase}")
        except Exception as failure:
            error = error or {"type": type(failure).__name__, "message": str(failure), "event": getattr(failure, "event", None)}
        elapsed = (finished - started) // 1000
        state = "success" if error is None else "timeout" if "timeout" in str(error).lower() else "protocol_error"
        evidence = {"operation": "query", "index_was_empty": False, "parse_cache_was_empty": None,
                    "first_query_in_process": first, "reopened_existing_index": reopened,
                    "warmup_completed": warm, "result_cache": cache}
        operation = {"phase": phase, "session": product.output.name, "pid": product.process.pid,
                     "request": arguments, "started_ns": started, "finished_ns": finished,
                     "elapsed_us": elapsed, "before": before, "after": after, "cache_control": control,
                     "response": response, "error": error,
                     "native_usage_delta": usage_delta(before.get("diagnostics", {}).get("process_resources"),
                                                        (after or {}).get("diagnostics", {}).get("process_resources")),
                     "sample_index": len(self.samples) if record else None}
        self.operations.append(operation)
        self.event("query_attempt", **operation)
        if record:
            index = len(self.samples)
            self.samples.append({"evidence": evidence, "status": state, "elapsed_us": elapsed})
            if response is not None:
                self.raw_queries.append({"sample_index": index, "source_root": str(product.project),
                                         "expected_path": source_path(number, self.plan["files"]),
                                         "expected_symbol": symbol(number), "response": response})
        if after is not None:
            self.resource(product, after, phase)
        require(error is None, f"query observation failed: {error}")
        return response

    def cold(self, number):
        self.guard()
        project = self.output / "fixtures" / f"cold-{number:03d}"
        source = make_fixture(project, self.plan["files"], self.plan["query_samples"])
        self.event("pristine_fixture", project=str(project), source_manifest=source,
                   index_and_persistent_parse_cache_absent=True)
        product = self.start(f"cold-{number:03d}", project)
        try:
            before = self.status(product)
            require(index_identity(before)["indexed_files"] == 0, "cold process opened a populated index")
            self.resource(product, before, "cold_build_before")
            response, failure, after = None, None, None
            started = time.monotonic_ns()
            try:
                response = product.tool("index", {"path": str(project), "full": True},
                                        timeout=self.plan["request_timeout_seconds"])
            except Exception as error:
                failure = {"type": type(error).__name__, "message": str(error)}
            finished = time.monotonic_ns()
            try:
                after = self.status(product)
                require(index_identity(after)["indexed_files"] == self.plan["files"], "cold build did not index the complete fixture")
            except Exception as error:
                failure = failure or {"type": type(error).__name__, "message": str(error)}
            self.samples.append({"evidence": {"operation": "build", "index_was_empty": True,
                                               "parse_cache_was_empty": True, "first_query_in_process": None,
                                               "reopened_existing_index": False, "warmup_completed": False,
                                               "result_cache": "unknown"},
                                 "status": "success" if failure is None else "protocol_error",
                                 "elapsed_us": (finished - started) // 1000})
            self.event("cold_build", session=product.output.name, started_ns=started, finished_ns=finished,
                       before=before, after=after, raw=response, error=failure)
            if after is not None:
                self.resource(product, after, "cold_build_after")
                self.persisted_identity = index_identity(after)
            require(failure is None, f"cold build failed: {failure}")
        finally:
            self.close(product)
        require(source == source_manifest(project), "product changed authored fixture inputs")
        storage = disk_objects(project)
        self.storage.append({"project": str(project), **storage})
        self.disk_partitions.extend({key: obj[key] for key in ("storage_id", "component", "bytes")}
                                    for obj in storage["objects"])
        self.event("closed_fixture_storage", project=str(project), **storage)
        return project, source

    def execute(self):
        project, source = None, None
        for number in range(self.plan["cold_samples"]):
            project, source = self.cold(number)
        baseline_db = db_snapshot(project / ".codecortex/index.sqlite3")
        for number in range(self.plan["query_samples"]):
            self.guard()
            product = self.start(f"reopen-{number:03d}", project)
            try:
                self.event("reopen_existing_index", session=product.output.name,
                           before_launch_db=baseline_db, build_calls_in_process=0,
                           source_manifest_unchanged=source == source_manifest(project))
                self.query(product, number, "process_reopen", first=True, reopened=True, warm=False)
            finally:
                self.close(product)
        require(db_snapshot(project / ".codecortex/index.sqlite3") == baseline_db,
                "reopen profile changed persisted index facts")
        product = self.start("warm", project)
        try:
            self.query(product, self.plan["query_samples"] + 1, "warmup", first=True,
                       reopened=True, warm=False, record=False)
            for number in range(self.plan["query_samples"]):
                self.query(product, number, "warm_uncached")
                self.query(product, number, "cache_hit")
        finally:
            self.close(product)
        require(source_manifest(project) == source, "read profile changed source inputs")
        require(db_snapshot(project / ".codecortex/index.sqlite3") == baseline_db,
                "read profile changed persisted index facts")

    def finish(self):
        expected = self.plan["cold_samples"] + 3 * self.plan["query_samples"]
        # Earlier per-cold snapshots remain in events.jsonl. The shared disk
        # ledger uses one final inventory after all reopen/warm sessions end.
        self.storage, self.disk_partitions = [], []
        for project in sorted((self.output / "fixtures").iterdir()):
            if not (project / ".codecortex/index.sqlite3").is_file():
                continue
            storage = disk_objects(project)
            self.storage.append({"project": str(project), **storage})
            self.disk_partitions.extend({key: obj[key] for key in ("storage_id", "component", "bytes")}
                                        for obj in storage["objects"])
            self.event("final_closed_fixture_storage", project=str(project), **storage)
        replay_input = {"schema_version": 1, "profile": self.plan["profile"],
                        "expected_samples": expected, "samples": self.samples,
                        "raw_queries": self.raw_queries, "resources": self.resources,
                        "disk_partitions": self.disk_partitions,
                        "disk_layout_complete": len(self.storage) == self.plan["cold_samples"], "costs": []}
        input_path = self.output / "replay-input.json"
        write_json(input_path, replay_input)
        require(digest(self.evaluator) == self.evaluator_digest, "replay evaluator changed during measurement")
        if self.evaluator_build is not None:
            require(evaluator_identity(self.evaluator, self.evaluator_build["receipt_path"], self.identity,
                                       release=self.plan["profile"] == "release") == self.evaluator_build,
                    "replay build witness changed during measurement")
        require(all(digest(path) == sha for path, sha in self.runner_digests.items()), "lifecycle observer source changed during execution")
        code = 2
        command = [str(self.evaluator), "--input", str(input_path), "--output", str(self.output / "report.json")]
        with (self.output / "replay.stdout").open("x") as stdout, (self.output / "replay.stderr").open("x") as stderr:
            result = subprocess.run(command, stdout=stdout, stderr=stderr, timeout=120)
            code = result.returncode
        report_path = self.output / "report.json"
        require(report_path.is_file(), "replay did not retain its report")
        require(digest(self.evaluator) == self.evaluator_digest, "replay evaluator changed during replay")
        report = json.loads(report_path.read_text())
        require(report.get("exit_code") == code, "replay exit/report mismatch")
        # Preserve an independent second replay and require exact reports.
        replay_path = self.output / "report-replayed.json"
        repeat = subprocess.run([str(self.evaluator), "--input", str(input_path), "--output", str(replay_path)],
                                capture_output=True, text=True, timeout=120)
        write_json(self.output / "repeat-replay-receipt.json", {"exit_code": repeat.returncode,
                    "stdout": repeat.stdout, "stderr": repeat.stderr,
                    "byte_identical_report": replay_path.is_file() and digest(report_path) == digest(replay_path)})
        require(repeat.returncode == code and replay_path.is_file()
                and digest(report_path) == digest(replay_path), "same inputs did not replay identically")
        receipt = {"schema_version": 1, "scope": "observed stdio lifecycle/cache/resources; no release approval or performance-improvement claim",
                   "product": self.identity, "evaluator_sha256": self.evaluator_digest,
                   "evaluator_build": self.evaluator_build,
                   "observer_sha256": self.runner_digests, "plan": self.plan,
                   "sample_counts": {"expected": expected, "recorded": len(self.samples)},
                   "failures": self.failures, "replay": {"command": command, "exit_code": code,
                                                          "input_sha256": digest(input_path), "report_sha256": digest(report_path)},
                   "status": "complete_observation" if not self.failures and code == 0 else "incomplete_or_failed_observation",
                   "release_certified": False, "os_page_cache": "not_cleared_unknown_not_cold_disk",
                   "timing_scope": "synchronous client end-to-end call including transport and raw journal; before/after status and resource probes excluded",
                   "resource_scope": "current server RSS/CPU/IO from server SELF through owned stdio; runner SELF separate; tree only when native identity mapping succeeds; stage samples do not establish continuous peaks",
                   "disk_scope": "final inventory of all retained cold fixture indexes after all product sessions and SQLite observations close; each fixture separately reported; not one active-index footprint; logical FTS pages are already within physical SQLite bytes",
                   "provider_cost": {"status": "not_applicable_to_disabled_profile", "reported": None, "estimated": None,
                                     "input_tokens": None, "output_tokens": None, "requests_billed": None,
                                     "reason": "provider disabled; no live billing receipt; unavailable amounts are not zero",
                                     "network_filter": "requested_wrapper" if self.network_wrapper else "not_measured"},
                   "storage": self.storage, "session_count": len(self.sessions),
                   "platform": {"sys_platform": sys.platform, "machine": os.uname().machine,
                                "python": sys.version}, "driver_resource_usage": self_usage()}
        artifact_sizes = {"fixture_source": 0, "fixture_storage": 0, "raw_and_reports": 0}
        for path in self.output.rglob("*"):
            if path.is_file():
                relative = path.relative_to(self.output)
                kind = ("fixture_storage" if ".codecortex" in relative.parts or ".cache" in relative.parts
                        else "fixture_source" if relative.parts[0] == "fixtures" else "raw_and_reports")
                artifact_sizes[kind] += path.stat().st_size
        receipt["artifact_bytes_observed"] = {"components": artifact_sizes,
                                               "total": sum(artifact_sizes.values()),
                                               "scope": "logical file lengths before writing the final receipt; not an allocated-disk or archive-complete total"}
        receipt["exit_code"] = 0 if receipt["status"] == "complete_observation" else 3 if self.cancelled else code or 1
        self.events.flush()
        receipt["artifact_inventory"] = artifact_inventory(self.output)
        receipt["artifact_inventory_scope"] = "all retained run files except this receipt; original raw/cache/lifecycle/transport/source/DB/replay bytes; verify before review or replay"
        write_json(self.output / "receipt.json", receipt)
        verify_artifact_inventory(self.output, receipt)
        return receipt


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "verify":
        parser = argparse.ArgumentParser(description="Verify the sealed original lifecycle raw artifact inventory")
        parser.add_argument("--output-dir", type=Path, required=True)
        args = parser.parse_args(sys.argv[2:])
        receipt = json.loads((args.output_dir / "receipt.json").read_text())
        verify_artifact_inventory(args.output_dir, receipt)
        print(json.dumps({"status": "sealed_raw_verified", "files": len(receipt["artifact_inventory"]),
                          "receipt_sha256": digest(args.output_dir / "receipt.json")}))
        return 0
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--build-receipt", type=Path, required=True)
    parser.add_argument("--evaluator", type=Path, required=True)
    parser.add_argument("--evaluator-build-receipt", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--package-kind", choices=("default", "semantic"), default="default")
    parser.add_argument("--profile", choices=("smoke", "release"), default="smoke")
    parser.add_argument("--query-samples", type=int, default=400)
    parser.add_argument("--cold-samples", type=int, default=30)
    parser.add_argument("--files", type=int, default=32)
    parser.add_argument("--request-timeout-seconds", type=int, default=30)
    parser.add_argument("--run-timeout-seconds", type=int, default=3600)
    parser.add_argument("--artifact-budget-bytes", type=int, default=256 * 1024 * 1024)
    parser.add_argument("--deny-network-wrapper", type=Path)
    args = parser.parse_args()
    plan = validate_plan({key: getattr(args, key) for key in
                          ("profile", "query_samples", "cold_samples", "files", "request_timeout_seconds",
                           "run_timeout_seconds", "artifact_budget_bytes")})
    identity = binary_identity(args.binary, args.build_receipt, args.package_kind)
    verify_current_source(identity)
    if args.profile == "release":
        build = json.loads(args.build_receipt.read_text())
        verify_release_profile(build["cargo_artifact"]["profile"])
        require(identity["build_profile"] == "release", "product receipt is not an actual release build")
    replay_identity = evaluator_identity(args.evaluator, args.evaluator_build_receipt, identity,
                                         release=args.profile == "release")
    output = new_directory(args.output_dir)
    driver = Driver(identity, args.evaluator.resolve(strict=True), output, plan, args.deny_network_wrapper,
                    evaluator_build=replay_identity)
    write_json(output / "plan.json", {"plan": plan, "product": identity, "configuration": CONFIG,
                                     "evaluator_sha256": driver.evaluator_digest, "evaluator_build": replay_identity,
                                     "observer_sha256": driver.runner_digests})
    previous_handler = signal.signal(signal.SIGINT, lambda *_: setattr(driver, "cancelled", True))
    try:
        try:
            driver.execute()
        except BaseException as error:
            driver.failures.append({"type": type(error).__name__, "message": str(error)})
            driver.event("run_failed", failures=driver.failures)
        try:
            receipt = driver.finish()
        except BaseException as error:
            receipt = {"schema_version": 1, "status": "invalid_measurement", "exit_code": 2,
                       "error": f"{type(error).__name__}: {error}", "failures": driver.failures,
                       "release_certified": False, "partial_artifacts_retained": True}
            write_json(output / "failure-receipt.json", receipt)
        print(json.dumps({"status": receipt["status"], "exit_code": receipt["exit_code"], "output": str(output)}))
        return receipt["exit_code"]
    finally:
        signal.signal(signal.SIGINT, previous_handler)
        driver.events.close()


if __name__ == "__main__":
    sys.exit(main())
