#!/usr/bin/env python3
"""Independent synthetic controls. Never product or original-TODO certification.

Only build admission is mocked. Real Product, RPC, process/thread state, raw
writers, fixture mutation and seals are exercised. Timed joins are accelerated
for held real threads; no is_alive(), Product.close(), or seal result is faked.
"""
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
import threading
import time
from unittest import mock

CASES = ('all_stopped', 'sampler_alive', 'main_reader_alive', 'comparison_reader_alive',
         'main_watcher_alive', 'comparison_watcher_alive', 'stopped_close_error',
         'main_construction_failure', 'comparison_construction_failure', 'worker_unfinished')


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def exercise(case, evidence, runtime, base):
    helper = base.RuntimeDriverControls()
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
            before_files = {str(p.relative_to(evidence / 'before-release')): digest(p)
                for p in (evidence / 'before-release').rglob('*') if p.is_file()}
            after_files = {str(p.relative_to(evidence / 'after-release')): digest(p)
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
            write(evidence / 'observation.json', result)
            write(evidence / 'before-release-sha256.json', before_files)
            write(evidence / 'after-release-sha256.json', after_files)
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


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--source', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--cases', nargs='+', choices=CASES, default=CASES)
    p.add_argument('--expect', choices=('observe', 'safe'), default='observe')
    a = p.parse_args()
    sys.path[:0] = [str(a.source / 'scripts'), str(a.source / 'scripts/tests')]
    runtime = importlib.import_module('p8_runtime')
    base = importlib.import_module('test_p8_runtime')
    a.output.mkdir(parents=True, exist_ok=False)
    results = []
    for case in a.cases:
        result = exercise(case, a.output / case, runtime, base)
        results.append(result)
        print(json.dumps({k: result[k] for k in ('case', 'seal_exists', 'expected_seal',
                         'safety_expectation_met', 'raw_terminal_operations', 'run_error',
                         'files_changed_after_return')}), flush=True)
    write(a.output / 'results.json', dict(schema_version=1,
        scope='independent_synthetic_owned_writer_lifecycle_control_only',
        source=str(a.source), runtime_sha256=digest(runtime.__file__),
        control_sha256=digest(__file__), cases=results,
        original_todo_certification=False, all_expectations_met=all(r['safety_expectation_met'] for r in results)))
    return int(a.expect == 'safe' and not all(r['safety_expectation_met'] for r in results))

if __name__ == '__main__':
    raise SystemExit(main())
