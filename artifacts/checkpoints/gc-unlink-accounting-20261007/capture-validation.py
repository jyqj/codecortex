#!/usr/bin/env python3
"""Capture one fixed-source, offline GC validation phase without editing source."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

REPO = Path('/workspace/scratch/71bc431c4e4f/codecortex-p7-016')
OUT = REPO / 'artifacts/checkpoints/gc-unlink-accounting-20261007'
PHASE = sys.argv[1]
COMMANDS = {
    'baseline': [['cargo', '+1.95.0', 'test', '--offline', '--locked', '-j2', '-p', 'cc-semantic',
                  '--test', 'gc_unlink_accounting', '--message-format=json-render-diagnostics', '--', '--nocapture']],
    'repaired': [['cargo', '+1.95.0', 'test', '--offline', '--locked', '-j2', '-p', 'cc-semantic',
                  '--test', 'gc_unlink_accounting', '--test', 'semantic_gc',
                  '--message-format=json-render-diagnostics', '--', '--nocapture']],
    'checks': [
        ['cargo', '+1.95.0', 'clippy', '--offline', '--locked', '-j2', '-p', 'cc-semantic',
         '--lib', '--test', 'gc_unlink_accounting', '--test', 'semantic_gc', '--', '-D', 'warnings'],
        ['cargo', '+1.95.0', 'fmt', '--all', '--', '--check'],
    ],
}
assert PHASE in COMMANDS
OUT.mkdir(parents=True, exist_ok=True)
assert not (OUT / f'{PHASE}-validation.json').exists(), 'preserve prior receipts'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read(*argv):
    return subprocess.check_output(argv, cwd=REPO, text=True).strip()


def source_map():
    raw = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard',
                                   '-z', '--', 'crates', 'Cargo.toml', 'Cargo.lock'], cwd=REPO)
    return {name: digest((REPO / name).read_bytes()) for name in sorted(set(raw.decode().split('\0')) - {''})}


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


settings = {
    'CARGO_TARGET_DIR': '/workspace/scratch/71bc431c4e4f/target-validation-cost',
    'CARGO_BUILD_JOBS': '2', 'CARGO_PROFILE_DEV_DEBUG': '0', 'CARGO_PROFILE_TEST_DEBUG': '0',
    'CARGO_INCREMENTAL': '0', 'CARGO_TERM_COLOR': 'never',
    'CODECORTEX_GC_UNLINK_EVIDENCE_DIR': str(OUT / f'{PHASE}-observations'),
}
env = os.environ.copy()
env.update(settings)
before = source_map()
manifest = OUT / f'{PHASE}-source-sha256.json'
save(manifest, before)
receipt = {
    'phase': PHASE,
    'head': read('git', 'rev-parse', 'HEAD'),
    'tree': read('git', 'rev-parse', 'HEAD^{tree}'),
    'branch': read('git', 'branch', '--show-current'),
    'source_status_before': read('git', 'status', '--porcelain', '--', 'crates', 'Cargo.toml', 'Cargo.lock'),
    'source_inputs': len(before), 'source_manifest': manifest.name,
    'source_manifest_sha256': digest(manifest.read_bytes()),
    'rustc': read('rustc', '+1.95.0', '--version'),
    'cargo': read('cargo', '+1.95.0', '--version'),
    'environment': settings,
    'raw_logs_normalized': False,
    'not_run': ['full_workspace', 'provider_network', 'SIGKILL', 'GC_WAL_staging', 'source_guard', 'full_mark_unlink_race'],
    'commands': [],
}
for index, argv in enumerate(COMMANDS[PHASE], 1):
    path = OUT / f'{PHASE}-{index}.log'
    assert not path.exists(), 'preserve prior logs'
    start = time.monotonic()
    with path.open('wb') as log:
        proc = subprocess.run(argv, cwd=REPO, env=env, stdout=log, stderr=subprocess.STDOUT)
    raw = path.read_bytes()
    output = raw.decode(errors='replace')
    artifacts = []
    for line in output.splitlines():
        if not line.startswith('{'):
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get('reason') == 'compiler-artifact' and value.get('executable'):
            exe = Path(value['executable'])
            artifacts.append({
                'package_id': value['package_id'], 'manifest_path': value['manifest_path'],
                'target': value['target'], 'profile': value['profile'], 'features': value['features'],
                'fresh': value['fresh'], 'executable': str(exe), 'executable_bytes': exe.stat().st_size,
                'executable_sha256': digest(exe.read_bytes()),
            })
    summaries = []
    for match in re.finditer(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out', output):
        summaries.append(dict(status=match[1], **dict(zip(
            ['passed', 'failed', 'ignored', 'measured', 'filtered_out'], map(int, match.groups()[1:])))))
    row = {
        'argv': argv, 'exit_code': proc.returncode, 'elapsed_seconds': round(time.monotonic() - start, 3),
        'log': path.name, 'log_bytes': len(raw), 'log_sha256': digest(raw),
        'test_summaries': summaries, 'executable_artifacts': artifacts,
    }
    receipt['commands'].append(row)
    print(json.dumps({key: row[key] for key in ['argv', 'exit_code', 'elapsed_seconds', 'log', 'test_summaries']}), flush=True)
after = source_map()
receipt['source_inputs_unchanged'] = before == after
receipt['source_mismatches'] = sorted(name for name in before.keys() | after.keys() if before.get(name) != after.get(name))
receipt['head_after'] = read('git', 'rev-parse', 'HEAD')
receipt['source_status_after'] = read('git', 'status', '--porcelain', '--', 'crates', 'Cargo.toml', 'Cargo.lock')
receipt['command_exits_all_zero'] = all(row['exit_code'] == 0 for row in receipt['commands'])
save(OUT / f'{PHASE}-validation.json', receipt)
print(json.dumps({key: receipt[key] for key in ['phase', 'head', 'source_inputs', 'source_manifest_sha256', 'source_inputs_unchanged', 'command_exits_all_zero']}), flush=True)
sys.exit(0 if receipt['command_exits_all_zero'] and receipt['source_inputs_unchanged'] else 1)
