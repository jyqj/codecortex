import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

sys.dont_write_bytecode = True
root = Path('/workspace/scratch/50c364fd60b1/codecortex')
out = Path('/workspace/scratch/50c364fd60b1/validation/final-product-checks')
out.mkdir(exist_ok=False)
sys.path.insert(0, str(root / 'scripts'))
from p7_build_identity import source_snapshot

overrides = {
    'PATH': '/workspace/scratch/50c364fd60b1/cargo/bin:' + os.environ['PATH'],
    'CARGO_HOME': '/workspace/scratch/50c364fd60b1/cargo',
    'RUSTUP_HOME': '/workspace/scratch/50c364fd60b1/rustup',
    'CARGO_TARGET_DIR': '/workspace/scratch/50c364fd60b1/target-final',
    'CARGO_BUILD_JOBS': '4',
    'CARGO_INCREMENTAL': '0',
    'CARGO_PROFILE_DEV_DEBUG': '0',
    'CARGO_PROFILE_TEST_DEBUG': '0',
    'PYTHONDONTWRITEBYTECODE': '1',
}
env = {**os.environ, **overrides}
commands = [
    ('format', ['cargo', 'fmt', '--all', '--', '--check']),
    ('clippy', ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings']),
    ('workspace-tests', ['cargo', 'test', '--workspace', '--locked', '--offline']),
    ('eval-corpus', ['cargo', 'test', '-p', 'cc-eval', '--locked', '--offline', '--', 'integration_fixtures_and_corpus']),
]
record = {
    'scope': 'Engineering regression, no scale, latency, platform matrix, live provider or release certification',
    'started_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'environment_overrides': overrides,
    'rustc': subprocess.check_output(['rustc', '-Vv'], env=env, text=True),
    'source_before': source_snapshot(root),
    'commands': [],
}
def save():
    (out / 'receipt.json').write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
save()
for name, command in commands:
    started = time.monotonic()
    path = out / (name + '.log')
    with path.open('xb') as log:
        result = subprocess.run(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
    row = {'name': name, 'command': command, 'exit_code': result.returncode,
           'elapsed_seconds': time.monotonic() - started,
           'log': path.name, 'log_sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    record['commands'].append(row)
    save()
    print(json.dumps(row), flush=True)
    if result.returncode:
        break
record['source_after'] = source_snapshot(root)
record['source_inputs_equal'] = record['source_before']['inputs'] == record['source_after']['inputs']
record['completed_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
record['all_commands_passed'] = len(record['commands']) == len(commands) and all(x['exit_code'] == 0 for x in record['commands']) and record['source_inputs_equal']
save()
sys.exit(0 if record['all_commands_passed'] else 1)
