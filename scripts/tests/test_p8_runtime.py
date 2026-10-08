import argparse
from concurrent.futures import ThreadPoolExecutor, wait
from contextlib import ExitStack
import hashlib
import importlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runtime as runtime
from test_p8_runtime_evidence import ReceiptFixture


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


# Independent owned-writer controls: real stdio processes, actual thread state,
# and actual archive mutation. Timed joins alone are accelerated.

def _owned_digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def _owned_write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def _owned_writer_control(case, evidence):
    helper = RuntimeDriverControls()
    args, identity = helper.prepare_control(0)
    release, entered = threading.Event(), threading.Event()
    captures, futures, raw_objects, seals, joins, after_close = [], [], [], [], [], []
    real_product, real_raw = runtime.Product, runtime.Raw
    real_diagnostics, real_tool = runtime.diagnostics, runtime.Product.tool
    real_join, real_seal = threading.Thread.join, runtime.seal_output
    rollback = importlib.import_module('p8_rollback')
    transport_module = importlib.import_module('resource_harness.runtime')
    real_exit, real_write = transport_module.StdioRPC._exit, rollback.write_json
    target = 'full-product' if case.startswith('comparison_') else 'product'
    child_release = args.binary.parent / 'descendant-release'
    deadline_seconds = 15
    fail_once = [False]

    if case.endswith('reader_alive'):
        child_code = ("import json,time;from pathlib import Path;"
                      f"p=Path({str(child_release)!r});end=time.monotonic()+{deadline_seconds};"
                      "\nwhile not p.exists() and time.monotonic()<end: time.sleep(.005)\n"
                      "print(json.dumps({'jsonrpc':'2.0','method':'late-synthetic-notification'}),flush=True)\n")
        project_name = 'fresh-full' if target == 'full-product' else 'project'
        tail = ("\nfrom pathlib import Path\nimport subprocess\n"
                f"if Path(sys.argv[-1]).name == {project_name!r}:\n"
                f"    child=subprocess.Popen([sys.executable,'-c',{child_code!r}])\n"
                f"    Path({str(args.binary.parent / 'descendant-pid')!r}).write_text(str(child.pid))\n")
        with args.binary.open('a') as stream: stream.write(tail)
    elif case == 'stopped_close_error':
        with args.binary.open('a') as stream:
            stream.write("\nfrom pathlib import Path\nif Path(sys.argv[-1]).name == 'project': raise SystemExit(3)\n")
    identity['binary_sha256'] = runtime.digest(args.binary)

    class CapturingProduct(real_product):
        def __init__(self, *a, **kw):
            captures.append(self)
            super().__init__(*a, **kw)

    class CapturingRaw(real_raw):
        def __init__(self, *a, **kw):
            super().__init__(*a, **kw)
            raw_objects.append(self)
        def emit(self, kind, **fields):
            if self.stream.closed:
                after_close.append(dict(kind=kind, thread=threading.current_thread().name))
            return super().emit(kind, **fields)

    class CapturingPool(ThreadPoolExecutor):
        def submit(self, *a, **kw):
            future = super().submit(*a, **kw)
            futures.append(future)
            return future

    def belongs(thread, name):
        return any(getattr(p, 'output', Path()).name == target
                   and getattr(getattr(p, 'transport', None), name, None) is thread for p in captures)

    def joined(thread, timeout=None):
        held = ((case == 'sampler_alive' and thread.name == 'p8-runtime-self-samples')
                or (case.endswith('reader_alive') and belongs(thread, 'reader'))
                or (case.endswith('watcher_alive') and belongs(thread, 'exit_watcher')))
        if held and not release.is_set() and timeout is not None:
            joins.append(dict(thread=thread.name, requested_seconds=timeout,
                              actual_control_seconds=.03, alive_before=thread.is_alive()))
            return real_join(thread, .03)
        return real_join(thread, timeout)

    def diagnostics(product):
        result = real_diagnostics(product)
        if case == 'sampler_alive' and threading.current_thread().name == 'p8-runtime-self-samples':
            entered.set()
            if not release.wait(deadline_seconds): raise RuntimeError('synthetic sampler release deadline')
        return result

    def watcher(transport):
        selected = any(getattr(p, 'output', Path()).name == target
                       and getattr(p, 'process', None) is transport.process for p in captures)
        if case.endswith('watcher_alive') and selected:
            # The child actually exited; this real owned watcher has not yet
            # emitted its real terminal event or returned from its thread.
            transport.process.wait()
            entered.set()
            if not release.wait(deadline_seconds): raise RuntimeError('synthetic watcher release deadline')
        return real_exit(transport)

    def product_write(path, value):
        if (case.endswith('construction_failure') and Path(path).parent.name == target
                and Path(path).name == 'process.json' and not fail_once[0]):
            fail_once[0] = True
            raise OSError('synthetic process receipt failure after actual Popen and StdioRPC start')
        return real_write(path, value)

    def tool(product, name, arguments, timeout=45):
        result = real_tool(product, name, arguments, timeout)
        if (case == 'worker_unfinished' and product.output.name == 'product'
                and (name == 'search' or (name == 'index' and not arguments.get('full')))):
            entered.set()
            if not release.wait(deadline_seconds): raise RuntimeError('synthetic worker release deadline')
        return result

    def waited(selected, timeout):
        if case in ("worker_unfinished", "sampler_alive") and timeout == 180:
            if not entered.wait(5):
                raise RuntimeError("held real writer did not enter within control deadline")
        if case == 'worker_unfinished':
            return wait(selected, timeout=.03)
        return wait(selected, timeout=timeout)

    def states():
        result = []
        for p in captures:
            process, transport = getattr(p, 'process', None), getattr(p, 'transport', None)
            result.append(dict(output=str(getattr(p, 'output', 'not assigned')),
                process_exit_code=process.poll() if process is not None else None,
                process_created=process is not None,
                reader_alive=transport.reader.is_alive() if transport is not None else None,
                watcher_alive=transport.exit_watcher.is_alive() if transport is not None else None))
        return dict(products=result, live_sampler_threads=[t.name for t in threading.enumerate()
                    if t.name == 'p8-runtime-self-samples' and t.is_alive()],
                    unfinished_futures=sum(not f.done() for f in futures),
                    raw_closed=[r.stream.closed for r in raw_objects])

    def seal(path):
        seals.append(states())
        return real_seal(path)

    evidence.mkdir(parents=True, exist_ok=False)
    report, run_error = None, None
    try:
        with ExitStack() as stack:
            for obj, name, replacement in (
                (runtime, 'verify_build', lambda *a, **kw: identity),
                (runtime, 'verify_receipt', lambda *a, **kw: identity['build_identity']),
                (runtime, 'Product', CapturingProduct), (runtime, 'Raw', CapturingRaw),
                (runtime, 'ThreadPoolExecutor', CapturingPool),
                (runtime, 'diagnostics', diagnostics), (runtime, 'wait', waited),
                (runtime, 'seal_output', seal), (real_product, 'tool', tool),
                (threading.Thread, 'join', joined), (transport_module.StdioRPC, '_exit', watcher),
                (rollback, 'write_json', product_write)):
                stack.enter_context(mock.patch.object(obj, name, replacement))
            try: report = runtime.run(args)
            except BaseException as error: run_error = f'{type(error).__name__}: {error}'
            before = states()
            rows = [json.loads(line) for line in (args.output / 'raw.jsonl').read_text().splitlines()]
            operations = [r for r in rows if r['kind'] == 'operation']
            verification_before = None
            if (args.output / 'seal.json').exists():
                try: runtime.verify_output(args.output); verification_before = 'verified'
                except Exception as error: verification_before = f'{type(error).__name__}: {error}'
            shutil.copytree(args.output, evidence / 'before-release')
            release.set()
            child_release.write_text('release only this control descendant\n')
            wait(futures, timeout=3)
            # Collect the actual held writers before closing any streams. This
            # cleanup is test ownership, after the observed run() returned.
            for p in captures:
                process = getattr(p, 'process', None)
                if process is not None and process.poll() is None:
                    try: process.stdin.close()
                    except Exception: pass
                    try: process.wait(timeout=3)
                    except Exception: process.kill(); process.wait(timeout=3)
                transport = getattr(p, 'transport', None)
                if transport is not None:
                    real_join(transport.reader, 3); real_join(transport.exit_watcher, 3)
            for t in threading.enumerate():
                if t.name == 'p8-runtime-self-samples': real_join(t, 3)
            after = states()
            verification_after = None
            if (args.output / 'seal.json').exists():
                try: runtime.verify_output(args.output); verification_after = 'verified'
                except Exception as error: verification_after = f'{type(error).__name__}: {error}'
            shutil.copytree(args.output, evidence / 'after-release')
            before_files = {str(p.relative_to(evidence / 'before-release')): _owned_digest(p)
                for p in (evidence / 'before-release').rglob('*') if p.is_file()}
            after_files = {str(p.relative_to(evidence / 'after-release')): _owned_digest(p)
                for p in (evidence / 'after-release').rglob('*') if p.is_file()}
            changed = [p for p in before_files if before_files[p] != after_files.get(p)]
            sealed = (args.output / 'seal.json').exists()
            expected_seal = case in ('all_stopped', 'stopped_close_error')
            result = dict(case=case, synthetic_control_only=True, original_todo_certification=False,
                requested_operations=args.operations, submitted_futures=len(futures),
                raw_terminal_operations=len(operations), terminal_ids=sorted(r['id'] for r in operations),
                all_submitted_futures_terminal_at_return=before['unfinished_futures'] == 0,
                report=report, run_error=run_error, states_at_return=before, states_after_release=after,
                seal_call_states=seals, seal_exists=sealed, expected_seal=expected_seal,
                safety_expectation_met=(sealed == expected_seal and run_error is None),
                verify_before_release=verification_before, verify_after_release=verification_after,
                files_changed_after_return=changed, accelerated_real_joins=joins,
                emit_attempts_after_raw_close=after_close, held_thread_entered=entered.is_set(),
                construction_failure_triggered=fail_once[0])
            _owned_write(evidence / 'observation.json', result)
            _owned_write(evidence / 'before-release-sha256.json', before_files)
            _owned_write(evidence / 'after-release-sha256.json', after_files)
            return result
    finally:
        release.set()
        child_release.write_text('finally release own descendant\n')
        for p in captures:
            process = getattr(p, 'process', None)
            if process is not None and process.poll() is None:
                process.kill(); process.wait(timeout=3)
            transport = getattr(p, 'transport', None)
            if transport is not None:
                real_join(transport.reader, 3); real_join(transport.exit_watcher, 3)
                if not transport.reader.is_alive() and not transport.exit_watcher.is_alive():
                    p.raw.close(); p.stderr.close(); process.stdout.close()
        wait(futures, timeout=3)
        for raw in raw_objects:
            raw.close()
        helper.doCleanups()


class OwnedWriterSealControls(unittest.TestCase):
    def check_owned_writer_case(self, case):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory) / case
            value = _owned_writer_control(case, evidence)
            self.assertIsNone(value["run_error"])
            self.assertEqual(value["seal_exists"], value["expected_seal"])
            report, state = value["report"], value["states_at_return"]
            self.assertIs(report["task_complete"], False)
            expected_terminals = 0 if case == "main_construction_failure" else (59 if case == "worker_unfinished" else 60)
            self.assertEqual(value["raw_terminal_operations"], expected_terminals)
            if expected_terminals == 60:
                self.assertEqual(value["terminal_ids"], list(range(60)))
            self.assertEqual(state["raw_closed"], [value["expected_seal"]])
            if value["expected_seal"]:
                self.assertEqual(report["artifact_seal_status"], "sealed")
                self.assertEqual(report["artifact_seal"], "seal.json")
                self.assertEqual(value["verify_before_release"], "verified")
                self.assertEqual(value["verify_after_release"], "verified")
                self.assertEqual(value["files_changed_after_return"], [])
                self.assertEqual(len(value["seal_call_states"]), 1)
                self.assertEqual(report["exit_code"], 0 if case == "all_stopped" else 2)
                if case == "stopped_close_error":
                    self.assertEqual(report["status"], "failed")
                    self.assertEqual(state["products"][0]["process_exit_code"], 3)
            else:
                self.assertEqual(report["exit_code"], 2)
                self.assertEqual(report["status"], "failed")
                self.assertEqual(report["artifact_seal_status"], "unsealed_owned_writers")
                self.assertIsNone(report["artifact_seal"])
                self.assertIsNone(report["raw_sha256"])
                self.assertEqual(value["seal_call_states"], [])
                self.assertEqual(value["emit_attempts_after_raw_close"], [])
            cleanup = report["owned_cleanup"]
            if case == "sampler_alive":
                self.assertTrue(value["held_thread_entered"])
                self.assertEqual(state["live_sampler_threads"], ["p8-runtime-self-samples"])
                self.assertIs(cleanup["sampler_stopped"], False)
                self.assertIn("raw.jsonl", value["files_changed_after_return"])
            if case.endswith(("reader_alive", "watcher_alive")):
                target = "full-product" if case.startswith("comparison_") else "product"
                p = next(p for p in state["products"] if Path(p["output"]).name == target)
                self.assertEqual(p["process_exit_code"], 0)
                self.assertTrue(p["reader_alive" if case.endswith("reader_alive") else "watcher_alive"])
                self.assertIs(cleanup["comparison_product_stopped" if target == "full-product" else "product_stopped"], False)
                self.assertIn(target + "/rpc.jsonl", value["files_changed_after_return"])
            if case.endswith("construction_failure"):
                self.assertTrue(value["construction_failure_triggered"])
                target = "full-product" if case.startswith("comparison_") else "product"
                p = next(p for p in state["products"] if Path(p["output"]).name == target)
                self.assertTrue(p["process_created"])
                self.assertIsNone(p["process_exit_code"])
                self.assertTrue(p["reader_alive"])
                self.assertTrue(p["watcher_alive"])
                self.assertIs(cleanup["comparison_product_construction_pending" if target == "full-product" else "product_construction_pending"], True)
                self.assertEqual(value["submitted_futures"], expected_terminals)
            if case == "worker_unfinished":
                self.assertEqual(state["unfinished_futures"], 1)
                self.assertEqual(cleanup["unfinished_work"], 1)
                rows = [json.loads(line) for line in (evidence / "after-release/raw.jsonl").read_text().splitlines()]
                self.assertEqual(sorted(r["id"] for r in rows if r["kind"] == "operation"), list(range(60)))
                self.assertIn("raw.jsonl", value["files_changed_after_return"])
            after = value["states_after_release"]
            self.assertEqual(after["live_sampler_threads"], [])
            self.assertEqual(after["unfinished_futures"], 0)
            for p in after["products"]:
                self.assertIsNotNone(p["process_exit_code"])
                self.assertIs(p["reader_alive"], False)
                self.assertIs(p["watcher_alive"], False)

    def test_all_stopped_seal_contract(self):
        self.check_owned_writer_case('all_stopped')

    def test_sampler_alive_seal_contract(self):
        self.check_owned_writer_case('sampler_alive')

    def test_main_reader_alive_seal_contract(self):
        self.check_owned_writer_case('main_reader_alive')

    def test_comparison_reader_alive_seal_contract(self):
        self.check_owned_writer_case('comparison_reader_alive')

    def test_main_watcher_alive_seal_contract(self):
        self.check_owned_writer_case('main_watcher_alive')

    def test_comparison_watcher_alive_seal_contract(self):
        self.check_owned_writer_case('comparison_watcher_alive')

    def test_stopped_close_error_seal_contract(self):
        self.check_owned_writer_case('stopped_close_error')

    def test_main_construction_failure_seal_contract(self):
        self.check_owned_writer_case('main_construction_failure')

    def test_comparison_construction_failure_seal_contract(self):
        self.check_owned_writer_case('comparison_construction_failure')

    def test_worker_unfinished_seal_contract(self):
        self.check_owned_writer_case('worker_unfinished')


class RuntimeFailureLifecycleTests(unittest.TestCase):
    """Explicit protocol fakes with real executor threads; never product evidence."""
    def control(self, *, oracle_code=0, raw_failure=False, refuse_drain=False, drain_timeout=False):
        retained = os.environ.get("P8_RUNTIME_CONTROL_EVIDENCE_DIR")
        if retained:
            Path(retained).mkdir(parents=True, exist_ok=True)
            root = Path(tempfile.mkdtemp(prefix="runtime-protocol-", dir=retained))
        else:
            temporary = tempfile.TemporaryDirectory(prefix="runtime-protocol-")
            self.addCleanup(temporary.cleanup)
            root = Path(temporary.name)
        fixture = ReceiptFixture(root)
        out = root / "observation"
        args = SimpleNamespace(binary=fixture.binaries["codecortex"], oracle=fixture.binaries["p8-oracle"],
                               statistics=fixture.binaries["p8-runtime-statistics"],
                               build_receipt=fixture.out / "build-receipt.json", output=out,
                               concurrency=1, operations=180 if raw_failure else 60, files=8,
                               interval_ms=.001, profile="mixed")
        started, release, finished = threading.Event(), threading.Event(), threading.Event()
        submitted, raw_streams = [], []
        state = dict(seal_while_worker_live=False, statistics_calls=0, drain_timeouts=[])
        self.addCleanup(release.set)

        class FakeProcess:
            def __init__(self): self.returncode = None
            def poll(self): return self.returncode
            def kill(self): self.returncode = -9

        class FakeProduct:
            def __init__(self, identity, project, output, _cache):
                self.project, self.process = project, FakeProcess()
                self.transport = SimpleNamespace(reader=SimpleNamespace(is_alive=lambda: False),
                                                 exit_watcher=SimpleNamespace(is_alive=lambda: False))
                output.mkdir()
                (output / "product-stderr.log").write_text("protocol fake; no product execution\n")
            def initialize(self): pass
            def tool(self, name, arguments, timeout=45):
                if name == "index":
                    if (raw_failure or drain_timeout) and arguments.get("full") is False:
                        started.set()
                        assert release.wait(5), "protocol worker was not released by control cleanup"
                        (self.project / "worker-finished.txt").write_text("real thread finished its owned write\n")
                        finished.set()
                    return dict(parse_errors=[], resolution_freshness=dict(complete=True))
                return [dict(name=runtime.QUERY, file_path="stable.py")]
            def close(self, **_kwargs): self.process.returncode = 0

        original_wait, original_seal, original_raw = runtime.wait, runtime.seal_output, runtime.Raw

        class ObservedPool(runtime.ThreadPoolExecutor):
            def submit(self, *args, **kwargs):
                future = super().submit(*args, **kwargs)
                submitted.append(future)
                return future

        class BudgetRaw(original_raw):
            def __init__(self, path):
                super().__init__(path)
                raw_streams.append(self)
            def emit(self, kind, **data):
                if raw_failure and kind == "operation" and data.get("status") == "queue_rejected":
                    assert started.wait(2), "the actual executor worker must be active before offering failure"
                    runtime.MAX_RAW_BYTES = self.size  # Exercise the real Raw budget rejection.
                return super().emit(kind, **data)

        def wait(futures, timeout=None):
            state["drain_timeouts"].append(timeout)
            if drain_timeout and timeout == 180:
                assert started.wait(2)
                return set(), set(futures)  # The original drain boundary, without sleeping 180s.
            if (raw_failure or drain_timeout) and timeout == 70:
                if refuse_drain:
                    return set(), set(futures)  # Explicitly simulate a cleanup deadline expiration.
                release.set()
            return original_wait(futures, timeout=timeout)

        def seal(path):
            state["seal_while_worker_live"] = started.is_set() and not finished.is_set()
            result = original_seal(path)
            if raw_failure:
                release.set()
                assert finished.wait(2)
            return result

        def oracle(command, **_kwargs):
            Path(command[command.index("--output") + 1]).write_text(json.dumps(
                dict(exit_code=oracle_code, comparison=dict(equal=oracle_code == 0),
                     error="protocol infrastructure failure" if oracle_code == 2 else None)))
            return SimpleNamespace(returncode=oracle_code, stdout="protocol oracle only", stderr="")

        def statistics(_binary, output):
            state["statistics_calls"] += 1
            rows = [json.loads(line) for line in (output / "raw.jsonl").read_text().splitlines()]
            attempts = [row for row in rows if row["kind"] == "operation"]
            value = dict(exit_code=0, status="passed_observation", offered=len(attempts))
            (output / "statistics.json").write_text(json.dumps(value))
            return value

        identity = dict(build_identity={"protocol_control": True})
        with ExitStack() as stack:
            for name, value in (("Product", FakeProduct), ("Raw", BudgetRaw), ("wait", wait),
                                ("ThreadPoolExecutor", ObservedPool),
                                ("seal_output", seal), ("MAX_RAW_BYTES", 512 * 1024 * 1024),
                                ("diagnostics", lambda _product: {}),
                                ("mutate", lambda _project, number: dict(action="protocol_only", ordinal=number)),
                                ("verify_build", lambda *_args: identity),
                                ("verify_receipt", lambda *_args: identity["build_identity"]),
                                ("replay_statistics", statistics)):
                stack.enter_context(mock.patch.object(runtime, name, value))
            # Protocol source files only; no product index or Git mutation is executed.
            def make_fixture(project, files):
                project.mkdir()
                (project / "stable.py").write_text("# explicit protocol fixture only\n")
            stack.enter_context(mock.patch.object(runtime, "make_fixture", make_fixture))
            stack.enter_context(mock.patch.object(runtime.subprocess, "run", side_effect=oracle))
            try:
                state["report"] = runtime.run(args)
            except Exception as error:
                state["raised"] = f"{type(error).__name__}: {error}"
            finally:
                release.set()
                if started.is_set():
                    assert finished.wait(2)
                _, pending = original_wait([future for future in submitted if not future.cancelled()], timeout=2)
                assert not pending, "all real control workers must stop before fixture cleanup"
                for raw in raw_streams:
                    raw.close()
        state["seal_exists"] = (out / "seal.json").exists()
        state["root"] = str(root)
        (root / "control-observation.json").write_text(json.dumps(state, indent=2, sort_keys=True) + "\n")
        return state, out

    def test_oracle_difference_keeps_statistics_and_failure_exit_one(self):
        state, out = self.control(oracle_code=1)
        self.assertNotIn("raised", state)
        self.assertEqual(state["report"]["exit_code"], 1, state)
        self.assertEqual(state["report"]["outcomes"], {"success": 60})
        self.assertEqual(state["statistics_calls"], 1)
        self.assertEqual(json.loads((out / "statistics.json").read_text())["offered"], 60)
        self.assertIn("complete no-repair endpoint oracle failed", state["report"]["failures"])
        runtime.verify_output(out)

    def test_equal_oracle_still_keeps_passing_statistics(self):
        state, out = self.control()
        self.assertEqual(state["report"]["exit_code"], 0, state)
        self.assertEqual(state["statistics_calls"], 1)
        self.assertEqual(state["report"]["outcomes"], {"success": 60})
        runtime.verify_output(out)

    def test_oracle_infrastructure_failure_stays_exit_two(self):
        state, out = self.control(oracle_code=2)
        self.assertEqual(state["report"]["exit_code"], 2, state)
        self.assertEqual(state["statistics_calls"], 0)
        runtime.verify_output(out)

    def test_raw_budget_abort_drains_actual_worker_before_sealing(self):
        state, out = self.control(raw_failure=True)
        self.assertFalse(state["seal_while_worker_live"], state)
        self.assertNotIn("raised", state)
        self.assertEqual(state["report"]["exit_code"], 2)
        self.assertIn(70, state["drain_timeouts"])
        runtime.verify_output(out)

    def test_original_drain_deadline_has_one_cleanup_wait_and_all_terminal_rows(self):
        state, out = self.control(drain_timeout=True)
        self.assertNotIn("raised", state)
        self.assertEqual(state["report"]["exit_code"], 2)
        self.assertEqual(state["drain_timeouts"], [180, 70])
        rows = [json.loads(line) for line in (out / "raw.jsonl").read_text().splitlines()]
        attempts = [row for row in rows if row["kind"] == "operation"]
        self.assertEqual(sorted(row["id"] for row in attempts), list(range(60)))
        self.assertEqual(sum(row["status"] == "canceled" for row in attempts), 59)
        runtime.verify_output(out)

    def test_unconfirmed_worker_shutdown_cannot_create_a_seal(self):
        state, out = self.control(raw_failure=True, refuse_drain=True)
        self.assertFalse(state["seal_exists"], state)
        self.assertEqual(state["report"]["exit_code"], 2)
        self.assertEqual(state["report"]["artifact_seal_status"], "unsealed_owned_writers")
        self.assertIsNone(state["report"]["artifact_seal"])
        with self.assertRaises(FileNotFoundError): runtime.verify_output(out)


if __name__ == "__main__":
    unittest.main()
