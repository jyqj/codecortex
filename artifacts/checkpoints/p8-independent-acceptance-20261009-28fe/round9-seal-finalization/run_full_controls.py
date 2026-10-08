from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import time

ROOT = Path('/workspace/scratch/28fef0db5e01/codecortex')
OUT = Path(__file__).resolve().parent
INPUTS = ('scripts/p8_runtime.py', 'scripts/tests/test_p8_runtime.py',
          'scripts/tests/test_p8_runtime_cache.py', 'scripts/tests/test_p8_runtime_finalization.py')
EXPECTED_PARENT = 'c2ad27b2b189cbc98775f20a550d718dd4913038'

def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT).decode().strip()

def hashes():
    return {p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest() for p in INPUTS}

assert git('rev-parse', 'HEAD') == EXPECTED_PARENT
assert git('diff', '--cached', '--name-only').splitlines() == [
    'scripts/p8_runtime.py', 'scripts/tests/test_p8_runtime_finalization.py']
assert not git('diff', '--name-only')
receipt = dict(schema_version=1, kind='actual_integrated_p8_python_regression_execution',
    cwd=str(ROOT), actual_head=git('rev-parse', 'HEAD'),
    staged_tree_before=git('write-tree'), inputs_before=hashes(),
    changed_inputs=git('diff', '--cached', '--name-only').splitlines(),
    argv=['python3', '-B', '-m', 'unittest', 'discover', '-s', 'scripts/tests', '-p', 'test_p8*.py', '-v'],
    started_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    scope='All P8 Python controls on the staged finalization integration; no native runtime or original TODO acceptance.')
(OUT / 'all-p8-running.json').write_text(json.dumps(receipt, indent=2) + '\n')
started = time.monotonic()
env = os.environ.copy()
env['PYTHONDONTWRITEBYTECODE'] = '1'
with (OUT / 'all-p8.log').open('xb') as log:
    result = subprocess.run(receipt['argv'], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
receipt.update(exit_code=result.returncode, elapsed_seconds=time.monotonic() - started,
    finished_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    inputs_after=hashes(), staged_tree_after=git('write-tree'))
raw = (OUT / 'all-p8.log').read_bytes()
receipt.update(log_bytes=len(raw), log_sha256=hashlib.sha256(raw).hexdigest(),
    inputs_unchanged=receipt['inputs_before'] == receipt['inputs_after'],
    staged_tree_unchanged=receipt['staged_tree_before'] == receipt['staged_tree_after'])
(OUT / 'all-p8-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt, indent=2))
print(raw.decode(errors='replace')[-1000:])
raise SystemExit(result.returncode if result.returncode else
                 (0 if receipt['inputs_unchanged'] and receipt['staged_tree_unchanged'] else 2))
