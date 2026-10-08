import argparse
from concurrent.futures import ThreadPoolExecutor, wait
import json
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runtime as runtime


class RuntimeDriverControls(unittest.TestCase):
    """Exercise run/stdio/replay/seal with tiny synthetic executable controls.

    Build admission is mocked because these scripts are neither Cargo artifacts
    nor measured product evidence. Requests, process exits, fixture mutations,
    replay execution and final sealing use the actual driver paths.
    """

    def prepare_control(self, oracle_exit):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        directory = Path(temporary.name)
        build = directory / "build"
        build.mkdir()

        def executable(name, body):
            path = build / name
            path.write_text("#!" + sys.executable + "\n" + body)
            path.chmod(0o755)
            return path

        product = executable("codecortex", """import json, os, sys
for line in sys.stdin:
    message = json.loads(line)
    if 'id' not in message: continue
    if message['method'] == 'initialize': result = {}
    elif message['method'] == 'tools/list': result = {'tools': [{}] * 14}
    else:
        name = message['params']['name']
        if name == 'index': value = {'parse_errors': [], 'resolution_freshness': {'complete': True}}
        elif name == 'search': value = [{'name': 'p8_runtime_stable_signal', 'file_path': 'stable.py'}]
        else: value = {'diagnostics': {'process_resources': {'pid': os.getpid(), 'resident_bytes': 10000000}}}
        result = {'structuredContent': {'result': value}}
    print(json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}), flush=True)
""")
        oracle = executable("p8-oracle", """import argparse, json
from pathlib import Path
p = argparse.ArgumentParser()
for flag in ('left', 'right', 'output'): p.add_argument('--' + flag)
a = p.parse_args()
code = ORACLE_EXIT
Path(a.output).write_text(json.dumps({'comparison': {'equal': code == 0}, 'exit_code': code}))
raise SystemExit(code)
""".replace("ORACLE_EXIT", str(oracle_exit)))
        statistics = executable("p8-runtime-statistics", """import argparse, hashlib, json
from pathlib import Path
p = argparse.ArgumentParser()
for flag in ('plan', 'raw', 'output'): p.add_argument('--' + flag)
a = p.parse_args()
def sha(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
rows = [json.loads(line) for line in Path(a.raw).read_text().splitlines()]
rows = [row for row in rows if row['kind'] == 'operation']
code = int(any(row['status'] != 'success' for row in rows))
Path(a.output).write_text(json.dumps({'plan_sha256': sha(a.plan), 'raw_sha256': sha(a.raw),
    'replay_binary_sha256': sha(__file__), 'exit_code': code, 'recorded_samples': len(rows),
    'observation_status': 'synthetic_control'}, sort_keys=True))
raise SystemExit(code)
""")
        for name in ("source-before.json", "source-after.json", "product-build.jsonl", "seal.json"):
            (build / name).write_text("{}\n")
        (build / "product-build.stderr").write_text("synthetic control; no Cargo compilation\n")
        root = Path(runtime.__file__).resolve().parents[1]
        for relative in runtime.OBSERVER_FILES:
            destination = build / "observer-source" / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((root / relative).read_bytes())
        receipt = build / "build-receipt.json"
        receipt.write_text(json.dumps({"source_before": {"synthetic_control": True},
                                      "oracle_sha256": runtime.digest(oracle),
                                      "statistics_path": str(statistics)}))
        identity = {"binary_path": str(product), "binary_sha256": runtime.digest(product),
                    "build_identity": {"synthetic_control": True}}
        args = argparse.Namespace(binary=product, oracle=oracle, statistics=statistics,
                                  build_receipt=receipt, output=directory / "run", profile="mixed",
                                  concurrency=1, operations=60, files=8, interval_ms=1)
        return args, identity

    def run_control(self, oracle_exit):
        args, identity = self.prepare_control(oracle_exit)
        with mock.patch.object(runtime, "verify_build", return_value=identity), \
                mock.patch.object(runtime, "verify_receipt", return_value=identity["build_identity"]):
            report = runtime.run(args)
        raw = [json.loads(line) for line in (args.output / "raw.jsonl").read_text().splitlines()]
        self.assertEqual(sum(row["kind"] == "operation" for row in raw), args.operations)
        self.assertEqual(next(row["exit_code"] for row in raw if row["kind"] == "oracle_process"), oracle_exit)
        runtime.verify_output(args.output)
        return args.output, report

    def interrupted_control(self, trigger):
        args, identity = self.prepare_control(0)
        args.concurrency, args.operations = 4, 300
        interrupted, release = threading.Event(), threading.Event()
        futures, waits = [], []
        real_tool, real_raw = runtime.Product.tool, runtime.Raw

        class TrackingPool(ThreadPoolExecutor):
            def submit(self, *arguments, **keywords):
                future = super().submit(*arguments, **keywords)
                futures.append(future)
                return future

        class BudgetFailureRaw(real_raw):
            def emit(self, kind, **fields):
                if trigger == "raw_budget" and kind == "operation" and fields.get("status") == "queue_rejected":
                    # Exercise the actual raw-size guard, including all later
                    # cancellation writes failing against that same full sink.
                    runtime.MAX_RAW_BYTES = self.size
                    interrupted.set()
                return super().emit(kind, **fields)

        def delayed_completion(product, name, arguments, timeout=45):
            response = real_tool(product, name, arguments, timeout)
            if product.output.name == "product" and (name == "search" or (name == "index" and not arguments.get("full"))):
                # Delay client handling of an already received real stdio
                # response. The driver must still own and join this work.
                if not release.wait(5):
                    raise RuntimeError("bounded delayed client completion control")
            return response

        def observed_wait(selected, timeout):
            selected = list(selected)
            waits.append(timeout)
            if trigger == "drain_deadline" and timeout == 180:
                interrupted.set()
                return set(), set(selected)
            return wait(selected, timeout=timeout)

        def release_after_interruption():
            interrupted.wait(5)
            release.wait(.25)
            release.set()

        releaser = threading.Thread(target=release_after_interruption, daemon=True)
        releaser.start()
        try:
            with mock.patch.object(runtime, "verify_build", return_value=identity), \
                    mock.patch.object(runtime, "verify_receipt", return_value=identity["build_identity"]), \
                    mock.patch.object(runtime, "ThreadPoolExecutor", TrackingPool), \
                    mock.patch.object(runtime, "Raw", BudgetFailureRaw), \
                    mock.patch.object(runtime, "MAX_RAW_BYTES", runtime.MAX_RAW_BYTES), \
                    mock.patch.object(runtime, "wait", side_effect=observed_wait), \
                    mock.patch.object(runtime.Product, "tool", delayed_completion):
                report = runtime.run(args)
                self.assertTrue(interrupted.is_set())
                self.assertTrue(all(future.done() for future in futures), "sealing preceded owned work completion")
                self.assertFalse((args.output / "project/temporary.py").exists(), "waiting writer ran after cancellation")
                self.assertEqual(report["exit_code"], 2)
                self.assertEqual(report["work_cleanup"]["still_running"], 0)
                self.assertEqual(report["work_cleanup"]["cleanup_bound_seconds"], 70)
                self.assertIn(70, waits)
                runtime.verify_output(args.output)
        finally:
            release.set()
            releaser.join(timeout=1)
            # Keep a failing regression bounded and clean up its own threads.
            wait([future for future in futures if not future.done()], timeout=5)
        return args.output, report, len(futures), waits

    def test_completed_unequal_oracle_preserves_gate_failure_and_all_statistics(self):
        output, report = self.run_control(1)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["exit_code"], 1)
        self.assertIn("complete no-repair endpoint oracle failed", report["failures"])
        self.assertEqual(report["outcomes"], {"success": 60})
        self.assertEqual(report["latency"]["n"], 60)
        self.assertEqual(report["latency_by_operation"]["build"]["n"], 20)
        self.assertEqual(report["latency_by_operation"]["read"]["n"], 40)
        self.assertFalse(report["release_approval"])
        self.assertFalse(report["task_complete"])
        self.assertEqual(report["parity_sha256"], runtime.digest(output / "parity.json"))
        self.assertEqual((output / "statistics.json").read_bytes(),
                         (output / "statistics-replay.json").read_bytes())
        self.assertEqual(json.loads((output / "statistics.json").read_text())["recorded_samples"], 60)
        for name in ("product", "full-product"):
            self.assertEqual(json.loads((output / name / "process.json").read_text())["cleanup"], "completed")

    def test_oracle_execution_error_remains_driver_failure(self):
        output, report = self.run_control(2)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["exit_code"], 2)
        self.assertIn("oracle could not complete", report["error"])
        self.assertEqual(json.loads((output / "parity.json").read_text())["exit_code"], 2)

    def test_equal_oracle_retains_successful_observation(self):
        _, report = self.run_control(0)
        self.assertEqual(report["status"], "passed_observation")
        self.assertEqual(report["exit_code"], 0)
        self.assertEqual(report["failures"], [])
        self.assertEqual(report["outcomes"], {"success": 60})

    def test_raw_budget_failure_cancels_and_joins_before_sealing(self):
        output, report, submitted, _ = self.interrupted_control("raw_budget")
        self.assertIn("raw evidence budget exhausted", report["error"])
        missing = json.loads((output / report["terminal_retention_failures"]).read_text())
        # Every admitted request and the triggering rejected offer keep their
        # terminal identity, even though the original raw sink stays exhausted.
        self.assertEqual(sorted(row["id"] for row in missing), list(range(submitted + 1)))
        self.assertEqual(report["offered_terminal_rows"], submitted + 1)
        self.assertTrue(any(row["status"] == "canceled" for row in missing))
        self.assertFalse((output / "statistics.json").exists())
        runtime.verify_output(output)

    def test_existing_drain_deadline_preserves_canceled_outcomes_and_bounds(self):
        output, report, _, waits = self.interrupted_control("drain_deadline")
        self.assertIn("failed to drain in 180 seconds", report["error"])
        self.assertEqual(waits, [180, 70])
        rows = [json.loads(line) for line in (output / "raw.jsonl").read_text().splitlines()]
        operations = [row for row in rows if row["kind"] == "operation"]
        self.assertEqual(sorted(row["id"] for row in operations), list(range(300)))
        cleanup = next(row for row in rows if row["kind"] == "drain_deadline")
        self.assertGreater(cleanup["canceled"], 0)
        self.assertEqual(cleanup["still_running"], 0)
        self.assertEqual(cleanup["cleanup_bound_seconds"], 70)

    def test_submit_failure_retains_offered_attempt_and_releases_slot(self):
        for enqueue_first in (False, True):
            with self.subTest(enqueue_first=enqueue_first):
                args, identity = self.prepare_control(0)
                attempts, futures, semaphores = [], [], []
                original_semaphore = threading.BoundedSemaphore

                class FailingPool(ThreadPoolExecutor):
                    def submit(self, *arguments, **keywords):
                        attempts.append(arguments[1])
                        if len(attempts) == 5 and not enqueue_first:
                            raise RuntimeError("controlled submit failure before enqueue")
                        future = super().submit(*arguments, **keywords)
                        futures.append(future)
                        if len(attempts) == 5:
                            raise RuntimeError("controlled submit failure after enqueue")
                        return future

                def semaphore(capacity):
                    value = original_semaphore(capacity)
                    semaphores.append((capacity, value))
                    return value

                try:
                    with mock.patch.object(runtime, "verify_build", return_value=identity), \
                            mock.patch.object(runtime, "verify_receipt", return_value=identity["build_identity"]), \
                            mock.patch.object(runtime, "ThreadPoolExecutor", FailingPool), \
                            mock.patch.object(runtime.threading, "BoundedSemaphore", side_effect=semaphore):
                        report = runtime.run(args)
                    self.assertTrue(all(future.done() for future in futures))
                    self.assertEqual(report["status"], "failed")
                    self.assertEqual(report["exit_code"], 2)
                    self.assertIn("controlled submit failure", report["error"])
                    self.assertEqual(report["offered_terminal_rows"], 5)
                    raw = [json.loads(line) for line in (args.output / "raw.jsonl").read_text().splitlines()]
                    rows = [row for row in raw if row["kind"] == "operation"]
                    self.assertEqual(sorted(row["id"] for row in rows), attempts)
                    rejected = next(row for row in rows if row["id"] == 4)
                    self.assertEqual(rejected["status"], "error")
                    self.assertEqual(rejected["reason"], "submission_error")
                    self.assertNotIn("started_ns", rejected)
                    self.assertNotIn("call_started_ns", rejected)
                    self.assertNotIn("response", rejected)
                    self.assertFalse((args.output / "statistics.json").exists())
                    self.assertEqual(len(semaphores), 1)
                    capacity, slots = semaphores[0]
                    acquired = 0
                    while slots.acquire(blocking=False):
                        acquired += 1
                    self.assertEqual(acquired, capacity, "failed submission leaked an acquired slot")
                    for _ in range(acquired):
                        slots.release()
                    runtime.verify_output(args.output)
                finally:
                    wait([future for future in futures if not future.done()], timeout=5)

    def test_keyboard_interrupt_records_failure_before_and_after_drain(self):
        for after_drain in (False, True):
            with self.subTest(after_drain=after_drain):
                args, identity = self.prepare_control(0)
                futures = []

                class TrackingPool(ThreadPoolExecutor):
                    def submit(self, *arguments, **keywords):
                        future = super().submit(*arguments, **keywords)
                        futures.append(future)
                        return future

                def interrupted_wait(selected, timeout):
                    if timeout == 180:
                        if after_drain:
                            wait(selected, timeout=timeout)
                        raise KeyboardInterrupt("controlled scheduler interruption")
                    return wait(selected, timeout=timeout)

                try:
                    with mock.patch.object(runtime, "verify_build", return_value=identity), \
                            mock.patch.object(runtime, "verify_receipt", return_value=identity["build_identity"]), \
                            mock.patch.object(runtime, "ThreadPoolExecutor", TrackingPool), \
                            mock.patch.object(runtime, "wait", side_effect=interrupted_wait):
                        report = runtime.run(args)
                    self.assertTrue(all(future.done() for future in futures))
                    self.assertEqual(report["status"], "failed")
                    self.assertEqual(report["exit_code"], 2)
                    self.assertTrue(report["interrupted"])
                    self.assertEqual(report["error"], "KeyboardInterrupt: controlled scheduler interruption")
                    self.assertEqual(report["offered_terminal_rows"], 60)
                    rows = [json.loads(line) for line in (args.output / "raw.jsonl").read_text().splitlines()]
                    self.assertEqual(sorted(row["id"] for row in rows if row["kind"] == "operation"), list(range(60)))
                    self.assertEqual(json.loads((args.output / "report.json").read_text())["status"], "failed")
                    self.assertFalse((args.output / "statistics.json").exists())
                    runtime.verify_output(args.output)
                finally:
                    wait([future for future in futures if not future.done()], timeout=5)


class RuntimeEvidenceTests(unittest.TestCase):
    def test_exact_public_hit_rejects_query_echo_and_wrong_path(self):
        runtime.require_stable_symbol([{"name": runtime.QUERY, "file_path": "stable.py"}])
        for response in ({"query": runtime.QUERY}, [{"query": runtime.QUERY}],
                         [{"name": runtime.QUERY, "file_path": "wrong.py"}],
                         [{"name": "wrong", "file_path": "stable.py"}]):
            with self.assertRaises((AssertionError, RuntimeError, ValueError)):
                runtime.require_stable_symbol(response)

    def test_clustered_rss_cannot_certify_an_hour(self):
        second = 1_000_000_000
        complete = [{"at_ns": i * second} for i in range(3601)]
        self.assertTrue(runtime.sample_coverage(complete, 0, 3600 * second)["passed"])
        self.assertFalse(runtime.sample_coverage(complete[:10], 0, 3600 * second)["passed"])
        self.assertFalse(runtime.sample_coverage(complete[10:], 0, 3600 * second)["passed"])
        self.assertFalse(runtime.sample_coverage(complete[:10] + complete[20:], 0, 3600 * second)["passed"])
        self.assertFalse(runtime.sample_coverage(complete[::-1], 0, 3600 * second)["passed"])

    def test_zero_or_missing_rss_cannot_pass(self):
        for value in (None, 0, -1, True):
            result = runtime.rss_trend([{"server": {"resident_bytes": value}}] * 12)
            self.assertFalse(result["passed"])
            self.assertEqual(result["status"], "unavailable")

    def test_existing_memory_threshold_is_preserved(self):
        mib = 1024 * 1024
        samples = [{"server": {"resident_bytes": 100 * mib}}] * 12
        self.assertTrue(runtime.rss_trend(samples)["passed"])
        samples[-3:] = [{"server": {"resident_bytes": 158 * mib}}] * 3
        result = runtime.rss_trend(samples)
        self.assertFalse(result["passed"])
        self.assertEqual(result["allowed_bytes"], 157 * mib)

    def test_failed_outcomes_still_enter_latency_denominator(self):
        rows = [{"offered_ns": 0, "finished_ns": value, "status": status}
                for value, status in ((10, "success"), (100, "error"), (1000, "queue_rejected"))]
        result = runtime.latency_summary(rows)
        self.assertEqual(result["n"], 3)
        self.assertEqual(result["maximum_ns"], 1000)
        self.assertFalse(result["tail_stability_claim"])

    def test_mutations_use_real_git_and_do_not_grow_source(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "project"
            runtime.make_fixture(root, 8)
            head_before = runtime.git(root, "rev-parse", "HEAD")["stdout"]
            sizes = []
            for number in range(24):
                event = runtime.mutate(root, number)
                sizes.append(sum(p.stat().st_size for p in root.glob("*.py")))
                if number == 4:
                    self.assertEqual(event["action"], "real_git_branch_switch")
                    self.assertNotEqual(runtime.git(root, "rev-parse", "HEAD")["stdout"], head_before)
            self.assertLess(max(sizes) - min(sizes), 100)
            self.assertFalse((root / "temporary.py").exists())
            self.assertFalse((root / "renamed.py").exists())

    def test_unowned_git_mutation_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises((FileNotFoundError, AssertionError, RuntimeError, ValueError)):
                runtime.git(Path(tmp), "init")

    def test_raw_is_exclusive_and_preserves_every_row(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "raw.jsonl"
            raw = runtime.Raw(path)
            raw.emit("operation", id=1, status="error")
            raw.close()
            with self.assertRaises(FileExistsError): runtime.Raw(path)
            self.assertEqual(json.loads(path.read_text())["status"], "error")


if __name__ == "__main__":
    unittest.main()
