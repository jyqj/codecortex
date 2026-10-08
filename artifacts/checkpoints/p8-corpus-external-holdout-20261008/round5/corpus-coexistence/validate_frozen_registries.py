#!/usr/bin/env python3
"""Reuse the original frozen F validator on both unchanged public DEV entries."""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import time

ROOT = Path('/workspace/scratch/031390cf22eb/codecortex-closeout')
OUT = Path('/workspace/scratch/031390cf22eb/round5-corpus-coexistence')
SOURCE = '56d15ed246eedd9add0c0d2843001b5f4e971ee8'
BIN = Path('/workspace/scratch/031390cf22eb/round4-validation/binaries/cc-eval')
BIN_SHA = '554d1baefeadd367614a0755a94b2db5a6194b284a83c03b7d8fe172368dd33d'
sha = lambda b: hashlib.sha256(b).hexdigest()


def head():
    return subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD']).decode().strip()


def snapshot(expected):
    result = {}
    for path, digest in expected.items():
        file = ROOT / path
        for parent in (file, *file.parents):
            assert not parent.is_symlink(), str(parent)
            if parent == ROOT:
                break
        assert stat.S_ISREG(file.lstat().st_mode)
        raw = file.read_bytes()
        assert sha(raw) == digest, path
        result[path] = {'sha256': digest, 'bytes': len(raw), 'mode': stat.S_IMODE(file.stat().st_mode)}
    return result


def invoke(command, log_path):
    started = datetime.now(timezone.utc).isoformat()
    tick = time.monotonic()
    with log_path.open('xb') as stream:
        result = subprocess.run(command, cwd=ROOT, env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'},
                                stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.STDOUT, timeout=180)
    return {'command': command, 'cwd': str(ROOT), 'exit_code': result.returncode,
        'started_utc': started, 'finished_utc': datetime.now(timezone.utc).isoformat(),
        'elapsed_seconds': time.monotonic() - tick, 'log': str(log_path), 'log_sha256': sha(log_path.read_bytes())}


audit_path = OUT / 'corpus-coexistence-independent.json'
audit = json.loads(audit_path.read_text())
expected = {p.split(':', 1)[1]: record['sha256'] for p, record in audit['inputs'].items() if p.startswith(SOURCE + ':')}
before_head = head()
before = snapshot(expected)
assert sha(BIN.read_bytes()) == BIN_SHA
assert not (OUT / 'native600-validation').exists()
assert not (OUT / 'parallel327-validation').exists()
command = ['python3', str(ROOT / 'scripts/p8_native_registry.py'), '--repo-root', str(ROOT),
           '--cc-eval', str(BIN), '--serde-source', '/workspace/scratch/031390cf22eb/session/upstream-serde',
           '--vite-source', '/workspace/scratch/031390cf22eb/upstream-vite',
           '--output-dir', str(OUT / 'native600-validation')]
helper = invoke(command, OUT / 'native600-validation-command.log')
calls = []
directory = OUT / 'parallel327-validation'
directory.mkdir()
index_path = ROOT / 'crates/cc-eval/benchmarks/manifests/public-dev-20261008.dataset-index.json'
index = json.loads(index_path.read_text())
for repo in index['repositories']:
    for profile, entry in repo['profiles'].items():
        suite = ROOT / entry['suite']['path']
        assert sha(suite.read_bytes()) == entry['suite']['sha256']
        result = invoke([str(BIN), 'validate', '--suite', str(suite)], directory / (repo['repository'] + '-' + profile + '.log'))
        calls.append({**result, 'repository': repo['repository'], 'profile': profile,
                      'rows': entry['rows'], 'suite_sha256': entry['suite']['sha256']})
after = snapshot(expected)
assert before == after and sha(BIN.read_bytes()) == BIN_SHA
helper_receipt_path = OUT / 'native600-validation/registry-validation.json'
helper_receipt = json.loads(helper_receipt_path.read_text()) if helper_receipt_path.exists() else None
passed = (helper['exit_code'] == 0 and helper_receipt is not None and helper_receipt['all_20_suites_validated']
          and len(calls) == 12 and all(c['exit_code'] == 0 for c in calls))
result = {
    'schema_version': 1, 'status': 'passed_32_existing_suite_validations' if passed else 'failed_input_validation',
    'auditor': '/root/p8_corpus_closeout', 'scope': 'actual original F validate only; no new locks, no retrieval and no ranking',
    'script_sha256': sha(Path(__file__).read_bytes()),
    'reviewed_input_source_commit': SOURCE, 'observed_worktree_head_before': before_head,
    'observed_worktree_head_after': head(), 'all_executed_registry_and_data_inputs_equal_fixed_source': True,
    'coexistence_audit_path': str(audit_path), 'coexistence_audit_sha256': sha(audit_path.read_bytes()),
    'validator_path': str(BIN), 'validator_sha256': BIN_SHA,
    'validator_source_commit': 'fb772551cff6b4620a6fcdb94c57b78350cebb33',
    'DEV600_materializer_command': helper,
    'DEV600_actual_20_suite_calls': helper_receipt['native_validator_calls'] if helper_receipt else [],
    'DEV600_registry_receipt': str(helper_receipt_path),
    'DEV600_registry_receipt_sha256': sha(helper_receipt_path.read_bytes()) if helper_receipt else None,
    'DEV327_actual_12_suite_calls': calls,
    'successful_validate_processes': (helper_receipt['successful_validator_calls'] if helper_receipt else 0) + sum(c['exit_code'] == 0 for c in calls),
    'unchanged_worktree_inputs_before_after': before,
    'inputs_unchanged': before == after,
    'DEV600_original_183_blob_export': 'original byte-identical snapshot export; no freeze or commit-null conversion',
    'DEV600_new_source_relocation': 'only source.root relocated to original fixed clean Serde/Vite checkouts',
    'DEV327_source_mode': 'original admitted source snapshots with commit=null; no suite edit or relocation',
    'native_unique_questions_remain': 626, 'compat_unique_projections_remain': 580,
    'new_input_freezes': 0, 'retrieval_calls': 0, 'protected_or_fresh_holdout_body_reads': 0,
    'limitations': ['32 validate calls cover two existing registry entries and duplicate historical questions; they are not 32 new datasets.',
                    'The fixed F validator validates input/source locks only; this is not a current-P2 product rebuild or quality result.',
                    'Original registry validation receipts and any original failed quality/Rust measurements remain unchanged.']
}
target = OUT / 'registry-input-validation-independent.json'
with target.open('x') as stream:
    json.dump(result, stream, ensure_ascii=False, indent=2, sort_keys=True)
    stream.write('\n')
print(json.dumps({'path': str(target), 'sha256': sha(target.read_bytes()), 'status': result['status'],
                  'successful_validate_processes': result['successful_validate_processes'],
                  'inputs_unchanged': result['inputs_unchanged'], 'worktree_input_count': len(before)}))
raise SystemExit(0 if passed else 1)
