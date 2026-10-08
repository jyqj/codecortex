from pathlib import Path
import argparse
import datetime
import hashlib
import json
import os
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('--expected-head', required=True)
args = parser.parse_args()
root = Path('/workspace/scratch/28fef0db5e01/codecortex')
out = Path('/workspace/scratch/28fef0db5e01/integration-validation/round8-pins5-source-proof')
out.mkdir(exist_ok=False)
paths = (
    'scripts/verify_reviewed_source_v15.py',
    'scripts/reviewed-source-registry-v15.json',
    'artifacts/checkpoints/p8-completion-20261009/independent-source-review.json',
    'docs/roadmap/code-index-v2/tasks.json',
    'scripts/p8_runtime.py',
    'scripts/tests/test_p8_runtime_cache.py',
    'scripts/tests/test_p8_runtime.py',
)
def git(*items):
    return subprocess.check_output(['git', *items], cwd=root).decode().strip()
def hashes():
    return {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in paths}
def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()
def save():
    temporary = out / 'receipt.tmp'
    temporary.write_text(json.dumps(receipt, indent=2) + '\n')
    temporary.replace(out / 'receipt.json')

if git('rev-parse', 'HEAD') != args.expected_head:
    raise RuntimeError('Proof must run at the actual fixed PINS5 commit')
if git('diff', '--name-only') or git('diff', '--cached', '--name-only'):
    raise RuntimeError('Proof source must have no tracked working or staged changes')
receipt = {'schema_version': 1, 'kind': 'original_source_gates_on_actual_PINS5',
           'cwd': str(root), 'head_before': git('rev-parse', 'HEAD'), 'tree_before': git('rev-parse', 'HEAD^{tree}'),
           'inputs_before': hashes(), 'started_at_utc': utc(), 'status': 'running', 'commands': [],
           'scope': 'Actual original source-integrity tests, full v15 including historical v14 proof, historical integrations, plan and facts gates. Runtime acceptance remains separate.'}
save()
environment = os.environ.copy()
environment['PYTHONDONTWRITEBYTECODE'] = '1'
commands = [
    ['python3', '-B', '-m', 'unittest', 'discover', '-s', 'tests/source_integrity', '-v'],
    ['python3', '-B', 'scripts/verify_reviewed_source_v15.py', '--source-version', 'p8-completion-source-20261009-v15'],
    ['python3', '-B', 'scripts/verify_historical_integrations_v2.py'],
    ['python3', '-B', 'scripts/code_index_plan.py'],
    ['python3', '-B', 'scripts/p8_facts.py', '--check'],
    ['git', 'diff', '--check'],
]
for number, command in enumerate(commands):
    entry = {'argv': command, 'started_at_utc': utc(), 'status': 'running', 'log': f'{number:02d}.log'}
    receipt['commands'].append(entry)
    save()
    began = time.monotonic()
    log = out / entry['log']
    with log.open('xb') as output:
        result = subprocess.run(command, cwd=root, env=environment, stdout=output, stderr=subprocess.STDOUT)
    raw = log.read_bytes()
    entry.update(exit_code=result.returncode, elapsed_seconds=time.monotonic() - began, finished_at_utc=utc(),
                 log_bytes=len(raw), log_sha256=hashlib.sha256(raw).hexdigest(), status='passed' if result.returncode == 0 else 'failed')
    save()
    print(json.dumps(entry), flush=True)
receipt.update(head_after=git('rev-parse', 'HEAD'), tree_after=git('rev-parse', 'HEAD^{tree}'), inputs_after=hashes(), finished_at_utc=utc())
receipt['source_unchanged'] = receipt['head_before'] == receipt['head_after'] and receipt['tree_before'] == receipt['tree_after'] and receipt['inputs_before'] == receipt['inputs_after']
receipt['passed'] = receipt['source_unchanged'] and all(c['exit_code'] == 0 for c in receipt['commands'])
receipt['status'] = 'passed' if receipt['passed'] else 'failed'
save()
print(json.dumps({'status': receipt['status'], 'head': receipt['head_after'], 'source_unchanged': receipt['source_unchanged']}), flush=True)
raise SystemExit(0 if receipt['passed'] else 2)
