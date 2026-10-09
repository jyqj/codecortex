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
out = Path('/workspace/scratch/50c364fd60b1/validation/lane-ownership-round10/engineering-checks')
out.mkdir(exist_ok=False)
product = sys.argv[1]
assert len(product) == 40 and all(c in '0123456789abcdef' for c in product)
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
    ('search-lib', ['cargo', 'test', '-p', 'cc-search', '--lib', '--locked', '--offline']),
    ('mechanism-anchor', ['cargo', 'test', '-p', 'cc-eval', '--lib', '--locked', '--offline', 'fixed_controls_match_current_product_source_once']),
    ('mechanism-prepare', [sys.executable, '-B', 'scripts/p7_mechanism_build.py', '--source', str(root), '--expected-head', product, '--output', '/workspace/scratch/50c364fd60b1/validation/lane-ownership-round10/mechanism-prepare', '--prepare-only']),
]
before = source_snapshot(root)
assert before['source_commit'] == product
assert subprocess.check_output(['git', 'diff', '--name-only', product, '--'], cwd=root) == b''
record = {
    'scope': 'Actual fixed-P5 lane registry engineering regression; not a workspace-wide, full-scale, platform, provider or release certification.',
    'source_before': before,
    'expected_product_source': product,
    'started_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'environment_overrides': overrides,
    'rustc': subprocess.check_output(['rustc', '-Vv'], env=env, text=True),
    'commands': [],
    'workspace_full_suite': 'not_run_on_P5; retained P2 exit 101 and original G CI retain their own identities',
    'mechanism_scope': 'Original prepare-only validates source controls and materializes four variants; it does not compile variants, exercise a provider, certify a mechanism ablation or run a study.',
}

def save():
    (out / 'receipt.json').write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')

save()
for name, command in commands:
    head_before = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    assert head_before == product
    started = time.monotonic()
    path = out / (name + '.log')
    with path.open('xb') as log:
        result = subprocess.run(command, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
    head_after = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    row = {'name': name, 'command': command, 'exit_code': result.returncode,
           'elapsed_seconds': time.monotonic() - started,
           'head_before': head_before, 'head_after': head_after,
           'log': path.name, 'log_sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    record['commands'].append(row)
    save()
    print(json.dumps(row), flush=True)
    if result.returncode or head_after != product:
        break
record['source_after'] = source_snapshot(root)
record['source_inputs_equal'] = record['source_before']['inputs'] == record['source_after']['inputs']
record['completed_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
record['all_selected_commands_passed'] = (
    len(record['commands']) == len(commands)
    and all(c['exit_code'] == 0 and c['head_after'] == product for c in record['commands'])
    and record['source_inputs_equal']
    and record['source_after']['source_commit'] == product
)
save()
sys.exit(0 if record['all_selected_commands_passed'] else 1)
