#!/usr/bin/env python3
"""Capture a fixed-source P7-015 phase; preserve all attempts and raw output."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

REPO = Path('/workspace/scratch/71bc431c4e4f/codecortex-p7-015')
OUT = REPO / 'artifacts/checkpoints/worker-contention-measurements-20261007'
TARGET = Path('/workspace/scratch/71bc431c4e4f/target-validation-cost')
PHASE = sys.argv[1]
COMMANDS = {
    'contention': [
        ['cargo', '+1.95.0', 'clean', '--offline', '--locked', '-p', 'cc-semantic', '-p', 'cc-server', '-p', 'cc-eval'],
        ['cargo', '+1.95.0', 'test', '--offline', '--locked', '-j2', '-p', 'cc-eval', '--features', 'semantic',
         '--test', 'p7_worker_contention', '--test', 'semantic_lifecycle',
         '--message-format=json-render-diagnostics', '--', '--nocapture'],
    ],
    'fairness': [
        ['cargo', '+1.95.0', 'test', '--offline', '--locked', '-j2', '-p', 'cc-server', '--features', 'semantic-http',
         '--test', 'p7_production_fairness_model_review',
         '--message-format=json-render-diagnostics', '--', '--nocapture'],
    ],
    'checks': [
        ['cargo', '+1.95.0', 'clippy', '--offline', '--locked', '-j2', '-p', 'cc-eval', '--features', 'semantic',
         '--test', 'p7_worker_contention', '--test', 'semantic_lifecycle', '--', '-D', 'warnings'],
        ['cargo', '+1.95.0', 'fmt', '--all', '--', '--check'],
    ],
}
assert PHASE in COMMANDS
OUT.mkdir(parents=True, exist_ok=True)
assert not (OUT / f'{PHASE}-validation.json').exists(), 'preserve prior receipts'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def read(*argv):
    return subprocess.check_output(argv, cwd=REPO, text=True).strip()


def source_map():
    raw = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard',
                                   '-z', '--', 'crates', 'Cargo.toml', 'Cargo.lock'], cwd=REPO)
    return {name: file_digest(REPO / name) for name in sorted(set(raw.decode().split('\0')) - {''})}


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


settings = {
    'CARGO_TARGET_DIR': str(TARGET),
    'CARGO_BUILD_BUILD_DIR': str(TARGET),
    'CARGO_BUILD_JOBS': '2', 'CARGO_PROFILE_DEV_DEBUG': '0', 'CARGO_PROFILE_TEST_DEBUG': '0',
    'CARGO_INCREMENTAL': '0', 'CARGO_TERM_COLOR': 'never',
    'RUSTC': '/workspace/scratch/71bc431c4e4f/rust-env/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc',
    'RUSTC_WRAPPER': '', 'RUSTC_WORKSPACE_WRAPPER': '',
    'CODECORTEX_WORKER_CONTENTION_EVIDENCE_DIR': str(OUT / f'{PHASE}-observations'),
    'CODECORTEX_LIFECYCLE_RECEIPT_DIR': str(OUT / f'{PHASE}-original-lifecycle'),
}
env = dict(os.environ, **settings)
before = source_map()
manifest = OUT / f'{PHASE}-source-sha256.json'
assert not manifest.exists(), 'preserve prior manifest'
save(manifest, before)
receipt = {
    'phase': PHASE,
    'head': read('git', 'rev-parse', 'HEAD'),
    'tree': read('git', 'rev-parse', 'HEAD^{tree}'),
    'branch': read('git', 'branch', '--show-current'),
    'source_status_before': read('git', 'status', '--porcelain', '--', 'crates', 'Cargo.toml', 'Cargo.lock'),
    'source_inputs': len(before), 'source_manifest': manifest.name,
    'source_manifest_sha256': file_digest(manifest),
    'rustc': read('rustc', '+1.95.0', '--version'),
    'selected_rustc_version': read(settings['RUSTC'], '--version', '--verbose'),
    'selected_rustc_sha256': file_digest(Path(settings['RUSTC'])),
    'cargo': read('cargo', '+1.95.0', '--version'),
    'environment': settings,
    'raw_logs_normalized': False,
    'shared_target_boundary': 'One Rust runner. Changed workspace packages cc-semantic/cc-server/cc-eval are explicitly cleaned in the contention phase before compilation. This is scoped cache invalidation, not a hermetic-build claim.',
    'not_run': ['full_workspace', 'external_provider', 'source_guard', 'full_P7_015_acceptance', 'bounded_reconcile_change'],
    'commands': [],
}
assert receipt['source_status_before'] == '', 'freeze source before validation'
for index, argv in enumerate(COMMANDS[PHASE], 1):
    path = OUT / f'{PHASE}-{index}.log'
    assert not path.exists(), 'preserve prior logs'
    started = time.monotonic()
    with path.open('xb') as log:
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
        if value.get('reason') != 'compiler-artifact':
            continue
        artifact_manifest = Path(value.get('manifest_path', ''))
        if not artifact_manifest.is_relative_to(REPO / 'crates'):
            continue
        files = []
        for filename in value['filenames']:
            product = Path(filename)
            files.append({'path': str(product), 'bytes': product.stat().st_size,
                          'sha256': file_digest(product)})
        executable = value.get('executable')
        artifacts.append({
            'package_id': value['package_id'], 'manifest_path': value['manifest_path'],
            'target': value['target'], 'profile': value['profile'], 'features': value['features'],
            'fresh': value['fresh'], 'executable': executable, 'files': files,
            'executable_sha256': file_digest(Path(executable)) if executable else None,
        })
    summaries = []
    for match in re.finditer(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out', output):
        summaries.append(dict(status=match[1], **dict(zip(
            ['passed', 'failed', 'ignored', 'measured', 'filtered_out'], map(int, match.groups()[1:])))))
    row = {
        'argv': argv, 'exit_code': proc.returncode, 'elapsed_seconds': round(time.monotonic() - started, 3),
        'log': path.name, 'log_bytes': len(raw), 'log_sha256': digest(raw),
        'test_summaries': summaries, 'workspace_artifacts': artifacts,
    }
    if PHASE == 'contention' and argv[2] == 'test':
        row['cleaned_workspace_libraries_recompiled'] = {
            name: any(a['target']['name'] == name and a['target']['kind'] == ['lib']
                      and a['fresh'] is False for a in artifacts)
            for name in ['cc_semantic', 'cc_server', 'cc_eval']
        }
    receipt['commands'].append(row)
    print(json.dumps({key: row[key] for key in ['argv', 'exit_code', 'elapsed_seconds', 'log', 'test_summaries']}), flush=True)
    if proc.returncode:
        break
after = source_map()
receipt['source_inputs_unchanged'] = before == after
receipt['source_mismatches'] = sorted(name for name in before.keys() | after.keys() if before.get(name) != after.get(name))
receipt['head_after'] = read('git', 'rev-parse', 'HEAD')
receipt['source_status_after'] = read('git', 'status', '--porcelain', '--', 'crates', 'Cargo.toml', 'Cargo.lock')
receipt['command_exits_all_zero'] = len(receipt['commands']) == len(COMMANDS[PHASE]) and all(row['exit_code'] == 0 for row in receipt['commands'])
receipt['cleaned_workspace_recompile_verified'] = all(
    all(row['cleaned_workspace_libraries_recompiled'].values())
    for row in receipt['commands'] if 'cleaned_workspace_libraries_recompiled' in row
) if PHASE == 'contention' and len(receipt['commands']) == 2 else None
save(OUT / f'{PHASE}-validation.json', receipt)
print(json.dumps({key: receipt[key] for key in ['phase', 'head', 'source_inputs', 'source_manifest_sha256', 'source_inputs_unchanged', 'command_exits_all_zero', 'cleaned_workspace_recompile_verified']}), flush=True)
sys.exit(0 if receipt['command_exits_all_zero'] and receipt['source_inputs_unchanged']
         and receipt['head'] == receipt['head_after']
         and receipt['cleaned_workspace_recompile_verified'] is not False else 1)
