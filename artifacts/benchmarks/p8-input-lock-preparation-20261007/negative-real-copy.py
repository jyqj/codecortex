#!/usr/bin/env python3
"""A report-membership negative control on an isolated copy of real inputs."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

BASE = Path('/workspace/scratch/71bc431c4e4f')
OUT = Path(__file__).resolve().parent
SCRIPT = BASE / 'codecortex-p8-input-lock/scripts/lock_benchmark_inputs.py'
LOCK = OUT / '019-input-lock.json'
SPEC = json.loads((OUT / 'input-spec.json').read_text())
PIN = json.loads((OUT / '019-input-lock-receipt.json').read_text())['lock_sha256']
ADDED = 'p7-019-validation/canonical-three-policy/run/later-unlisted-report.json'
assert not (BASE / ADDED).exists()
commands = []


def run(root, label):
    command = [sys.executable, '-B', str(SCRIPT), 'verify', '--root', str(root), '--lock', str(LOCK), '--expected-lock-sha256', PIN]
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    log = OUT / (label + '.log')
    with log.open('xb') as stream:
        stream.write(result.stdout)
    commands.append({'command': command, 'exit_code': result.returncode, 'log': log.name, 'log_sha256': hashlib.sha256(result.stdout).hexdigest()})
    return result


with tempfile.TemporaryDirectory(prefix='p8-lock-negative-', dir=BASE) as name:
    root = Path(name)
    for item in SPEC['inputs']:
        source, target = BASE / item['path'], root / item['path']
        target.parent.mkdir(parents=True, exist_ok=True)
        if item['kind'] == 'tree':
            shutil.copytree(source, target, dirs_exist_ok=True, copy_function=shutil.copy2)
        else:
            shutil.copy2(source, target)
    baseline = run(root, 'real-copy-matching')
    assert baseline.returncode == 0, baseline.stdout.decode()
    payload = b'{"origin":"negative_control_only","new_report":true}\n'
    (root / ADDED).write_bytes(payload)
    negative = run(root, 'real-copy-added-report')
    assert negative.returncode == 2
    assert b'directory membership changed' in negative.stdout
    assert not (BASE / ADDED).exists()
    copied_files = sum(path.is_file() for path in root.rglob('*')) - 1
assert not (BASE / ADDED).exists()
receipt = {
    'schema_version': 1, 'task': 'P8-001', 'scope': 'preparation_only',
    'execution_owner': '/root/todo_audit',
    'locker_source_commit': '1bb5ea9c6d66e9109fdaf8766606337eedec8219',
    'locker_script_sha256': hashlib.sha256(SCRIPT.read_bytes()).hexdigest(),
    'lock_sha256': PIN, 'commands': commands,
    'copy_method': 'shutil.copy2/copytree into a private temporary directory; no hardlinks or source writes',
    'copied_files': copied_files, 'added_member': ADDED,
    'added_bytes_sha256': hashlib.sha256(payload).hexdigest(),
    'original_added_path_absent_before_and_after': True,
    'temporary_copy_removed': True,
    'result': 'matching_copy_passed_and_unlisted_report_member_rejected',
    'rust_or_product_executed': False, 'new_benchmark_runs': 0,
    'distinct_new_test_function_credit': 0, 'release_candidate': False,
    'boundary': 'Two input-lock CLI checks on copied prior evidence; not new benchmark execution or quality evidence.',
    'control_script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
}
with (OUT / 'real-copy-drift-receipt.json').open('x') as stream:
    json.dump(receipt, stream, indent=2)
    stream.write('\n')
print(json.dumps({key: receipt[key] for key in ['result', 'copied_files', 'lock_sha256', 'new_benchmark_runs', 'release_candidate']}))
