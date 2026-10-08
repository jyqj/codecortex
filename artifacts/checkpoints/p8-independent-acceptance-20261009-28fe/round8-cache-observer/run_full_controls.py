from pathlib import Path
import datetime
import hashlib
import json
import os
import subprocess
import time

root = Path('/workspace/scratch/28fef0db5e01/codecortex')
out = Path(__file__).resolve().parent
inputs = ('scripts/p8_runtime.py', 'scripts/tests/test_p8_runtime_cache.py', 'scripts/tests/test_p8_runtime.py')
def hashes():
    return {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in inputs}
def git(*args):
    return subprocess.check_output(['git', *args], cwd=root).decode().strip()
receipt = {
    'schema_version': 1,
    'kind': 'actual_integrated_p8_python_regression_execution',
    'cwd': str(root),
    'actual_head': git('rev-parse', 'HEAD'),
    'staged_tree_before': git('write-tree'),
    'changed_inputs': git('diff', '--cached', '--name-only').splitlines(),
    'inputs_before': hashes(),
    'argv': ['python3', '-B', '-m', 'unittest', 'discover', '-s', 'scripts/tests', '-p', 'test_p8*.py', '-v'],
    'started_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'scope': 'All P8 Python controls on the staged integration; not a native one-hour soak or original TODO completion.'
}
(out / 'all-p8-running.json').write_text(json.dumps(receipt, indent=2) + '\n')
started = time.monotonic()
environment = os.environ.copy()
environment['PYTHONDONTWRITEBYTECODE'] = '1'
with (out / 'all-p8.log').open('xb') as log:
    result = subprocess.run(receipt['argv'], cwd=root, env=environment, stdout=log, stderr=subprocess.STDOUT)
receipt.update(exit_code=result.returncode, elapsed_seconds=time.monotonic() - started,
               finished_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
               inputs_after=hashes(), staged_tree_after=git('write-tree'))
raw = (out / 'all-p8.log').read_bytes()
receipt.update(log_bytes=len(raw), log_sha256=hashlib.sha256(raw).hexdigest())
receipt['inputs_unchanged'] = receipt['inputs_before'] == receipt['inputs_after']
receipt['staged_tree_unchanged'] = receipt['staged_tree_before'] == receipt['staged_tree_after']
(out / 'all-p8-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt, indent=2))
print(raw.decode(errors='replace')[-1500:])
raise SystemExit(result.returncode if result.returncode else (0 if receipt['inputs_unchanged'] and receipt['staged_tree_unchanged'] else 2))
