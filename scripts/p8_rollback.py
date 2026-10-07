#!/usr/bin/env python3
"""P8-016: exercise a bounded rollback on a newly owned synthetic project.

Never accepts an existing user project or database. Both products require their
original build receipts. A future schema number is an injected fault, not a new
released format. Unknown-format cache reader and release-pair certification are
reported separately from this executable, offline local drill.
"""
import argparse
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
    require(digest(manifest) == before.get("manifest_sha256"), "product source manifest digest mismatch")
    require(len(json.loads(manifest.read_text())) == before.get("input_count"),
            "product source manifest count mismatch")
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
    return {name: value for name, value in file_manifest(project).items()
            if not name.startswith(".codecortex/") and name != ".codecortex.json"}


def db_snapshot(path):
    require(Path(path).is_file() and not Path(path).is_symlink(), "owned database is absent or a symlink")
    with sqlite3.connect(Path(path).as_uri() + "?mode=ro", uri=True, timeout=1) as connection:
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
        with sqlite3.connect(Path(source).as_uri() + "?mode=ro", uri=True, timeout=1) as old:
            with sqlite3.connect(destination, timeout=1) as backup:
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

    def rpc(self, method, params):
        return self.transport.rpc(method, params, timeout=45)

    def tool(self, name, arguments):
        response = self.rpc("tools/call", dict(name=name, arguments=arguments))
        require(response.get("isError") is not True, f"{name} returned a tool error: {response}")
        require(isinstance(response.get("structuredContent"), dict), f"{name} lacks structured content")
        value = response["structuredContent"]
        return value.get("result", value)

    def verify_local(self, full=True):
        self.rpc("initialize", dict(protocolVersion="2024-11-05", capabilities={},
                                    clientInfo=dict(name="p8-rollback", version="1")))
        with self.transport.write_lock:
            notification = dict(jsonrpc="2.0", method="notifications/initialized")
            self.transport.journal.emit("notification", payload=notification)
            self.process.stdin.write(json.dumps(notification) + "\n")
            self.process.stdin.flush()
        tools = self.rpc("tools/list", {})["tools"]
        require(len(tools) == 14, "existing 14-tool MCP surface changed")
        indexed = self.tool("index", dict(path=str(self.project), full=full))
        if full:
            require(indexed.get("files_scanned", 0) > 0, "full rebuild did not scan source")
        capability = self.tool("status", dict(aspect="capabilities", project_path=str(self.project)))
        require(capability.get("capabilities", {}).get("search") is True, "local search is not available")
        retrieval = capability.get("retrieval", {})
        require(retrieval.get("semantic_state") == "not_configured", "disabled semantic state is dishonest")
        require(retrieval.get("dense_state") == "disabled", "disabled dense state is dishonest")
        search = self.tool("search", dict(query=MARKER, mode="symbol", top_k=5, project_path=str(self.project)))
        # Assert the symbol is in a returned result, not merely an echoed query.
        results = search.get("results", search.get("hits", [])) if isinstance(search, dict) else search
        require(isinstance(results, list) and any(MARKER in json.dumps(hit) for hit in results),
                f"local query did not return the authored symbol: {search}")
        result = dict(tool_count=len(tools), index=indexed, capabilities=capability, search=search)
        write_json(self.output / "local.json", result)
        return result

    def close(self):
        error = None
        try:
            self.process.stdin.close()
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
            self.record["cleanup"] = "completed" if error is None else "forced_after_timeout"
            require(self.process.returncode == 0 and error is None, error or "product exited nonzero")
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
        with sqlite3.connect(database, timeout=1) as connection:
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
    parser.add_argument("--default-binary", type=Path, required=True)
    parser.add_argument("--default-receipt", type=Path, required=True)
    parser.add_argument("--semantic-binary", type=Path, required=True)
    parser.add_argument("--semantic-receipt", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--deny-network-wrapper", type=Path)
    args = parser.parse_args(argv)
    try:
        default = binary_identity(args.default_binary, args.default_receipt, "default")
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
