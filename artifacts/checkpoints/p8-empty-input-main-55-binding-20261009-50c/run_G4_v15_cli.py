import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path('/workspace/scratch/50c364fd60b1/codecortex')
out = Path('/workspace/scratch/50c364fd60b1/validation/main-55-integration/G4-v15-cli')
out.mkdir(exist_ok=False)
G = '2b60ae7bff2b4b2bf52257bb5cde600fc61c3858'
P = '31a42daeb12da6936695abb08eb3912d4f3c6064'
R = '20efd9664f1be2ca4705bec757505bcb530e7f48'
command = [sys.executable, '-B', 'scripts/verify_reviewed_source_v15.py',
           '--source-version', 'p8-completion-source-20261009-v15']

def source():
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    tree = subprocess.check_output(['git', 'rev-parse', 'HEAD^{tree}'], cwd=root, text=True).strip()
    changed = subprocess.check_output(['git', 'diff', '--name-only', G, '--'], cwd=root, text=True).splitlines()
    return {'head': head, 'tree': tree, 'tracked_changes_from_G4': changed,
            'registry_sha256': hashlib.sha256((root / 'scripts/reviewed-source-registry-v15.json').read_bytes()).hexdigest(),
            'verifier_sha256': hashlib.sha256((root / 'scripts/verify_reviewed_source_v15.py').read_bytes()).hexdigest()}

before = source()
assert before['head'] == G and before['tracked_changes_from_G4'] == []
receipt = {'source': G, 'product_source': P, 'review_source': R,
           'scope': 'Original v15 CLI only on G4, with the original source-version argument and 3600-second bound. No native unittest suite, full workspace, benchmark, TODO or release approval is claimed by this run.',
           'command': command, 'timeout_seconds': 3600,
           'started_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
           'source_before': before, 'status': 'running'}

def save():
    (out / 'receipt.json').write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n')

save()
started = time.monotonic()
with (out / 'v15-cli.log').open('xb') as log:
    try:
        result = subprocess.run(command, cwd=root, env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'},
                                stdout=log, stderr=subprocess.STDOUT, timeout=3600)
        receipt['exit_code'] = result.returncode
        receipt['status'] = 'completed'
    except subprocess.TimeoutExpired:
        receipt['exit_code'] = None
        receipt['status'] = 'timeout'
receipt['elapsed_seconds'] = time.monotonic() - started
receipt['completed_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
receipt['source_after'] = source()
receipt['source_unchanged'] = before == receipt['source_after']
receipt['log_sha256'] = hashlib.sha256((out / 'v15-cli.log').read_bytes()).hexdigest()
receipt['passed'] = receipt['exit_code'] == 0 and receipt['source_unchanged']
save()
print(json.dumps({k: v for k, v in receipt.items() if k not in {'source_before', 'source_after'}}, ensure_ascii=False), flush=True)
sys.exit(0 if receipt['passed'] else 1)
