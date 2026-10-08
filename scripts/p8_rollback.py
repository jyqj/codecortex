#!/usr/bin/env python3
"""P8-016: exercise a bounded rollback on a newly owned synthetic project.

Never accepts an existing user project or database. Both products require their
original build receipts. A future schema number is an injected fault, not a new
released format. Unknown-format cache reader and release-pair certification are
reported separately from this executable, offline local drill.
"""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys
import time

from p8_cold_build import digest, new_directory, write_json
from resource_harness.runtime import Journal, StdioRPC


MARKER = "p8_rollback_source_marker"
FIXTURE_FILES = {
    "Cargo.toml": '[package]\nname = "p8-rollback-owned-fixture"\nversion = "0.1.0"\nedition = "2021"\n',
    "src/lib.rs": f"pub fn {MARKER}(value: u32) -> u32 {{ value + 17 }}\n",
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def product_manifest_hashes(path, source):
    """Read either producer's bound manifest without discarding duplicate paths."""
    raw = Path(path).read_bytes()
    require(hashlib.sha256(raw).hexdigest() == source.get("manifest_sha256"),
            "product source manifest digest mismatch")

    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate key in product source manifest")
            result[key] = value
        return result

    manifest = json.loads(raw, object_pairs_hook=unique_object)
    if isinstance(manifest, dict):
        entries = list(manifest.items())
    else:
        require(isinstance(manifest, list) and all(isinstance(row, dict) for row in manifest),
                "product source manifest must be a digest map or cold-build entry list")
        entries = [(row.get("path"), row.get("sha256")) for row in manifest]
    hashes = {}
    for name, value in entries:
        require(isinstance(name, str) and "\0" not in name and Path(name).parts
                and not Path(name).is_absolute()
                and ".." not in Path(name).parts and Path(name).as_posix() == name,
                "invalid path in product source manifest")
        require(name not in hashes, "duplicate path in product source manifest")
        require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value),
                "invalid digest in product source manifest")
        hashes[name] = value
    require(type(source.get("input_count")) is int and len(hashes) == source["input_count"],
            "product source manifest count mismatch")
    return hashes


def binary_identity(binary, receipt_path, package):
    """Bind existing bytes to the supplied historical build witness.

    Does not relabel an older receipt as the current checkout. Full compiler and
    source revalidation belongs to the producer/cold-build gate.
    """
    binary = Path(binary).resolve(strict=True)
    receipt_path = Path(receipt_path).resolve(strict=True)
    receipt = json.loads(receipt_path.read_text())
    require(receipt.get("build_exit_code") == 0, "product receipt build did not succeed")
    require(receipt.get("stop_reason") is None and receipt.get("guard_stop") is None,
            "stopped product build cannot be used for rollback")
    require(receipt.get("package_kind", receipt.get("cell", {}).get("package")) == package,
            "product receipt package kind mismatch")
    require(receipt.get("status", "passed") == "passed", "product receipt is not passed")
    require(receipt.get("binary_sha256") == digest(binary), "product binary digest mismatch")
    command = receipt.get("build_command", receipt.get("command", []))
    require("--locked" in command and "--no-default-features" in command,
            "product build must be locked and explicitly feature-selected")
    artifact = receipt.get("cargo_artifact", {})
    require(artifact.get("target", {}).get("name") == "codecortex"
            and artifact.get("target", {}).get("kind") == ["bin"],
            "product receipt does not describe the codecortex binary")
    require(isinstance(artifact.get("features"), list)
            and sorted(artifact["features"]) == ([] if package == "default" else ["semantic"]),
            "product receipt artifact feature mismatch")
    before = receipt.get("source_before")
    require(isinstance(before, dict) and before == receipt.get("source_after"),
            "product receipt source binding missing or changed during build")
    require(re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", before.get("source_commit", "")),
            "product source commit is missing")
    manifest_name = receipt.get("source_manifest", "source-inputs.json")
    require(isinstance(manifest_name, str) and Path(manifest_name).name == manifest_name,
            "source manifest must be a sibling filename")
    manifest = receipt_path.parent / manifest_name
    product_manifest_hashes(manifest, before)
    return dict(binary_path=str(binary), binary_sha256=digest(binary), package_kind=package,
                receipt_path=str(receipt_path), receipt_sha256=digest(receipt_path),
                source=before, source_manifest_path=str(manifest),
                build_profile=receipt.get("build_profile", receipt.get("profile")),
                claim="existing product bound to historical build receipt; not current-source certification")


def file_manifest(root):
    root = Path(root)
    if not root.exists():
        return {}
    result = {}
    for path in sorted(root.rglob("*")):
        require(not path.is_symlink(), "fixture/cache symlinks are not admitted")
        if path.is_file():
            result[path.relative_to(root).as_posix()] = digest(path)
    return result


def source_manifest(project):
    # Snapshot every authored source/configuration file except the intentionally
    # switched product config and derived .codecortex cache. New or deleted user
    # source files cannot hide behind a hard-coded expected file list.
    result = {}
    for path in sorted(Path(project).rglob("*")):
        name = path.relative_to(project).as_posix()
        # Exclude before opening: raw open/close of the SQLite file in a
        # process holding POSIX locks can release that process's lock. Derived
        # cache bytes are neither authored source nor part of this manifest.
        if name == ".codecortex" or name.startswith(".codecortex/") or name == ".codecortex.json":
            continue
        require(not path.is_symlink(), "authored source symlinks are not admitted")
        if path.is_file():
            result[name] = digest(path)
    return result


def db_snapshot(path):
    require(Path(path).is_file() and not Path(path).is_symlink(), "owned database is absent or a symlink")
    with closing(sqlite3.connect(Path(path).as_uri() + "?mode=ro", uri=True, timeout=1)) as connection:
        tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
        return dict(schema_version=connection.execute("PRAGMA user_version").fetchone()[0],
                    integrity=connection.execute("PRAGMA integrity_check").fetchone()[0],
                    foreign_key_errors=len(connection.execute("PRAGMA foreign_key_check").fetchall()),
                    indexed_files=connection.execute("SELECT COUNT(*) FROM files").fetchone()[0],
                    future_sentinel_present="p8_future_only" in tables)


def backup_database(source, destination, timeout_seconds=10):
    """SQLite backup includes committed WAL pages; never copies a live main file."""
    destination = Path(destination)
    require(not destination.exists() and not destination.is_symlink(), "backup destination already exists")
    # Reserve the filename atomically before SQLite opens it.
    with destination.open("xb"):
        pass
    deadline = time.monotonic() + timeout_seconds
    def progress(_status, _remaining, _total):
        if time.monotonic() >= deadline:
            raise TimeoutError("SQLite backup exceeded its bounded deadline")
    try:
        with closing(sqlite3.connect(Path(source).as_uri() + "?mode=ro", uri=True, timeout=1)) as old:
            with closing(sqlite3.connect(destination, timeout=1)) as backup:
                old.backup(backup, pages=128, progress=progress, sleep=0.05)
        require(db_snapshot(destination)["integrity"] == "ok", "database backup is not integral")
    except BaseException:
        # Keep the incomplete backup as failure evidence; a rerun needs a new dir.
        raise
    return dict(path=str(destination), sha256=digest(destination), snapshot=db_snapshot(destination))


def validate_rebuild(before, after):
    require(before["future_sentinel_present"], "fault injection did not persist the future-only sentinel")
    require(after["schema_version"] != before["schema_version"], "future schema was reused without rebuild")
    require(not after["future_sentinel_present"], "future-only table survived controlled rebuild")
    require(after["integrity"] == "ok" and after["foreign_key_errors"] == 0,
            "rebuilt database integrity failed")
    require(after["indexed_files"] > 0, "rebuilt database lost indexed source")


class Product:
    """Owned sequential stdio calls using the existing RPC transport implementation."""
    def __init__(self, identity, project, output, cache_root, network_wrapper=None):
        self.output = new_directory(output)
        self.project = project
        self.identity = identity
        # Retain only basic runtime paths/locale, never ambient provider secrets,
        # user CODECORTEX overrides, proxies, or a configured network opt-in.
        env = {key: value for key, value in os.environ.items()
               if key in ("PATH", "HOME", "LANG", "LC_ALL", "TMPDIR", "SYSTEMROOT")}
        env.update(CODECORTEX_PPID_POLL_MS="0", CODECORTEX_SEMANTIC_CACHE_ROOT=str(cache_root),
                   RAYON_NUM_THREADS="2", TOKIO_WORKER_THREADS="2")
        command = [identity["binary_path"], "mcp", "--project-path", str(project)]
        if network_wrapper:
            command = [sys.executable, str(network_wrapper), "--receipt",
                       str(self.output / "network.json"), "--", *command]
        self.record = dict(command=command, binary_sha256=identity["binary_sha256"],
                           environment={key: env[key] for key in ("CODECORTEX_PPID_POLL_MS",
                           "CODECORTEX_SEMANTIC_CACHE_ROOT", "RAYON_NUM_THREADS", "TOKIO_WORKER_THREADS")},
                           cleanup="pending")
        self.raw = (self.output / "rpc.jsonl").open("x")
        self.stderr = (self.output / "product-stderr.log").open("x")
        try:
            require(digest(identity["binary_path"]) == identity["binary_sha256"], "binary changed before launch")
            self.process = subprocess.Popen(command, cwd=project, env=env, stdin=subprocess.PIPE,
                                             stdout=subprocess.PIPE, stderr=self.stderr, text=True,
                                             close_fds=True)
        except BaseException:
            self.raw.close()
            self.stderr.close()
            raise
        self.record["pid"] = self.process.pid
        self.transport = StdioRPC(self.process, Journal(self.raw), phase=lambda: self.output.name)
        write_json(self.output / "process.json", self.record)

    def rpc(self, method, params, timeout=45):
        return self.transport.rpc(method, params, timeout=timeout)

    def tool(self, name, arguments, timeout=45):
        response = self.rpc("tools/call", dict(name=name, arguments=arguments), timeout=timeout)
        require(response.get("isError") is not True, f"{name} returned a tool error: {response}")
        require(isinstance(response.get("structuredContent"), dict), f"{name} lacks structured content")
        value = response["structuredContent"]
        return value.get("result", value)

    def initialize(self):
        if self.record.get("initialized"):
            self.transport.pending.raise_if_terminal()
            return self.record["tool_count"]
        self.rpc("initialize", dict(protocolVersion="2024-11-05", capabilities={},
                                    clientInfo=dict(name="p8-rollback", version="1")))
        with self.transport.write_lock:
            notification = dict(jsonrpc="2.0", method="notifications/initialized")
            self.transport.journal.emit("notification", payload=notification)
            self.process.stdin.write(json.dumps(notification) + "\n")
            self.process.stdin.flush()
        tools = self.rpc("tools/list", {})["tools"]
        require(len(tools) == 14, "existing 14-tool MCP surface changed")
        self.transport.pending.raise_if_terminal()
        self.record.update(initialized=True, tool_count=len(tools))
        return len(tools)

    def verify_local(self, full=True):
        tool_count = self.initialize()
        indexed = self.tool("index", dict(path=str(self.project), full=full))
        if full:
            require(indexed.get("files_scanned", 0) > 0, "full rebuild did not scan source")
        capability = self.tool("status", dict(aspect="capabilities", project_path=str(self.project)))
        require(capability.get("capabilities", {}).get("search") is True, "local search is not available")
        retrieval = capability.get("retrieval", {})
        require(retrieval.get("semantic_state") == "not_configured", "disabled semantic state is dishonest")
        require(retrieval.get("dense_state") == "disabled", "disabled dense state is dishonest")
        search = self.tool("search", dict(query=MARKER, mode="symbol", top_k=5, project_path=str(self.project)))
        verify_fixture_search(self.project, search)
        result = dict(tool_count=tool_count, index=indexed, capabilities=capability, search=search)
        write_json(self.output / "local.json", result)
        return result

    def close(self, *, expected_exit_code=0):
        error = None
        try:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                if expected_exit_code == 0:
                    raise
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.terminate()  # Only this owned child, never a process scan.
                try:
                    self.process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=10)
                error = "product did not exit after stdin EOF"
            self.transport.reader.join(timeout=10)
            self.transport.exit_watcher.join(timeout=10)
            if self.transport.reader.is_alive() or self.transport.exit_watcher.is_alive():
                raise RuntimeError("owned stdio reader did not close; logs retained")
            self.transport.join()
            require(digest(self.identity["binary_path"]) == self.identity["binary_sha256"], "binary changed during run")
            self.record["exit_code"] = self.process.returncode
            self.record["expected_exit_code"] = expected_exit_code
            self.record["cleanup"] = "completed" if error is None else "forced_after_timeout"
            require(self.process.returncode == expected_exit_code and error is None,
                    error or "product exit differs from the explicit expected code")
            network_path = self.output / "network.json"
            if network_path.exists():
                network = json.loads(network_path.read_text())
                require(network.get("result") == "filter_loaded_and_probes_passed_before_exec"
                        and network.get("ipv4", {}).get("blocked") is True
                        and network.get("ipv6", {}).get("blocked") is True,
                        "network wrapper did not prove its filter/probes")
                self.record["network_receipt_sha256"] = digest(network_path)
        finally:
            write_json(self.output / "process.json", self.record)
            if not self.transport.reader.is_alive() and not self.transport.exit_watcher.is_alive():
                self.raw.close()
                self.stderr.close()
                self.process.stdout.close()


def exercise(identity, project, output, cache, wrapper, full=True):
    product = Product(identity, project, output, cache, wrapper)
    try:
        return product.verify_local(full=full)
    finally:
        product.close()


def product_schema_version(identity):
    """Read the schema contract from bytes already bound by the build manifest."""
    relative = "crates/cc-db/src/index_migrate.rs"
    manifest = product_manifest_hashes(identity["source_manifest_path"], identity["source"])
    root = Path(identity["source"]["source_root"]).resolve(strict=True)
    path = root / relative
    require(path.is_file() and path.resolve() == path,
            "schema source differs from the bound product manifest")
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == manifest.get(relative),
            "schema source differs from the bound product manifest")
    matches = re.findall(r"^pub const CURRENT_SCHEMA_VERSION: u32 = ([0-9]+);$", raw.decode(), re.MULTILINE)
    require(len(matches) == 1, "one exact production schema version is required")
    return int(matches[0])


def version_pair_contract(current, previous):
    require(current["source"]["source_commit"] != previous["source"]["source_commit"],
            "version rollback requires two distinct actual source revisions")
    current_version, previous_version = product_schema_version(current), product_schema_version(previous)
    require(0 < previous_version < current_version, "previous product must have an actually older schema")
    return dict(current=current_version, previous=previous_version)


def verify_fixture_search(project, response):
    require(isinstance(response, (list, dict)), "public query requires a structured result list")
    rows = response if isinstance(response, list) else response.get("results", response.get("hits"))
    require(isinstance(rows, list) and all(isinstance(row, dict) for row in rows),
            "public query requires a structured result list")
    hits = [row for row in rows if row.get("name") == MARKER or row.get("qname") == MARKER]
    require(bool(hits), "public query has no exact authored symbol hit")
    for hit in hits:
        require(hit.get("file_path") == "src/lib.rs", "public symbol path differs from its authored file")
        lines = (project / "src/lib.rs").read_text().splitlines()
        start, end = hit.get("start_line"), hit.get("end_line")
        require(type(start) is int and type(end) is int and 1 <= start <= end <= len(lines),
                "public symbol source span is invalid")
        require(f"fn {MARKER}(" in "\n".join(lines[start - 1:end]),
                "public symbol span does not define the authored symbol")
    return dict(matching_symbols=len(hits), file_path="src/lib.rs",
                source_sha256=digest(project / "src/lib.rs"))


def run_version_pair(current, previous, output):
    """Open a real newer product's DB with an unmodified older source build.

    Both database formats come from actual product executions. This does not
    inject a schema number, publish a release, or relabel source commits as tags.
    """
    output = Path(output)
    record = dict(schema_version=1, task="P8-016", status="failed", cases=[],
                  products=dict(current=current, previous=previous), runner_sha256=digest(__file__),
                  scope="actual public source revision rollback; no released-package certification",
                  schema_fault_injection=False, released_version_pair=False, release_certified=False)
    try:
        versions = version_pair_contract(current, previous)
        record["production_schema_versions"] = versions
        for label, identity in record["products"].items():
            witness = new_directory(output / f"{label}-product")
            for source, name in ((identity["binary_path"], "codecortex"),
                                 (identity["receipt_path"], "build-receipt.json"),
                                 (identity["source_manifest_path"], "source-inputs.json")):
                shutil.copy2(source, witness / name)
            require(digest(witness / "codecortex") == identity["binary_sha256"],
                    "retained version product differs from build identity")
        project = new_directory(output / "fixture")
        (project / "src").mkdir()
        for name, contents in FIXTURE_FILES.items():
            (project / name).write_text(contents)
        config = project / ".codecortex.json"
        write_json(config, {"auto_index": {"enabled": False}, "semantic": {"enabled": False}})
        source_before, config_before = source_manifest(project), digest(config)
        write_json(output / "source-before.json", source_before)
        backups = new_directory(output / "backups")
        shutil.copy2(config, backups / "config-original.json")
        database = project / ".codecortex/index.sqlite3"

        def observe(identity, label, expected_version, *, full=False, mismatch=False):
            product = Product(identity, project, output / label, output / f"cache-{label}")
            try:
                product.initialize()
                indexed = product.tool("index", {"path": str(project), "full": full}, timeout=120)
                require(indexed.get("parse_errors") == [], "version-pair fixture parse errors or missing report")
                capabilities = product.tool("status", {"aspect": "capabilities"})
                require(capabilities.get("capabilities", {}).get("search") is True,
                        "version-pair local search is not available")
                retrieval = capabilities.get("retrieval", {})
                require(retrieval.get("semantic_state") == "not_configured"
                        and retrieval.get("dense_state") == "disabled", "version-pair semantic disable fallback failed")
                search = product.tool("search", {"query": MARKER, "mode": "symbol", "top_k": 5})
                result = dict(index=indexed, capabilities=capabilities, search=search,
                              public_source=verify_fixture_search(project, search))
                write_json(output / label / "local.json", result)
            finally:
                product.close()
            state = db_snapshot(database)
            require(state["schema_version"] == expected_version and state["integrity"] == "ok"
                    and state["foreign_key_errors"] == 0 and state["indexed_files"] > 0,
                    "version-pair database schema/integrity/local content differs")
            stderr = output / label / "product-stderr.log"
            mismatch_count = stderr.read_text().count("index schema version mismatch, rebuild required")
            if mismatch:
                require(mismatch_count > 0, "older/newer schema was not explicitly rejected before rebuilding")
            require(source_manifest(project) == source_before and digest(config) == config_before,
                    "version rollback changed authored source or original config")
            return dict(database=state, database_sha256=digest(database), local=result,
                        schema_mismatch_diagnostics=mismatch_count, stderr_sha256=digest(stderr))

        first = observe(current, "01-current-build", versions["current"], full=True)
        current_backup = backup_database(database, backups / "current-original.sqlite3")
        record["current_database_backup"] = current_backup
        # The old process receives exactly the database created above. No SQL,
        # schema number, vector format, or source bytes are altered between them.
        require(digest(database) == first["database_sha256"], "new product database changed before old open")
        older = observe(previous, "02-previous-opens-current", versions["previous"], mismatch=True)
        record["cases"].append(dict(id="actual_previous_binary_rebuilds_newer_schema", status="passed",
                                     before=first, after=older, user_version_injection=False,
                                     source_commit=previous["source"]["source_commit"]))
        record["previous_database_backup"] = backup_database(database, backups / "previous-rebuilt.sqlite3")
        newer = observe(current, "03-current-restored", versions["current"], mismatch=True)
        record["cases"].append(dict(id="current_binary_restored_after_actual_downgrade", status="passed", result=newer))
        record["current_rebuilt_backup"] = backup_database(database, backups / "current-rebuilt.sqlite3")
        staging = database.parent / "restore-version-pair.sqlite3"
        backup_database(backups / "current-original.sqlite3", staging)
        for suffix in ("-wal", "-shm"):
            sidecar = database.with_name(database.name + suffix)
            require(not sidecar.is_symlink(), "owned version database sidecar became a symlink")
            sidecar.unlink(missing_ok=True)
        staging.replace(database)
        restored = observe(current, "04-original-backup-restored", versions["current"])
        require(digest(backups / "current-original.sqlite3") == current_backup["sha256"],
                "original current database backup changed during downgrade/recovery")
        require(digest(backups / "config-original.json") == config_before, "original configuration backup changed")
        for identity in (current, previous):
            require(digest(identity["binary_path"]) == identity["binary_sha256"], "actual version product changed")
        record["cases"].append(dict(id="actual_current_database_backup_restore", status="passed",
                                     original_backup_sha256=current_backup["sha256"], result=restored))
        record.update(status="passed_actual_source_version_pair", source_files=source_before,
                      source_unchanged=True, configuration_sha256=config_before)
    except BaseException as error:
        record["error"] = f"{type(error).__name__}: {error}"
    finally:
        record["evidence_files_sha256"] = {name: value for name, value in file_manifest(output).items()
                                           if name != "version-pair.json" and not name.startswith("fixture/.codecortex/")}
        write_json(output / "version-pair.json", record)
    return record


def run_drill(default, semantic, output, network_wrapper=None):
    record = dict(schema_version=1, task="P8-016", status="failed", runner_sha256=digest(__file__),
                  runner_dependencies_sha256={name: digest(Path(__file__).parent / name) for name in
                                              ("p8_cold_build.py", "resource_harness/runtime.py")},
                  products=dict(default=default, semantic=semantic), cases=[],
                  scope="synthetic future-schema injection with existing receipt-bound product packages; no release certification",
                  limitations=["not an actual released new-schema/old-release pair",
                               "unknown cache format reader behavior is not exercised with semantic disabled",
                               "current checkout product was not rebuilt by this drill"])
    try:
        for package, identity in (("default", default), ("semantic", semantic)):
            copy_dir = new_directory(output / f"{package}-build-witness")
            shutil.copyfile(identity["receipt_path"], copy_dir / "build-receipt.json")
            shutil.copyfile(identity["source_manifest_path"], copy_dir / "source-inputs.json")
        if network_wrapper:
            network_wrapper = Path(network_wrapper).resolve(strict=True)
            copy = output / "network-wrapper.py"
            shutil.copyfile(network_wrapper, copy)
            record["network_policy"] = dict(kind="inherited_process_seccomp", wrapper_sha256=digest(copy))
            network_wrapper = copy
        else:
            record["network_policy"] = dict(kind="disabled_config_only", enforcement="not_measured")
            record["limitations"].append("network syscalls were not independently denied/observed")
        project = new_directory(output / "fixture")
        (project / "src").mkdir()
        for name, text in FIXTURE_FILES.items():
            (project / name).write_text(text)
        config = project / ".codecortex.json"
        original_config = dict(semantic=dict(enabled=False, model_id="future/unavailable",
                               endpoint="http://127.0.0.1:9/v1", api_key_ref="env:P8_NOT_PROVISIONED"))
        write_json(config, original_config)
        backups = new_directory(output / "backups")
        shutil.copyfile(config, backups / "config-before.json")
        config_hash = digest(config)
        source_before = source_manifest(project)
        write_json(output / "source-before.json", source_before)
        cache_parent = new_directory(output / "semantic-cache")
        future_cache = new_directory(cache_parent / "future-format-999")
        rollback_cache = cache_parent / "rollback-format-1"
        (future_cache / "future.bin").write_bytes(b"synthetic future vector format must remain opaque\n")
        write_json(future_cache / "future.meta.json", dict(format_version=999, marker="future cache must be retained"))
        cache_before = file_manifest(future_cache)
        require(not rollback_cache.exists(), "rollback cache unexpectedly exists before disabled launch")
        exercise(semantic, project, output / "01-before", rollback_cache, network_wrapper)
        database = project / ".codecortex/index.sqlite3"
        old_state = db_snapshot(database)
        require(old_state["schema_version"] > 0, "initial product did not create a schema")
        record["old_database"] = backup_database(database, backups / "index-old.sqlite3")
        # Mutate only this owned, stopped database. Unsupported future schema
        # version + a sentinel make accidental old-format reuse observable.
        future_version = old_state["schema_version"] + 1000
        with closing(sqlite3.connect(database, timeout=1)) as connection:
            with connection:
                connection.execute(f"PRAGMA user_version={future_version}")
                connection.execute("CREATE TABLE p8_future_only(value TEXT NOT NULL)")
                connection.execute("INSERT INTO p8_future_only VALUES ('future rows must not be read as the old schema')")
        future_state = db_snapshot(database)
        record["future_database"] = backup_database(database, backups / "index-future.sqlite3")
        candidate_config = json.loads(json.dumps(original_config))
        candidate_config["semantic"]["enabled"] = True
        write_json(config, candidate_config)
        shutil.copyfile(config, backups / "config-candidate.json")
        # Restore the previously verified disabled configuration BEFORE any
        # candidate code can run. An enabled provider is never launched here.
        shutil.copyfile(backups / "config-before.json", config)
        require(digest(config) == config_hash, "configuration rollback changed original bytes")
        started = time.monotonic()
        exercise(semantic, project, output / "02-schema-rollback", rollback_cache, network_wrapper)
        rebuilt_state = db_snapshot(database)
        validate_rebuild(future_state, rebuilt_state)
        require(rebuilt_state["schema_version"] == old_state["schema_version"], "rollback did not restore the old schema")
        record["cases"].append(dict(id="future_schema_controlled_rebuild", status="passed",
                                     fault="synthetic PRAGMA user_version increase and future-only table",
                                     before=future_state, after=rebuilt_state,
                                     wall_seconds=time.monotonic() - started))
        exercise(default, project, output / "03-default-package", rollback_cache, network_wrapper, full=False)
        record["cases"].append(dict(id="semantic_to_default_disabled_local", status="passed",
                                     semantic_enabled=False, source_query=MARKER))
        # A reversible recovery fallback: retain rebuilt DB, restore the old
        # SQLite backup into a new staging file, then replace only owned cache.
        record["rebuilt_database"] = backup_database(database, backups / "index-rebuilt.sqlite3")
        staging = database.parent / "restore-staging.sqlite3"
        backup_database(backups / "index-old.sqlite3", staging)
        for suffix in ("-wal", "-shm"):
            sidecar = database.with_name(database.name + suffix)
            require(not sidecar.is_symlink(), "owned database sidecar became a symlink")
            sidecar.unlink(missing_ok=True)
        staging.replace(database)
        exercise(default, project, output / "04-backup-restored", rollback_cache, network_wrapper, full=False)
        require(db_snapshot(database)["schema_version"] == old_state["schema_version"], "old backup restore schema mismatch")
        require(digest(backups / "index-old.sqlite3") == record["old_database"]["sha256"], "old database backup was changed")
        record["cases"].append(dict(id="old_database_backup_restore", status="passed",
                                     backup_sha256=record["old_database"]["sha256"]))
        require(not rollback_cache.exists(), "disabled product created a semantic cache")
        require(file_manifest(future_cache) == cache_before, "future cache bytes were modified")
        record["cases"].append(dict(id="operator_cache_version_roots", status="passed",
                                     future_root=str(future_cache), rollback_root=str(rollback_cache),
                                     future_files_unchanged=cache_before, rollback_root_created=False,
                                     scope="physical version-root isolation and disabled no-write behavior; not an active reader test"))
        record["cases"].append(dict(id="active_cache_reader_rejects_unknown_format", status="not_run",
                                     reason="requires cc-semantic artifact_cache integration gate or an enabled fake-provider product"))
        source_after = source_manifest(project)
        write_json(output / "source-after.json", source_after)
        require(source_before == source_after, "rollback modified, added or deleted authored source")
        require(digest(config) == config_hash and digest(backups / "config-before.json") == config_hash,
                "configuration or its backup changed")
        for identity in (default, semantic):
            require(digest(identity["binary_path"]) == identity["binary_sha256"], "product package changed during drill")
        record["cases"].append(dict(id="source_and_config_integrity", status="passed",
                                     source_manifest=source_before, config_sha256=config_hash))
        record["status"] = "passed_limited_local_drill"
    except BaseException as error:
        record["error"] = f"{type(error).__name__}: {error}"
        record["status"] = "failed"
    finally:
        try:
            record["evidence_files_sha256"] = {name: value for name, value in file_manifest(output).items()
                                               if name != "rollback.json" and not name.startswith("fixture/.codecortex/")}
        except (OSError, ValueError) as error:
            record["evidence_scan_error"] = f"{type(error).__name__}: {error}"
            record["status"] = "failed"
        write_json(output / "rollback.json", record)
    return record


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version-pair", action="store_true")
    parser.add_argument("--default-binary", type=Path, required=True)
    parser.add_argument("--default-receipt", type=Path, required=True)
    parser.add_argument("--semantic-binary", type=Path)
    parser.add_argument("--semantic-receipt", type=Path)
    parser.add_argument("--previous-binary", type=Path)
    parser.add_argument("--previous-receipt", type=Path)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--deny-network-wrapper", type=Path)
    args = parser.parse_args(argv)
    try:
        default = binary_identity(args.default_binary, args.default_receipt, "default")
        if args.version_pair:
            if not args.previous_binary or not args.previous_receipt:
                parser.error("version pair requires --previous-binary and --previous-receipt")
            if args.deny_network_wrapper:
                parser.error("version-pair mode uses disabled default products; syscall denial is not asserted")
            previous = binary_identity(args.previous_binary, args.previous_receipt, "default")
            output = new_directory(args.output_dir)
            record = run_version_pair(default, previous, output)
            print(json.dumps(dict(status=record["status"], error=record.get("error"),
                                  receipt=str(output / "version-pair.json"))))
            return 0 if record["status"] == "passed_actual_source_version_pair" else 1
        if not args.semantic_binary or not args.semantic_receipt:
            parser.error("injected-schema drill requires --semantic-binary and --semantic-receipt")
        semantic = binary_identity(args.semantic_binary, args.semantic_receipt, "semantic")
        output = new_directory(args.output_dir)
        record = run_drill(default, semantic, output, args.deny_network_wrapper)
        print(json.dumps(dict(status=record["status"], receipt=str(output / "rollback.json"),
                              cases={case["id"]: case["status"] for case in record["cases"]})))
        return 0 if record["status"] == "passed_limited_local_drill" else 1
    except (OSError, ValueError, KeyError) as error:
        print(f"P8 rollback refused: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
