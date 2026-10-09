"""Independent negative controls for the bounded recovery evidence/oracles."""
import io
import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_recovery as recovery
from p8_rollback import Product


class RecoveryControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="p8-recovery-contract-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        (self.root / "src").mkdir()
        (self.root / "src/lib.rs").write_text("pub fn present_symbol() -> u32 { 1 }\n")

    def test_command_failure_retains_both_raw_logs_and_cannot_pass(self):
        output = self.root / "failed-command"
        with self.assertRaisesRegex(ValueError, "command failed"):
            recovery.retained_command([sys.executable, "-c", "import sys; print('raw-out'); print('raw-error', file=sys.stderr); sys.exit(7)"],
                                      self.root, output, dict(recovery.os.environ), timeout=5)
        record = json.loads((output / "command.json").read_text())
        self.assertEqual((record["status"], record["exit_code"]), ("failed", 7))
        self.assertIn("raw-out", (output / "stdout.log").read_text())
        self.assertIn("raw-error", (output / "stderr.log").read_text())
        self.assertEqual(set(record["logs_sha256"]), {"stdout.log", "stderr.log"})

    def test_active_faults_refuse_a_different_binary_before_starting_any_fixture(self):
        binary = self.root / "incorrect-product"
        binary.write_bytes(b"no executable is launched by this negative control")
        output = self.root / "must-not-exist"
        with self.assertRaisesRegex(ValueError, "differs from its exact build"):
            recovery.active_stdio_faults(binary, output, 223, "0" * 64)
        self.assertFalse(output.exists())

    def test_command_timeout_is_a_failed_owned_process_receipt(self):
        output = self.root / "timed-command"
        with self.assertRaisesRegex(ValueError, "command failed"):
            recovery.retained_command([sys.executable, "-c", "import time; time.sleep(20)"],
                                      self.root, output, dict(recovery.os.environ), timeout=0.05)
        record = json.loads((output / "command.json").read_text())
        self.assertEqual(record["status"], "failed")
        self.assertIn("TimeoutExpired", record["error"])
        self.assertNotEqual(record["exit_code"], 0)

    def test_full_evidence_prunes_disposable_target_before_opening_files(self):
        target = self.root / "cargo-target"
        target.mkdir()
        (target / "external-link").symlink_to("/does-not-exist")
        manifest = recovery.full_evidence_manifest(self.root)
        self.assertEqual(set(manifest), {"src/lib.rs"})
        (self.root / "unexpected-link").symlink_to(self.root / "src/lib.rs")
        with self.assertRaisesRegex(ValueError, "nonregular"):
            recovery.full_evidence_manifest(self.root)

    def hit(self, **changes):
        return dict(name="present_symbol", qname="present_symbol", file_path="src/lib.rs",
                    start_line=1, end_line=1) | changes

    def test_public_symbol_is_proven_against_its_actual_source_span(self):
        result = recovery.verify_symbol(self.root, [self.hit()], "present_symbol", present=True)
        self.assertEqual(result["matching_symbols"], 1)
        self.assertEqual(result["checked"][0]["source_sha256"], recovery.digest(self.root / "src/lib.rs"))

    def test_source_snapshot_never_opens_sqlite_cache_files(self):
        cache = self.root / ".codecortex"
        cache.mkdir()
        databases = [cache / name for name in ("index.sqlite3", "index.sqlite3-wal", "index.sqlite3-shm")]
        for database in databases:
            database.write_bytes(b"must never be read by source hashing")
        with mock.patch("p8_rollback.digest", wraps=recovery.digest) as hashed:
            manifest = recovery.source_manifest(self.root)
        self.assertEqual(set(manifest), {"src/lib.rs"})
        for database in databases:
            self.assertNotIn(mock.call(database), hashed.call_args_list)

    def test_stop_observation_uses_the_owned_child_wait_event(self):
        process = SimpleNamespace(pid=27)
        stopped_status = (signal.SIGSTOP << 8) | 0x7f
        with mock.patch.object(recovery.os, "waitpid", return_value=(27, stopped_status)) as wait:
            self.assertTrue(recovery.process_is_stopped(process))
        wait.assert_called_once_with(27, recovery.os.WUNTRACED | recovery.os.WNOHANG)
        with mock.patch.object(recovery.os, "waitpid", return_value=(0, 0)):
            self.assertFalse(recovery.process_is_stopped(process))
        with mock.patch.object(recovery.os, "waitpid", return_value=(27, 0)), self.assertRaises(ValueError):
            recovery.process_is_stopped(process)

    def test_echoed_query_is_not_a_successful_hit(self):
        with self.assertRaisesRegex(ValueError, "expected authored symbol"):
            recovery.verify_symbol(self.root, {"query": "present_symbol", "results": []},
                                   "present_symbol", present=True)

    def test_missing_or_malformed_result_array_is_not_empty_success(self):
        for response in ({"query": "deleted"}, {"results": "deleted"}, ["not an object"]):
            with self.subTest(response=response), self.assertRaises(ValueError):
                recovery.verify_symbol(self.root, response, "deleted", present=False)

    def test_false_source_path_or_span_cannot_prove_recovery(self):
        for changes in ({"file_path": "src/missing.rs"}, {"file_path": "../outside.rs"},
                        {"file_path": str(self.root / "src/lib.rs")}, {"start_line": 0},
                        {"end_line": 20}, {"name": "wrong_symbol", "qname": "wrong_symbol"}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                recovery.verify_symbol(self.root, [self.hit(**changes)], "present_symbol", present=True)

    def test_symbol_name_cannot_hide_wrong_source_bytes(self):
        (self.root / "src/lib.rs").write_text("pub fn another_symbol() -> u32 { 2 }\n")
        with self.assertRaisesRegex(ValueError, "does not define"):
            recovery.verify_symbol(self.root, [self.hit()], "present_symbol", present=True)

    def test_deleted_symbol_or_deleted_path_is_a_hard_failure(self):
        with self.assertRaisesRegex(ValueError, "resurrected"):
            recovery.verify_symbol(self.root, [self.hit()], "present_symbol", present=False)
        with self.assertRaisesRegex(ValueError, "deleted source path"):
            recovery.verify_symbol(self.root, [self.hit(name="other", qname="other")],
                                   "deleted_symbol", present=False, deleted_path="src/lib.rs")
        result = recovery.verify_symbol(self.root, [], "deleted_symbol", present=False)
        self.assertEqual(result["matching_symbols"], 0)

    def kill_observation(self):
        return dict(stop_observed=True, pending_request_id=6, exit_code=-signal.SIGKILL,
                    request=dict(kind="error", event=dict(kind="process_exit")))

    def test_observed_kill_requires_terminal_failure_of_pending_request(self):
        recovery.classify_killed_request(self.kill_observation())
        invalid = [dict(stop_observed=False), dict(pending_request_id=None), dict(exit_code=0),
                   dict(request=dict(kind="result", value={"ready": True})),
                   dict(request=dict(kind="error", event=dict(kind="rpc_timeout")))]
        for patch in invalid:
            with self.subTest(patch=patch), self.assertRaises(ValueError):
                recovery.classify_killed_request(self.kill_observation() | patch)

    def test_async_operation_retains_actual_result_or_error(self):
        self.assertEqual(recovery.AsyncCall(lambda: {"done": True}).collect(1),
                         dict(kind="result", value={"done": True}))
        def failed():
            raise ValueError("actual operation failed")
        outcome = recovery.AsyncCall(failed).collect(1)
        self.assertEqual(outcome["kind"], "error")
        self.assertEqual(outcome["message"], "actual operation failed")

    def test_request_observation_has_a_real_deadline(self):
        release = threading.Event()
        operation = recovery.AsyncCall(lambda: release.wait(1))
        try:
            with self.assertRaisesRegex(ValueError, "observation deadline"):
                operation.collect(0.001)
        finally:
            release.set()
            operation.collect(1)

    def test_only_explicit_busy_failures_are_retried(self):
        self.assertTrue(recovery.is_busy_error(dict(kind="error", message="database is locked")))
        self.assertFalse(recovery.is_busy_error(dict(kind="result", message="busy")))
        self.assertFalse(recovery.is_busy_error(dict(kind="error", message="invalid schema")))
        product = SimpleNamespace(project=self.root, tool=mock.Mock(side_effect=[ValueError("BuildBusy"), {"ok": True}]))
        with mock.patch.object(recovery.time, "sleep"):
            result = recovery.incremental_after_busy(product, mock.Mock())
        self.assertEqual(result, {"ok": True})
        self.assertEqual(product.tool.call_count, 2)

    def test_busy_retries_are_bounded_and_generic_errors_fail_immediately(self):
        for message, expected in (("database is locked", 3), ("corrupt source", 1)):
            product = SimpleNamespace(project=self.root, tool=mock.Mock(side_effect=ValueError(message)))
            with mock.patch.object(recovery.time, "sleep"), self.assertRaises(ValueError):
                recovery.incremental_after_busy(product, mock.Mock())
            self.assertEqual(product.tool.call_count, expected)

    def capability_product(self):
        payload = dict(has_index=True, capabilities=dict(search=True), retrieval=dict(
            local_state="available", identity_validation="checked_at_observation_boundary",
            semantic_state="not_configured", dense_state="disabled", query_coverage=dict(state="not_measured")))
        return SimpleNamespace(project=self.root, tool=mock.Mock(return_value=payload),
                               transport=SimpleNamespace(pending=SimpleNamespace(raise_if_terminal=mock.Mock())))

    def test_ready_receipt_requires_live_transport_and_honest_disabled_state(self):
        product = self.capability_product()
        recovery.checked_capability(product)
        product.transport.pending.raise_if_terminal.side_effect = RuntimeError("process already exited")
        with self.assertRaisesRegex(RuntimeError, "already exited"):
            recovery.checked_capability(product)
        product = self.capability_product()
        product.tool.return_value["retrieval"]["dense_state"] = "ready"
        with self.assertRaisesRegex(ValueError, "false ready"):
            recovery.checked_capability(product)

    def test_product_initialization_reuses_existing_protocol(self):
        product = object.__new__(Product)
        product.record = {}
        product.rpc = mock.Mock(side_effect=[{}, {"tools": [{} for _ in range(14)]}])
        product.process = SimpleNamespace(stdin=io.StringIO())
        product.transport = SimpleNamespace(write_lock=threading.Lock(), journal=mock.Mock(),
                                            pending=SimpleNamespace(raise_if_terminal=mock.Mock()))
        self.assertEqual(product.initialize(), 14)
        self.assertEqual(product.initialize(), 14)
        self.assertEqual(product.rpc.call_count, 2)
        self.assertIn("notifications/initialized", product.process.stdin.getvalue())

    def fake_identity(self):
        binary = self.root / "fake-product"
        binary.write_text("contract fixture only\n")
        receipt = self.root / "build-receipt.json"
        receipt.write_text("{}\n")
        manifest = self.root / "source-inputs.json"
        manifest.write_text("{}\n")
        return dict(binary_path=str(binary), binary_sha256=recovery.digest(binary),
                    receipt_path=str(receipt), source_manifest_path=str(manifest))

    def engineering_fixture(self):
        source = self.root / "witness-source"
        (source / "crates/cc-server/src").mkdir(parents=True)
        sources = {"Cargo.toml": '[workspace]\nmembers=["crates/cc-server"]\n',
                   "Cargo.lock": "version = 4\n", "crates/cc-server/Cargo.toml": '[package]\nname="cc-server"\n',
                   "crates/cc-server/src/main.rs": "fn main() {}\n"}
        for name, data in sources.items():
            (source / name).write_text(data)
        def git(*args):
            return subprocess.check_output(["git", *args], cwd=source, text=True).strip()
        git("init", "-q")
        git("add", ".")
        git("-c", "user.name=P8 recovery contract", "-c", "user.email=p8@example.invalid",
            "commit", "-qm", "synthetic witness input")
        binary = self.root / "synthetic-witness-product"
        binary.write_text("contract only, not an actual product\n")
        manifest = self.root / "manifest.json"
        recovery.write_json(manifest, {name: recovery.digest(source / name) for name in sources})
        artifact = dict(reason="compiler-artifact", target=dict(name="codecortex", kind=["bin"]),
                        fresh=False, features=[], manifest_path=str(source / "crates/cc-server/Cargo.toml"),
                        executable=str(binary))
        stdout = self.root / "product-build.jsonl"
        stdout.write_text(json.dumps(artifact) + "\n" + json.dumps(dict(reason="build-finished", success=True)) + "\n")
        stderr = self.root / "product-build.stderr"
        stderr.write_text("")
        witness = dict(schema_version=1, receipt_kind="post_build_source_and_binary_witness",
                       cold_build_claim=False, release_certified=False, build_exit_code=0,
                       package_kind="default", binary_sha256=recovery.digest(binary),
                       build_command=["cargo", "build", "--locked", "--offline"], cargo_artifact=artifact,
                       source_commit=git("rev-parse", "HEAD"), source_manifest=manifest.name,
                       source_manifest_sha256=recovery.digest(manifest), source_input_count=len(sources),
                       logs={stdout.name: recovery.digest(stdout), stderr.name: recovery.digest(stderr)})
        receipt = self.root / "engineering.json"
        recovery.write_json(receipt, witness)
        return binary, receipt, source, witness

    def test_engineering_witness_is_bound_but_cannot_pass_the_cold_receipt_path(self):
        binary, receipt, _, witness = self.engineering_fixture()
        identity = recovery.engineering_identity(binary, receipt, "default")
        self.assertFalse(identity["cold_build_claim"])
        self.assertEqual(identity["source"]["source_commit"], witness["source_commit"])
        with self.assertRaisesRegex(ValueError, "explicitly feature-selected"):
            recovery.binary_identity(binary, receipt, "default")

    def test_engineering_source_or_build_log_drift_is_rejected(self):
        binary, receipt, source, _ = self.engineering_fixture()
        file = source / "crates/cc-server/src/main.rs"
        original = file.read_bytes()
        file.write_text("changed source\n")
        with self.assertRaisesRegex(ValueError, "source bytes changed"):
            recovery.engineering_identity(binary, receipt, "default")
        file.write_bytes(original)
        (self.root / "product-build.stderr").write_text("changed log\n")
        with self.assertRaisesRegex(ValueError, "log digest"):
            recovery.engineering_identity(binary, receipt, "default")

    def test_engineering_witness_cannot_claim_cold_or_invent_source_count(self):
        binary, receipt, _, witness = self.engineering_fixture()
        for patch in (dict(cold_build_claim=True), dict(source_input_count=999), dict(package_kind="semantic")):
            recovery.write_json(receipt, witness | patch)
            with self.subTest(patch=patch), self.assertRaises(ValueError):
                recovery.engineering_identity(binary, receipt, "default")

    def test_failed_case_is_preserved_and_cannot_make_green_rollup(self):
        output = recovery.new_directory(self.root / "run")
        with mock.patch.object(recovery, "kill_restart", side_effect=ValueError("injected failure")), \
                mock.patch.object(recovery, "database_busy", return_value={}), \
                mock.patch.object(recovery, "deleted_source", return_value={}):
            report = recovery.run(self.fake_identity(), output)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["counts"], dict(passed=2, failed=1, not_run=4, cancelled=0))
        self.assertIn("injected failure", json.loads((output / "kill_restart/receipt.json").read_text())["error"])
        self.assertFalse(report["complete_P8_011"])

    def test_cancelled_case_leaves_unstarted_scenarios_not_run(self):
        output = recovery.new_directory(self.root / "cancelled")
        with mock.patch.object(recovery, "kill_restart", side_effect=KeyboardInterrupt), \
                mock.patch.object(recovery, "database_busy") as second:
            report = recovery.run(self.fake_identity(), output)
        self.assertEqual(report["status"], "cancelled")
        self.assertEqual(report["counts"]["not_run"], 6)
        second.assert_not_called()


if __name__ == "__main__":
    unittest.main()
