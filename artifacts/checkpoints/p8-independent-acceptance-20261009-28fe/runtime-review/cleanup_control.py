"""A bounded deterministic scheduler/retention failure with real stdio children."""
import json
from pathlib import Path
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, wait
from unittest import mock
from oracle_exit_control import runtime, setup

args, identity = setup(Path(sys.argv[1]), 0)
args.concurrency, args.operations = 4, 300
closed = threading.Event()
release = threading.Event()
futures = []
real_tool, real_close, real_raw = runtime.Product.tool, runtime.Product.close, runtime.Raw

class Pool(ThreadPoolExecutor):
    def submit(self, *a, **kw):
        future = super().submit(*a, **kw)
        futures.append(future)
        return future

class BudgetFailureRaw(real_raw):
    def emit(self, kind, **data):
        if kind == 'operation' and data.get('status') == 'queue_rejected':
            raise RuntimeError('runtime raw evidence budget exhausted')
        return super().emit(kind, **data)

def tool(self, name, arguments, timeout=45):
    response = real_tool(self, name, arguments, timeout)
    if self.output.name == 'product' and (name == 'search' or (name == 'index' and not arguments.get('full'))):
        if not release.wait(5):
            raise RuntimeError('bounded delayed client completion control')
    return response

def close(self, **kw):
    try:
        return real_close(self, **kw)
    finally:
        if self.output.name == 'product': closed.set()

def release_after_close():
    closed.wait(5)
    time.sleep(.25)
    release.set()

releaser = threading.Thread(target=release_after_close, daemon=True)
releaser.start()
with mock.patch.object(runtime, 'verify_build', return_value=identity), \
     mock.patch.object(runtime, 'verify_receipt', return_value=identity['build_identity']), \
     mock.patch.object(runtime, 'ThreadPoolExecutor', Pool), \
     mock.patch.object(runtime, 'Raw', BudgetFailureRaw), \
     mock.patch.object(runtime.Product, 'tool', tool), \
     mock.patch.object(runtime.Product, 'close', close):
    report = runtime.run(args)
    pending_after_return = sum(not future.done() for future in futures)
    wait(futures, timeout=5)
    releaser.join(timeout=1)
    try:
        runtime.verify_output(args.output)
        seal_after_completion = 'valid'
    except Exception as error:
        seal_after_completion = str(error)
    result = {'report_exit_code': report['exit_code'], 'report_error': report.get('error'),
              'submitted': len(futures), 'pending_after_return': pending_after_return,
              'later_source_add_exists': (args.output / 'project' / 'temporary.py').exists(),
              'seal_after_completion': seal_after_completion,
              'finished_futures': sum(f.done() for f in futures),
              'recorded_operations': sum(json.loads(line)['kind'] == 'operation' for line in (args.output/'raw.jsonl').read_text().splitlines())}
    (args.output.parent / 'control-result.json').write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))
