#!/usr/bin/env python3
"""External observer for the exact two source-integrity commands in S's CI."""
import ast
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import time

ROOT = Path('/workspace/scratch/031390cf22eb')
REPO = ROOT / 'codecortex-source-integrity-final'
OUT = ROOT / 'round4-validation/source-integrity-independent'
SOURCE = 'd5dfebd4f9add3694ec678830e6195c8d38b96f3'
TREE = 'fd91b781fb7f9745f240aaccd12961f85cc3ab0c'
P = '615662bd0e651dc40a9d1d0d6757bc28646b900d'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(*args):
    return subprocess.check_output(['git', *args], cwd=REPO)


def write(name, value):
    (OUT / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')


def snapshot(expected):
    actual = {}
    for rel, expected_hash in sorted(expected.items()):
        path = REPO
        for part in Path(rel).parts:
            path /= part
            assert not path.is_symlink(), rel
        assert path.is_file(), rel
        data = path.read_bytes()
        digest = sha(data)
        assert digest == expected_hash, (rel, digest, expected_hash)
        actual[rel] = {'sha256': digest, 'bytes': len(data), 'mode': oct(path.stat().st_mode & 0o777)}
    return {'head': git('rev-parse', 'HEAD').decode().strip(), 'tree': git('rev-parse', 'HEAD^{tree}').decode().strip(), 'git_status': git('status', '--porcelain').decode(), 'files': actual}


def execute(label, command, env):
    started = datetime.now(timezone.utc).isoformat()
    start = time.monotonic()
    samples = []
    logfile = OUT / (label + '.log')
    with logfile.open('wb') as stream:
        process = subprocess.Popen(command, cwd=REPO, env=env, stdout=stream, stderr=subprocess.STDOUT)
        write(label + '-running.json', {'source': SOURCE, 'command': command, 'cwd': str(REPO), 'pid': process.pid, 'started_utc': started, 'log': str(logfile), 'status': 'running_not_passed'})
        while True:
            samples.append({'elapsed_seconds': round(time.monotonic() - start, 3), 'free_bytes': shutil.disk_usage(ROOT).free})
            try:
                code = process.wait(timeout=5)
                break
            except subprocess.TimeoutExpired:
                pass
    result = {'source': SOURCE, 'tree': TREE, 'command': command, 'cwd': str(REPO), 'started_utc': started, 'finished_utc': datetime.now(timezone.utc).isoformat(), 'elapsed_seconds': round(time.monotonic() - start, 6), 'exit_code': code, 'log': str(logfile), 'log_sha256': sha(logfile.read_bytes()), 'disk_samples': samples, 'minimum_observed_free_bytes': min(x['free_bytes'] for x in samples), 'status': 'passed' if code == 0 else 'failed'}
    write(label + '-execution.json', result)
    print(json.dumps({k: result[k] for k in ['source', 'command', 'elapsed_seconds', 'exit_code', 'log_sha256', 'minimum_observed_free_bytes']}, ensure_ascii=False), flush=True)
    return result


def main():
    assert git('rev-parse', 'HEAD').decode().strip() == SOURCE
    assert git('rev-parse', 'HEAD^{tree}').decode().strip() == TREE
    assert git('status', '--porcelain') == b''
    original = json.loads((OUT / 'original-tests-at-P.json').read_bytes())
    assert original['source_commit'] == P and original['method_count'] == 174
    for path, digest in original['files_sha256'].items():
        assert sha((REPO / path).read_bytes()) == digest, path

    guard_path = 'scripts/verify_reviewed_source_v14.py'
    guard_bytes = (REPO / guard_path).read_bytes()
    assert sha(guard_bytes) == 'b1b23a48fddae0215f8bba4319426e9057f8e590ba2f3082bc2fa5844ad6fa12'
    pin_names = {'PRODUCT', 'REVIEW', 'REVIEW_PATH', 'REGISTRY_SHA256'}
    normalized = []
    pins = {}
    for raw in [git('show', f'{P}:{guard_path}'), guard_bytes]:
        module = ast.parse(raw)
        for node in module.body:
            if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name) and node.targets[0].id in pin_names:
                pins[node.targets[0].id] = ast.literal_eval(node.value)
                node.value = ast.Constant(value='<fixed-pin>')
        normalized.append(ast.dump(module, include_attributes=False))
    assert normalized[0] == normalized[1]
    assert pins['PRODUCT'] == P
    registry_path = 'scripts/reviewed-source-registry-v14.json'
    registry_bytes = (REPO / registry_path).read_bytes()
    assert sha(registry_bytes) == pins['REGISTRY_SHA256']
    registry = json.loads(registry_bytes)
    review_bytes = (REPO / pins['REVIEW_PATH']).read_bytes()
    review = json.loads(review_bytes)
    assert sha(review_bytes) == registry['review_sha256']
    assert review['complete_inputs'] == registry['complete_inputs']
    assert review['validation_inputs'] == registry['validation_inputs']
    expected = dict(registry['complete_inputs'])
    expected.update(registry['validation_inputs'])
    expected.update({guard_path: sha(guard_bytes), registry_path: sha(registry_bytes), pins['REVIEW_PATH']: sha(review_bytes)})
    workflow_path = '.github/workflows/ci.yml'
    workflow_bytes = (REPO / workflow_path).read_bytes()
    expected[workflow_path] = sha(workflow_bytes)
    workflow = workflow_bytes.decode()
    unit_line = 'PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/source_integrity -v'
    assert workflow.count(unit_line) == 1
    cli_lines = [line.strip() for line in workflow.splitlines() if re.match(r'\s*python3 scripts/verify_reviewed_source_v14\.py --source-version ', line)]
    assert len(cli_lines) == 1
    cli = shlex.split(cli_lines[0])
    assert cli == ['python3', 'scripts/verify_reviewed_source_v14.py', '--source-version', 'p8-oracle-compat-source-20261008-v14']
    before = snapshot(expected)
    assert before['git_status'] == ''
    write('S-inputs-before.json', before)
    env = dict(os.environ)
    env['PYTHONDONTWRITEBYTECODE'] = '1'
    tests = execute('original-174', ['python3', '-m', 'unittest', 'discover', '-s', 'tests/source_integrity', '-v'], env)
    # Do not rewrite, retry, substitute tests, or mark an incomplete run as pass.
    test_text = (OUT / 'original-174.log').read_text()
    rows = re.findall(r'^test_\S+ \(([^\n]+)\) \.\.\. (ok|FAIL|ERROR|skipped[^\n]*)$', test_text, re.M)
    actual_ids = [test_id for test_id, _result in rows]
    expected_ids = [row['id'] for row in original['methods']]
    identity_check = {'expected_count': len(expected_ids), 'reported_test_counts': [int(x) for x in re.findall(r'^Ran (\d+) tests? in ', test_text, re.M)], 'logged_method_count': len(actual_ids), 'missing_method_ids': sorted(set(expected_ids) - set(actual_ids)), 'unexpected_method_ids': sorted(set(actual_ids) - set(expected_ids)), 'duplicate_method_ids': sorted({x for x in actual_ids if actual_ids.count(x) > 1}), 'non_ok_methods': [{'id': test_id, 'result': result} for test_id, result in rows if result != 'ok']}
    write('original-174-method-identity.json', identity_check)
    guard = execute('v14-guard-cli', cli, env)
    after = snapshot(expected)
    write('S-inputs-after.json', after)
    assert after == before, 'fixed source or validation inputs changed during execution'
    complete = tests['exit_code'] == 0 and guard['exit_code'] == 0 and identity_check['reported_test_counts'] == [174] and identity_check['logged_method_count'] == 174 and not any(identity_check[key] for key in ['missing_method_ids', 'unexpected_method_ids', 'duplicate_method_ids', 'non_ok_methods'])
    report = {'schema_version': 1, 'reviewer': '/root/p8_corpus_closeout', 'status': 'passed_declared_source_integrity_scope' if complete else 'failed_or_incomplete', 'source': SOURCE, 'tree': TREE, 'reviewed_product_P': P, 'pins': pins, 'source_input_count': len(registry['complete_inputs']), 'validation_input_count': len(registry['validation_inputs']), 'snapshot_file_count': len(before['files']), 'test_related_files_equal_original_P': len(original['files_sha256']), 'guard_predicate_AST_unchanged_except_four_pins': True, 'original_174': tests, 'method_identity': identity_check, 'v14_guard_cli': guard, 'all_source_and_validation_inputs_unchanged': after == before, 'before_sha256': sha((OUT / 'S-inputs-before.json').read_bytes()), 'after_sha256': sha((OUT / 'S-inputs-after.json').read_bytes()), 'original_P_test_inventory_sha256': sha((OUT / 'original-tests-at-P.json').read_bytes()), 'observer_sha256': sha(Path(__file__).read_bytes()), 'limits': ['Original source-integrity tests and CLI only; no Rust/full CI, product quality, live provider or release certification claim.', 'This run does not change the original canonical exit2 or fresh-holdout exit1 measurements.', 'No test/guard predicate, source pin, fixture or scorer was altered to obtain this result.'], 'bytecode_housekeeping': 'PYTHONDONTWRITEBYTECODE=1 for both commands; no semantic test or guard override.'}
    write('final-S-source-integrity.json', report)
    print(json.dumps({'receipt': str(OUT / 'final-S-source-integrity.json'), 'sha256': sha((OUT / 'final-S-source-integrity.json').read_bytes()), 'status': report['status'], 'method_identity': identity_check}, ensure_ascii=False), flush=True)
    raise SystemExit(0 if complete else 1)


if __name__ == '__main__':
    main()
