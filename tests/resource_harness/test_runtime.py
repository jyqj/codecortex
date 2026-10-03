import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
from resource_harness.runtime import (
    EOF_TIMEOUT_SECONDS, MEMORY_GUARD_BYTES, READY_TIMEOUT_SECONDS,
    RPC_TIMEOUT_SECONDS, Journal, Observer, PendingRPC, RpcFailure,
    StdioRPC, guard_stop, memory_snapshot, process_snapshot,
)


class RuntimeTests(unittest.TestCase):
    def setUp(self):
        self.log = io.StringIO()
        self.journal = Journal(self.log)

    def rows(self):
        return [json.loads(line) for line in self.log.getvalue().splitlines()]

    def fixture(self, body):
        # Self-written Python only: no codecortex, database, GC, WAL, or signals.
        p = subprocess.Popen([sys.executable, '-u', '-c', body],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True)
        rpc = StdioRPC(p, self.journal)
        def cleanup():
            p.stdin.close()
            p.wait(timeout=EOF_TIMEOUT_SECONDS)
            rpc.join()
            p.stdout.close()
            p.stderr.close()
        self.addCleanup(cleanup)
        return p, rpc

    def test_protocol_constants_unchanged(self):
        self.assertEqual((MEMORY_GUARD_BYTES, RPC_TIMEOUT_SECONDS,
                          READY_TIMEOUT_SECONDS, EOF_TIMEOUT_SECONDS),
                         (12884901888, 300, 300, 15))

    def test_readonly_composition_phase_and_labels(self):
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)
            (p / 'memory.current').write_text('1000\n')
            (p / 'memory.stat').write_text('anon 100\nfile 800\nslab 50\nslab_reclaimable 40\n')
            (p / 'cgroup.procs').write_text('123\n')
            before = {f.name: f.read_bytes() for f in p.iterdir()}
            observer = Observer(self.journal, p, scope='shared_build_cache_runner',
                                processes={'fixture_root': 99999999})
            row = observer.begin_phase('generation')
            self.assertLessEqual(row['phase_start_ns'], row['cgroup']['read_start_ns'])
            self.assertEqual(row['cgroup']['group_path'], str(p.resolve()))
            self.assertEqual(row['cgroup']['composition_bytes']['anon'], 100)
            self.assertIsNone(row['cgroup']['composition_bytes']['slab_unreclaimable'])
            self.assertEqual(row['processes'][0]['role'], 'fixture_root')
            self.assertIsNone(row['processes'][0]['stat']['value'])
            self.assertIsNone(row['processes'][0]['tree_rss_bytes'])
            self.assertEqual(before, {f.name: f.read_bytes() for f in p.iterdir()})
            observer.begin_phase('cleanup')
            self.assertEqual(observer.sample()['phase'], 'cleanup')

    def test_missing_or_malformed_counters_unknown(self):
        with tempfile.TemporaryDirectory() as directory:
            row = memory_snapshot(directory, scope='fixture')
            self.assertIsNone(row['memory_current_bytes']['value'])
            self.assertTrue(row['memory_stat_counters']['error'])
            self.assertTrue(all(v is None for v in row['composition_bytes'].values()))
            (Path(directory) / 'memory.stat').write_text('anon invalid\n')
            self.assertIsNone(memory_snapshot(directory, scope='fixture')['memory_stat_counters']['value'])

    def test_all_pending_guard_woken_and_future_requests_rejected(self):
        pending = PendingRPC(self.journal)
        pairs = [pending.register('status', {}, phase='drain') for _ in range(4)]
        pending.terminal('resource_guard', 'memory_above_12GiB')
        for req, waiter in pairs:
            with self.assertRaises(RpcFailure) as caught:
                pending.wait(req, waiter)
            self.assertEqual(caught.exception.event['kind'], 'resource_guard')
        with self.assertRaises(RpcFailure):
            pending.register('status', {}, phase='drain')
        self.assertEqual(pending.pending, {})

    def test_response_guard_eof_exit_order_and_late_logs(self):
        for first in ('response', 'resource_guard', 'stdout_eof', 'process_exit'):
            pending = PendingRPC(self.journal)
            req, waiter = pending.register('status', {}, phase='drain')
            def deliver(kind):
                if kind == 'response':
                    pending.response({'id': req['id'], 'result': 'observed'}, phase='drain')
                else:
                    pending.terminal(kind)
            deliver(first)
            for kind in ('resource_guard', 'response', 'stdout_eof', 'process_exit'):
                deliver(kind)
            if first == 'response':
                self.assertEqual(pending.wait(req, waiter), 'observed')
            else:
                with self.assertRaises(RpcFailure) as caught:
                    pending.wait(req, waiter)
                self.assertEqual(caught.exception.event['kind'], first)
        self.assertEqual(sum(r['event'] == 'response' for r in self.rows()), 5)
        self.assertEqual(sum(r['event'] == 'terminal' for r in self.rows()), 15)

    def test_timeout_preserves_late_response_and_removes_pending(self):
        pending = PendingRPC(self.journal)
        req, waiter = pending.register('status', {}, phase='drain')
        with self.assertRaises(RpcFailure) as caught:
            pending.wait(req, waiter, timeout=.01)
        self.assertEqual(caught.exception.event['kind'], 'rpc_timeout')
        pending.response({'id': req['id'], 'result': 'late'}, phase='drain')
        self.assertFalse(pending.pending)
        self.assertEqual([r['event'] for r in self.rows()], ['request', 'rpc_timeout', 'response'])

    def test_guard_notifies_before_controlled_fixture_eof(self):
        p, rpc = self.fixture('import sys\nfor line in sys.stdin: pass\n')
        failures = []
        def wait():
            try:
                rpc.rpc('status', {})
            except RpcFailure as exc:
                failures.append(exc.event['kind'])
        thread = threading.Thread(target=wait)
        thread.start()
        # Registration barrier; no timing-dependent assumption about RPC startup.
        end = time.monotonic() + 2
        while not rpc.pending.pending and time.monotonic() < end:
            time.sleep(.001)
        self.assertTrue(rpc.pending.pending)
        def controlled_stop():
            self.assertEqual(rpc.pending.terminal_event['kind'], 'resource_guard')
            p.stdin.close()  # harmless fixture EOF, not a product kill/crash test
        start = time.monotonic()
        guard_stop(rpc, 'memory_above_12GiB', controlled_stop, observed_bytes=MEMORY_GUARD_BYTES + 1)
        thread.join(timeout=2)
        self.assertFalse(thread.is_alive())
        self.assertLess(time.monotonic() - start, 2)
        self.assertEqual(failures, ['resource_guard'])
        p.wait(timeout=2)
        rpc.join()
        self.assertIn('stdout_eof', [r.get('kind') for r in self.rows()])
        self.assertIn('process_exit', [r.get('kind') for r in self.rows()])

    def test_fixture_response_and_normal_eof_logs(self):
        p, rpc = self.fixture('import sys,json\nfor line in sys.stdin:\n r=json.loads(line); print(json.dumps({"id":r["id"],"result":{"ok":True}}),flush=True)\n')
        self.assertEqual(rpc.rpc('status', {}), {'ok': True})
        p.stdin.close()
        self.assertEqual(p.wait(timeout=2), 0)
        rpc.join()
        self.assertEqual(sum(r['event'] == 'response' for r in self.rows()), 1)

    def test_fixture_rpc_timeout_then_late_response_is_logged(self):
        p, rpc = self.fixture('import sys,json\nr=json.loads(sys.stdin.readline())\nsys.stdin.readline()\nprint(json.dumps({"id":r["id"],"result":"late"}),flush=True)\n')
        with self.assertRaises(RpcFailure) as caught:
            rpc.rpc('status', {}, timeout=.01)
        self.assertEqual(caught.exception.event['kind'], 'rpc_timeout')
        p.stdin.write('release\n')
        p.stdin.flush()
        p.wait(timeout=2)
        rpc.join()
        self.assertEqual(sum(r['event'] == 'response' for r in self.rows()), 1)

    def test_fixture_stdout_eof_wakes_before_root_exit(self):
        p, rpc = self.fixture('import os,sys\nsys.stdin.readline()\nos.close(1)\nfor line in sys.stdin: pass\n')
        with self.assertRaises(RpcFailure) as caught:
            rpc.rpc('status', {})
        self.assertEqual(caught.exception.event['kind'], 'stdout_eof')
        self.assertIsNone(p.poll())
        p.stdin.close()
        p.wait(timeout=2)
        rpc.join()

    def test_response_delivered_before_guard_does_not_clear_failed_gate(self):
        pending = PendingRPC(self.journal)
        req, waiter = pending.register('status', {}, phase='drain')
        pending.response({'id': req['id'], 'result': {'semantic_state': 'ready'}}, phase='drain')
        pending.terminal('resource_guard', 'memory_above_12GiB')
        self.assertEqual(pending.wait(req, waiter), {'semantic_state': 'ready'})
        with self.assertRaises(RpcFailure):
            pending.raise_if_terminal()

    def test_guard_and_response_concurrent_first_delivery_wins(self):
        pending = PendingRPC(self.journal)
        req, waiter = pending.register('status', {}, phase='drain')
        barrier = threading.Barrier(3)
        def respond():
            barrier.wait()
            pending.response({'id': req['id'], 'result': 'observed'}, phase='drain')
        def stop():
            barrier.wait()
            pending.terminal('resource_guard', 'memory_above_12GiB')
        threads = [threading.Thread(target=respond), threading.Thread(target=stop)]
        for thread in threads: thread.start()
        barrier.wait()
        for thread in threads: thread.join()
        first = self.rows()[1]
        if first['event'] == 'response':
            self.assertEqual(pending.wait(req, waiter), 'observed')
        else:
            with self.assertRaises(RpcFailure):
                pending.wait(req, waiter)
        self.assertFalse(pending.pending)
        self.assertEqual(len(self.rows()), 3)

    def test_fixture_exit_and_malformed_reader_wake_pending(self):
        for line in ('', 'invalid json'):
            with self.subTest(line=line):
                _, rpc = self.fixture('import sys\nsys.stdin.readline()\nprint(' + repr(line) + ',flush=True) if ' + repr(bool(line)) + ' else None\n')
                with self.assertRaises(RpcFailure) as caught:
                    rpc.rpc('status', {})
                self.assertIn(caught.exception.event['kind'], ('stdout_eof', 'process_exit', 'reader_error'))
                rpc.join()
                if line:
                    self.assertTrue(any(r.get('text') == line + '\n' for r in self.rows()))

    def test_concurrent_append_no_loss(self):
        threads = [threading.Thread(target=lambda: [self.journal.emit('fixture', index=i)
                                                   for i in range(100)]) for _ in range(4)]
        for thread in threads: thread.start()
        for thread in threads: thread.join()
        self.assertEqual(len(self.rows()), 400)


if __name__ == '__main__':
    unittest.main()
