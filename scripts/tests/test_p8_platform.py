"""P8 platform/rollback contract controls; synthetic compiler is never product evidence."""
import contextlib
import io
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_cold_build as cold
import p8_rollback as rollback


class PlatformControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="p8-platform-contract-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.root = self.base / "source"
        (self.root / "crates/cc-server/src").mkdir(parents=True)
        for name, value in {
            "Cargo.toml": '[workspace]\nmembers=["crates/cc-server"]\n',
            "Cargo.lock": "version = 4\n",
            "crates/cc-server/Cargo.toml": '[package]\nname="cc-server"\nversion="0.1.0"\n',
            "crates/cc-server/src/main.rs": "fn main() {}\n",
        }.items():
            (self.root / name).write_text(value)
        self.git("init", "-q")
        self.git("config", "user.name", "P8 contract test")
        self.git("config", "user.email", "p8-contract@example.invalid")
        self.git("add", ".")
        self.git("commit", "-qm", "synthetic source contract")
        self.counter = 0

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True).strip()

    def toolchain(self, failure=False):
        self.counter += 1
        directory = self.base / f"toolchain-{self.counter}"
        directory.mkdir()
        cargo = directory / "cargo-contract-fake"
        if failure:
            code = "import sys\nprint('synthetic compiler failure', file=sys.stderr)\nsys.exit(7)\n"
        else:
            code = '''import json, os, pathlib, sys
args=sys.argv[1:]
target=pathlib.Path(args[args.index('--target-dir')+1])
host=args[args.index('--target')+1]
release='--release' in args
path=target/host/('release' if release else 'debug')/'codecortex'
path.parent.mkdir(parents=True)
path.write_bytes(b'CONTRACT FAKE ONLY: not a CodeCortex product')
path.chmod(0o755)
artifact=dict(reason='compiler-artifact', target=dict(name='codecortex',kind=['bin']),
    manifest_path=str(pathlib.Path.cwd()/'crates/cc-server/Cargo.toml'),
    executable=str(path),features=['semantic'] if '--features' in args else [],fresh=False,
    profile=dict(opt_level='3' if release else '0',debug_assertions=not release,test=False))
print(json.dumps(artifact))
print(json.dumps(dict(reason='build-finished',success=True)))
'''
        cargo.write_text(f"#!{sys.executable}\n" + code)
        cargo.chmod(0o755)
        rustc = directory / "rustc-contract-fake"
        rustc.write_text("contract fake rustc identity; never invoked as a real compiler\n")
        rustc.chmod(0o755)
        return dict(cargo=dict(path=str(cargo), sha256=cold.digest(cargo), version_verbose="contract_fake"),
                    rustc=dict(path=str(rustc), sha256=cold.digest(rustc), version_verbose="contract_fake"),
                    matrix_toolchain="1.95", channel="1.95.0", rustc_release="1.95.0",
                    host="aarch64-apple-darwin" if cold.host_platform() == "macos" else "x86_64-unknown-linux-gnu")

    def build_receipt(self, package="default", failure=False):
        tools = self.toolchain(failure)
        output = cold.new_directory(self.base / f"build-{self.counter}")
        record = cold.build_cell(self.root, output,
                                 dict(platform=cold.host_platform(), toolchain="1.95", package=package),
                                 tools, "dev", 1, 10)
        return output / "receipt.json", record

    def rewrite(self, path, record):
        cold.write_json(path, record)

    def rewrite_artifact(self, path, record, mutation):
        log = path.parent / "cargo-build.jsonl"
        lines = [json.loads(line) for line in log.read_text().splitlines()]
        mutation(lines[0])
        log.write_text("".join(json.dumps(row) + "\n" for row in lines))
        record["cargo_artifact"] = lines[0]
        record["logs_sha256"]["cargo-build.jsonl"] = cold.digest(log)
        self.rewrite(path, record)

    def smoke_fixture(self, path, record):
        """Synthetic RPC evidence for parser controls, never product execution."""
        directory = path.parent / "stdio-smoke"
        project = directory / "fixture"
        (project / "src").mkdir(parents=True)
        process = directory / "process"
        process.mkdir()
        for name, text in rollback.FIXTURE_FILES.items():
            (project / name).write_text(text)
        values = {
            "initialize": {"protocolVersion": "2024-11-05"},
            "tools/list": {"tools": [{"name": f"fake-tool-{i}"} for i in range(14)]},
            "index": {"files_scanned": 2},
            "status": {"capabilities": {"search": True}, "retrieval": {
                "semantic_state": "not_configured", "dense_state": "disabled"}},
            "search": [{"name": rollback.MARKER, "file_path": "src/lib.rs", "start_line": 1, "end_line": 1}],
        }
        events = []
        for request_id, (name, value) in enumerate(values.items(), 1):
            tool = name in {"index", "status", "search"}
            request = dict(id=request_id, method="tools/call" if tool else name,
                           params={"name": name} if tool else {})
            response = {"structuredContent": {"result": value} if isinstance(value, list) else value} if tool else value
            events.extend([dict(event="request", payload=request),
                           dict(event="response", payload=dict(id=request_id, result=response))])
        (process / "rpc.jsonl").write_text("".join(json.dumps(row) + "\n" for row in events))
        cold.write_json(process / "process.json", dict(exit_code=0, expected_exit_code=0,
                        cleanup="completed", tool_count=14, binary_sha256=record["binary_sha256"]))
        cold.write_json(process / "local.json", dict(tool_count=14, index=values["index"],
                        capabilities=values["status"], search=values["search"]))
        (process / "product-stderr.log").write_text("SYNTHETIC CONTRACT FIXTURE ONLY\n")
        record["stdio_smoke"] = dict(status="passed", binary_sha256=record["binary_sha256"],
            source_files=rollback.source_manifest(project),
            logs_sha256={p.name: cold.digest(p) for p in process.iterdir()})
        cold.write_json(path, record)

    def bundle(self, parent, cell):
        tools = self.toolchain()
        tools["matrix_toolchain"] = cell["toolchain"]
        tools["channel"] = "1.95.0" if cell["toolchain"] == "1.95" else "stable"
        tools["host"] = "aarch64-apple-darwin" if cell["platform"] == "macos" else "x86_64-unknown-linux-gnu"
        output = cold.new_directory(self.base / f"bundle-build-{self.counter}")
        record = cold.build_cell(self.root, output, cell, tools, "dev", 1, 10)
        self.assertEqual(record["status"], "passed", record)
        path = output / "receipt.json"
        self.smoke_fixture(path, record)
        rows = [dict(cell=dict(platform=os_name, toolchain=tc, package=pkg),
                     status="not_run", reason="platform_unavailable" if os_name != cell["platform"] else "cell_not_selected")
                for os_name in cold.PLATFORMS for tc in cold.TOOLCHAINS for pkg in cold.PACKAGES]
        for row in rows:
            if row["cell"] == cell:
                row.clear()
                row.update(cell=cell, status="passed", receipt=str(path), receipt_sha256=cold.digest(path))
        matrix = output / "selection.json"
        cold.write_json(matrix, dict(source=record["source_before"], cells=rows))
        destination = parent / "-".join(cell.values())
        cold.export_cell(matrix, cell, destination)
        return destination

    def test_selected_cell_requires_stdio_and_does_not_certify_other_seven(self):
        parent = cold.new_directory(self.base / "bundles")
        destination = self.bundle(parent, dict(platform="linux", toolchain="1.95", package="default"))
        with self.assertRaisesRegex(ValueError, "eight-cell.*received 1"):
            cold.collect_cells(parent, self.root, self.git("rev-parse", "HEAD"))
        record = json.loads((destination / "receipt.json").read_text())
        del record["stdio_smoke"]
        with self.assertRaisesRegex(ValueError, "stdio smoke"):
            cold.validate_smoke(destination, record)

    def test_all_eight_synthetic_cells_replay_and_duplicate_is_rejected(self):
        import shutil
        parent = cold.new_directory(self.base / "complete-bundles")
        for os_name in cold.PLATFORMS:
            for tc in cold.TOOLCHAINS:
                for package in cold.PACKAGES:
                    destination = self.bundle(parent, dict(platform=os_name, toolchain=tc, package=package))
        result = cold.collect_cells(parent, self.root, self.git("rev-parse", "HEAD"))
        self.assertEqual(result["counts"], dict(passed=8, failed=0, not_run=0))
        shutil.copytree(destination, parent / "duplicate")
        with self.assertRaisesRegex(ValueError, "duplicate"):
            cold.collect_cells(parent, self.root, self.git("rev-parse", "HEAD"))

    def test_bundle_binary_mutation_and_unrequested_source_fail(self):
        parent = cold.new_directory(self.base / "mutated-bundle")
        destination = self.bundle(parent, dict(platform="linux", toolchain="1.95", package="default"))
        with self.assertRaisesRegex(ValueError, "commit differs"):
            cold.collect_cells(parent, self.root, "0" * 40)
        (destination / "codecortex").write_bytes(b"replaced synthetic binary")
        with self.assertRaisesRegex(ValueError, "byte digest"):
            cold.collect_cells(parent, self.root, self.git("rev-parse", "HEAD"))

    def test_smoke_success_label_cannot_hide_missing_rpc_response(self):
        path, record = self.build_receipt()
        self.smoke_fixture(path, record)
        log = path.parent / "stdio-smoke/process/rpc.jsonl"
        lines = log.read_text().splitlines()
        log.write_text("\n".join(lines[:-1]) + "\n")
        record["stdio_smoke"]["logs_sha256"]["rpc.jsonl"] = cold.digest(log)
        with self.assertRaisesRegex(ValueError, "response is missing"):
            cold.validate_smoke(path.parent, record)

    def test_valid_cold_contract_replays(self):
        path, record = self.build_receipt()
        self.assertEqual(record["status"], "passed", record.get("error"))
        self.assertEqual(cold.validate_cell(path)["binary_sha256"], record["binary_sha256"])
        self.assertFalse(record["cargo_artifact"]["fresh"])

    def test_default_and_semantic_are_separate_feature_cells(self):
        for package, expected in (("default", []), ("semantic", ["semantic"])):
            with self.subTest(package=package):
                path, record = self.build_receipt(package)
                self.assertEqual(record["status"], "passed", record.get("error"))
                self.assertEqual(cold.validate_cell(path)["cargo_artifact"]["features"], expected)

    def test_failed_build_preserves_nonzero_receipt_and_raw_stderr(self):
        path, record = self.build_receipt(failure=True)
        self.assertEqual(record["status"], "failed")
        self.assertEqual(record["build_exit_code"], 7)
        self.assertIn("synthetic compiler failure", (path.parent / "cargo-stderr.log").read_text())
        with self.assertRaisesRegex(ValueError, "passed"):
            cold.validate_cell(path)

    def test_warm_artifact_cannot_be_relabelled_cold(self):
        path, record = self.build_receipt()
        self.rewrite_artifact(path, record, lambda artifact: artifact.update(fresh=True))
        with self.assertRaisesRegex(ValueError, "cold"):
            cold.validate_cell(path)

    def test_feature_lie_is_rejected_even_after_log_digest_update(self):
        path, record = self.build_receipt()
        self.rewrite_artifact(path, record, lambda artifact: artifact.update(features=["semantic"]))
        with self.assertRaisesRegex(ValueError, "features"):
            cold.validate_cell(path)

    def test_profile_lie_is_rejected(self):
        path, record = self.build_receipt()
        self.rewrite_artifact(path, record, lambda artifact: artifact["profile"].update(opt_level="3"))
        with self.assertRaisesRegex(ValueError, "profile"):
            cold.validate_cell(path)

    def test_external_target_artifact_is_rejected(self):
        path, record = self.build_receipt()
        outside = self.base / "external-product"
        outside.write_bytes(b"not a target product")
        self.rewrite_artifact(path, record, lambda artifact: artifact.update(executable=str(outside)))
        with self.assertRaisesRegex(ValueError, "escaped"):
            cold.validate_cell(path)

    def test_source_drift_is_detected_despite_assume_unchanged(self):
        path, _ = self.build_receipt()
        self.git("update-index", "--assume-unchanged", "crates/cc-server/src/main.rs")
        (self.root / "crates/cc-server/src/main.rs").write_text("fn main() { panic!(); }\n")
        with self.assertRaisesRegex(ValueError, "bytes differ"):
            cold.validate_cell(path)

    def test_untracked_and_ignored_build_inputs_are_rejected(self):
        rogue = self.root / "crates/cc-server/rogue.rs"
        rogue.write_text("unbound\n")
        with self.assertRaisesRegex(ValueError, "untracked/ignored"):
            cold.source_identity(self.root)
        (self.root / ".gitignore").write_text("crates/cc-server/rogue.rs\n")
        with self.assertRaisesRegex(ValueError, "untracked/ignored"):
            cold.source_identity(self.root)

    def test_symlink_source_with_identical_bytes_is_rejected(self):
        target = self.base / "outside.rs"
        source = self.root / "crates/cc-server/src/main.rs"
        target.write_bytes(source.read_bytes())
        source.unlink()
        source.symlink_to(target)
        with self.assertRaisesRegex(ValueError, "non-regular"):
            cold.source_identity(self.root)

    def test_source_commit_pin_is_enforced(self):
        with self.assertRaisesRegex(ValueError, "commit differs"):
            cold.source_identity(self.root, "0" * 40)

    def test_compiler_byte_replacement_is_rejected(self):
        path, record = self.build_receipt()
        Path(record["toolchain"]["rustc"]["path"]).write_text("different compiler\n")
        with self.assertRaisesRegex(ValueError, "rustc bytes"):
            cold.validate_cell(path)

    def test_linux_receipt_does_not_certify_macos(self):
        path, record = self.build_receipt()
        record["cell"]["platform"] = "linux" if record["cell"]["platform"] == "macos" else "macos"
        self.rewrite(path, record)
        with self.assertRaisesRegex(ValueError, "matrix platform"):
            cold.validate_cell(path)

    def test_newer_compiler_does_not_certify_msrv(self):
        path, record = self.build_receipt()
        record["toolchain"]["rustc_release"] = "1.97.0"
        record["toolchain_after"]["rustc_release"] = "1.97.0"
        self.rewrite(path, record)
        with self.assertRaisesRegex(ValueError, "non-MSRV"):
            cold.validate_cell(path)

    def test_unlocked_command_is_rejected(self):
        path, record = self.build_receipt()
        record["command"].remove("--locked")
        self.rewrite(path, record)
        with self.assertRaisesRegex(ValueError, "--locked"):
            cold.validate_cell(path)

    def test_missing_stderr_digest_is_rejected(self):
        path, record = self.build_receipt()
        del record["logs_sha256"]["cargo-stderr.log"]
        self.rewrite(path, record)
        with self.assertRaisesRegex(ValueError, "both Cargo logs"):
            cold.validate_cell(path)

    def test_existing_output_preserves_earlier_receipt(self):
        output = cold.new_directory(self.base / "owned-output")
        receipt = output / "receipt.json"
        receipt.write_text("prior evidence\n")
        with self.assertRaises(FileExistsError):
            cold.new_directory(output)
        self.assertEqual(receipt.read_text(), "prior evidence\n")

    def test_output_symlink_is_refused(self):
        occupied = cold.new_directory(self.base / "occupied")
        alias = self.base / "alias"
        alias.symlink_to(occupied, target_is_directory=True)
        with self.assertRaises(FileExistsError):
            cold.new_directory(alias)
        self.assertTrue(alias.is_symlink())

    def test_inventory_has_eight_not_run_cells_and_nonzero_exit(self):
        output = self.base / "inventory"
        with mock.patch.object(cold, "discover_toolchain", return_value=(None, [], "toolchain_unavailable")), \
                mock.patch.object(cold, "build_cell") as builder, contextlib.redirect_stdout(io.StringIO()):
            code = cold.main(["--source-root", str(self.root), "--output-dir", str(output)])
        self.assertEqual(code, 2)
        matrix = json.loads((output / "matrix.json").read_text())
        self.assertEqual(len(matrix["cells"]), 8)
        self.assertEqual(matrix["counts"], dict(passed=0, failed=0, not_run=8))
        self.assertEqual({(row["cell"]["platform"], row["cell"]["toolchain"], row["cell"]["package"])
                          for row in matrix["cells"]},
                         {(os_name, tc, pkg) for os_name in cold.PLATFORMS
                          for tc in cold.TOOLCHAINS for pkg in cold.PACKAGES})
        builder.assert_not_called()

    def test_inventory_does_not_build_an_available_toolchain(self):
        with mock.patch.object(cold, "discover_toolchain", return_value=(self.toolchain(), [], None)), \
                mock.patch.object(cold, "build_cell") as builder, contextlib.redirect_stdout(io.StringIO()):
            code = cold.main(["--source-root", str(self.root), "--output-dir", str(self.base / "available")])
        self.assertEqual(code, 2)
        builder.assert_not_called()

    def test_invalid_source_still_preserves_eight_unpassed_cells(self):
        (self.root / "crates/cc-server/src/main.rs").write_text("changed source\n")
        output = self.base / "invalid-source"
        with contextlib.redirect_stdout(io.StringIO()):
            code = cold.main(["--source-root", str(self.root), "--output-dir", str(output)])
        self.assertEqual(code, 2)
        matrix = json.loads((output / "matrix.json").read_text())
        self.assertEqual(matrix["status"], "invalid")
        self.assertEqual(matrix["counts"], dict(passed=0, failed=0, not_run=8))

    def test_missing_tracking_toolchain_never_invokes_rustup_which(self):
        inventory = dict(exit_code=0, stdout="1.95.0-x86_64-unknown-linux-gnu (default)\n", stderr="")
        with mock.patch.object(cold, "probe", return_value=inventory) as command:
            tools, probes, reason = cold.discover_toolchain("rustup", "stable", "stable")
        self.assertIsNone(tools)
        self.assertEqual(reason, "toolchain_not_installed")
        self.assertEqual(probes, [inventory])
        command.assert_called_once_with(["rustup", "toolchain", "list"])

    def test_unreadable_toolchain_inventory_cannot_pass(self):
        inventory = dict(exit_code=1, stdout="", stderr="failed local inventory")
        with mock.patch.object(cold, "probe", return_value=inventory):
            tools, _, reason = cold.discover_toolchain("rustup", "1.95.0", "1.95")
        self.assertIsNone(tools)
        self.assertEqual(reason, "toolchain_inventory_unavailable")

    def test_rollback_accepts_existing_receipt_without_relabelling_its_source(self):
        path, record = self.build_receipt()
        identity = rollback.binary_identity(record["cargo_artifact"]["executable"], path, "default")
        self.assertEqual(identity["source"]["source_commit"], self.git("rev-parse", "HEAD"))
        self.assertIn("not current-source", identity["claim"])

    def schema_identity(self, label, version):
        root = self.base / label
        path = root / "crates/cc-db/src/index_migrate.rs"
        path.parent.mkdir(parents=True)
        path.write_text(f"pub const CURRENT_SCHEMA_VERSION: u32 = {version};\n")
        manifest = root / "source-inputs.json"
        cold.write_json(manifest, {"crates/cc-db/src/index_migrate.rs": cold.digest(path)})
        return dict(source={"source_root": str(root), "source_commit": label},
                    source_manifest_path=str(manifest))

    def test_actual_version_pair_requires_distinct_revisions_and_older_real_schema(self):
        current = self.schema_identity("current", 25)
        older = self.schema_identity("previous", 24)
        self.assertEqual(rollback.version_pair_contract(current, older), {"current": 25, "previous": 24})
        with self.assertRaisesRegex(ValueError, "distinct actual source"):
            rollback.version_pair_contract(current, current)
        same_schema = self.schema_identity("different-commit-same-schema", 25)
        with self.assertRaisesRegex(ValueError, "actually older schema"):
            rollback.version_pair_contract(current, same_schema)
        (Path(older["source"]["source_root"]) / "crates/cc-db/src/index_migrate.rs").write_text(
            "pub const CURRENT_SCHEMA_VERSION: u32 = 23;\n")
        with self.assertRaisesRegex(ValueError, "differs from the bound"):
            rollback.version_pair_contract(current, older)

    def test_version_pair_public_probe_rejects_echo_wrong_path_and_false_span(self):
        project = self.base / "public-fixture"
        (project / "src").mkdir(parents=True)
        (project / "src/lib.rs").write_text(rollback.FIXTURE_FILES["src/lib.rs"])
        hit = dict(name=rollback.MARKER, file_path="src/lib.rs", start_line=1, end_line=1)
        self.assertEqual(rollback.verify_fixture_search(project, [hit])["matching_symbols"], 1)
        for value in ({"query": rollback.MARKER, "results": []}, [hit | {"file_path": "other.rs"}],
                      [hit | {"start_line": 0}], [hit | {"end_line": 100}], "not structured"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                rollback.verify_fixture_search(project, value)
        (project / "src/lib.rs").write_text("pub fn different_symbol() {}\n")
        with self.assertRaisesRegex(ValueError, "does not define"):
            rollback.verify_fixture_search(project, [hit])

    def test_rollback_rejects_wrong_package_or_changed_binary(self):
        path, record = self.build_receipt()
        binary = record["cargo_artifact"]["executable"]
        with self.assertRaisesRegex(ValueError, "package kind"):
            rollback.binary_identity(binary, path, "semantic")
        Path(binary).write_bytes(b"replaced")
        with self.assertRaisesRegex(ValueError, "binary digest"):
            rollback.binary_identity(binary, path, "default")

    def test_rollback_rejects_unbound_source_or_manifest_traversal(self):
        path, record = self.build_receipt()
        binary = record["cargo_artifact"]["executable"]
        record["source_after"] = {}
        self.rewrite(path, record)
        with self.assertRaisesRegex(ValueError, "source binding"):
            rollback.binary_identity(binary, path, "default")
        record["source_after"] = record["source_before"]
        record["source_manifest"] = "../source-inputs.json"
        self.rewrite(path, record)
        with self.assertRaisesRegex(ValueError, "sibling filename"):
            rollback.binary_identity(binary, path, "default")

    def test_authored_source_mutation_addition_and_deletion_are_visible(self):
        project = self.base / "project"
        (project / "src").mkdir(parents=True)
        source = project / "src/lib.rs"
        source.write_text("original source\n")
        original = rollback.source_manifest(project)
        (project / ".codecortex.json").write_text("{}\n")
        self.assertEqual(original, rollback.source_manifest(project))
        source.write_text("changed source\n")
        self.assertNotEqual(original, rollback.source_manifest(project))
        source.unlink()
        self.assertNotEqual(original, rollback.source_manifest(project))
        source.write_text("original source\n")
        (project / "src/extra.rs").write_text("added source\n")
        self.assertNotEqual(original, rollback.source_manifest(project))

    def test_sqlite_backup_includes_committed_wal_and_is_exclusive(self):
        source = self.base / "index.sqlite3"
        backup = self.base / "old-backup.sqlite3"
        with sqlite3.connect(source) as connection:
            connection.execute("PRAGMA journal_mode=WAL")
            connection.execute("PRAGMA user_version=25")
            connection.execute("CREATE TABLE files(path TEXT NOT NULL)")
            connection.execute("INSERT INTO files VALUES ('src/lib.rs')")
            connection.commit()
            result = rollback.backup_database(source, backup)
        self.assertEqual(result["snapshot"]["indexed_files"], 1)
        self.assertEqual(result["snapshot"]["schema_version"], 25)
        self.assertEqual(result["snapshot"]["integrity"], "ok")
        with self.assertRaisesRegex(ValueError, "already exists"):
            rollback.backup_database(source, backup)
        self.assertEqual(result["sha256"], cold.digest(backup))

    def test_expired_backup_deadline_retains_failed_destination(self):
        source = self.base / "index.sqlite3"
        backup = self.base / "deadline-backup.sqlite3"
        with sqlite3.connect(source) as connection:
            connection.execute("CREATE TABLE files(path TEXT)")
            connection.execute("INSERT INTO files VALUES ('source')")
        with self.assertRaisesRegex(TimeoutError, "bounded deadline"):
            rollback.backup_database(source, backup, timeout_seconds=-1)
        self.assertTrue(backup.exists())

    def test_readonly_snapshot_closes_connection_on_success_and_failure(self):
        real_connect = sqlite3.connect
        for invalid in (False, True):
            with self.subTest(invalid=invalid):
                source = self.base / f"snapshot-close-{invalid}.sqlite3"
                with contextlib.closing(real_connect(source)) as connection:
                    if not invalid:
                        connection.execute("CREATE TABLE files(path TEXT)")
                opened = []

                def connect(*args, **kwargs):
                    connection = real_connect(*args, **kwargs)
                    opened.append(connection)
                    return connection

                with mock.patch.object(rollback.sqlite3, "connect", side_effect=connect):
                    if invalid:
                        with self.assertRaises(sqlite3.OperationalError):
                            rollback.db_snapshot(source)
                    else:
                        self.assertEqual(rollback.db_snapshot(source)["integrity"], "ok")
                self.assertEqual(len(opened), 1)
                with self.assertRaisesRegex(sqlite3.ProgrammingError, "closed database"):
                    opened[0].execute("SELECT 1")

    def test_backup_closes_all_connections_on_success_and_failure(self):
        real_connect = sqlite3.connect
        source = self.base / "backup-close-source.sqlite3"
        with contextlib.closing(real_connect(source)) as connection:
            connection.execute("CREATE TABLE files(path TEXT)")
        for expired in (False, True):
            with self.subTest(expired=expired):
                opened = []

                def connect(*args, **kwargs):
                    connection = real_connect(*args, **kwargs)
                    opened.append(connection)
                    return connection

                destination = self.base / f"backup-close-{expired}.sqlite3"
                with mock.patch.object(rollback.sqlite3, "connect", side_effect=connect):
                    if expired:
                        with self.assertRaises(TimeoutError):
                            rollback.backup_database(source, destination, timeout_seconds=-1)
                    else:
                        self.assertEqual(rollback.backup_database(source, destination)["snapshot"]["integrity"], "ok")
                self.assertGreaterEqual(len(opened), 2)
                for connection in opened:
                    with self.assertRaisesRegex(sqlite3.ProgrammingError, "closed database"):
                        connection.execute("SELECT 1")

    def test_rebuild_controls_reject_reuse_sentinel_loss_and_bad_integrity(self):
        before = dict(schema_version=1025, future_sentinel_present=True)
        after = dict(schema_version=25, future_sentinel_present=False, integrity="ok",
                     foreign_key_errors=0, indexed_files=1)
        rollback.validate_rebuild(before, after)
        for key, value in (("schema_version", 1025), ("future_sentinel_present", True),
                           ("integrity", "bad"), ("foreign_key_errors", 1), ("indexed_files", 0)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                rollback.validate_rebuild(before, after | {key: value})
        with self.assertRaisesRegex(ValueError, "fault injection"):
            rollback.validate_rebuild(before | {"future_sentinel_present": False}, after)


if __name__ == "__main__":
    unittest.main()
