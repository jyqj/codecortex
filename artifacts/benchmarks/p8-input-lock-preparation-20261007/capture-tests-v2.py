#!/usr/bin/env python3
"""Record one same-source stdlib control run; no Rust/product execution."""
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time

ROOT = Path('/workspace/scratch/71bc431c4e4f/codecortex-p8-input-lock')
OUT = Path(__file__).resolve().parent
LABEL = sys.argv[1]
assert LABEL in ('nonobject-baseline', 'final-controls')
PATHS = ['scripts/lock_benchmark_inputs.py', 'tests/evidence_lock/test_benchmark_input_lock.py', 'docs/BENCHMARK_INPUT_LOCK.md']


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args]).decode().strip()


def source():
    return {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in PATHS}


before = source()
head = git('rev-parse', 'HEAD')
assert not git('status', '--porcelain')
command = [sys.executable, '-B', '-m', 'unittest', 'discover', '-s', 'tests/evidence_lock', '-v']
started = time.monotonic()
result = subprocess.run(command, cwd=ROOT, env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'}, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
elapsed = time.monotonic() - started
assert source() == before and git('rev-parse', 'HEAD') == head
assert not git('status', '--porcelain')
with (OUT / (LABEL + '.log')).open('xb') as file:
    file.write(result.stdout)
match = re.search(rb'Ran (\d+) tests? in ([\d.]+)s', result.stdout)
receipt = {
    'schema_version': 1, 'task': 'P8-001', 'scope': 'preparation_only',
    'execution_owner': '/root/todo_audit', 'source_commit': head,
    'source_tree': git('rev-parse', 'HEAD^{tree}'), 'source_paths_sha256': before,
    'source_unchanged': True, 'command': command, 'cwd': str(ROOT),
    'python': {'version': platform.python_version(), 'implementation': platform.python_implementation(), 'executable': sys.executable},
    'exit_code': result.returncode, 'elapsed_seconds': round(elapsed, 6),
    'test_functions': int(match[1]) if match else None,
    'test_result': 'passed' if result.returncode == 0 and match else 'failed_or_incomplete',
    'log': (LABEL + '.log'), 'log_sha256': hashlib.sha256(result.stdout).hexdigest(),
    'capture_script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    'rust_or_product_executed': False, 'release_candidate': False,
    'counting': 'Stdlib test functions include multiple synthetic negative controls; they are not benchmark queries, Rust tests, live-provider checks or completed TODO IDs.',
}
with (OUT / (LABEL + '-receipt.json')).open('x') as file:
    json.dump(receipt, file, indent=2)
    file.write('\n')
print(json.dumps(receipt))
raise SystemExit(result.returncode)
