#!/usr/bin/env python3
"""Check retained bytes/receipts; optional actual-input checks execute no product."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path)
    parser.add_argument('--input-root', type=Path)
    args = parser.parse_args()
    here = Path(__file__).resolve().parent

    def read(name):
        return json.loads((here / name).read_text())

    index = read('file-index.json')
    assert {path.name for path in here.iterdir() if path.is_file()} == set(index['files']) | {'file-index.json'}
    for name, expected in index['files'].items():
        path = PurePosixPath(name)
        assert not path.is_absolute() and '..' not in path.parts
        data = (here / name).read_bytes()
        assert digest(data) == expected['sha256'] and len(data) == expected['bytes'], name
    task = read('task-receipt.json')
    for name, count, code in [('stdlib-controls', 14, 0), ('nonobject-baseline', 15, 1), ('final-controls', 15, 0)]:
        receipt = read(name + '-receipt.json')
        log = (here / receipt['log']).read_bytes()
        assert digest(log) == receipt['log_sha256']
        assert receipt['test_functions'] == count and receipt['exit_code'] == code
        assert receipt['source_unchanged'] and receipt['rust_or_product_executed'] is False
        assert re.search(r'Ran ' + str(count) + r' tests in [\d.]+s', log.decode())
        assert (b'FAILED (failures=1)' in log) if code else log.rstrip().endswith(b'OK')
    fixed = read('final-controls-receipt.json')
    baseline = read('nonobject-baseline-receipt.json')
    assert fixed['source_commit'] == task['source_commit']
    assert fixed['source_paths_sha256'] == task['source_paths_sha256']
    fixture = 'tests/evidence_lock/test_benchmark_input_lock.py'
    script = 'scripts/lock_benchmark_inputs.py'
    assert baseline['source_paths_sha256'][fixture] == fixed['source_paths_sha256'][fixture]
    assert digest((here / 'baseline-locker.py.txt').read_bytes()) == baseline['source_paths_sha256'][script]
    assert all(row['test_execution_started'] is False for row in read('capture-preflight.json')['attempts'])

    initial = read('p8-001-invalid-lock-control.json')
    final = read('p8-001-invalid-lock-final-control.json')
    assert initial['actual_exit_code'] == 1 and initial['expected_exit_code'] == 2
    assert final['actual_exit_code'] == final['expected_exit_code'] == 2
    independent = read('p8-001-independent-final-review.json')
    assert independent['source_commit'] == task['source_commit'] and not independent['findings']
    assert digest((here / 'p8-001-invalid-lock-final-control.json').read_bytes()) == independent['independent_final_control']['sha256']
    assert digest((here / 'final-controls-receipt.json').read_bytes()) == independent['author_validation_separate']['receipt_sha256']
    historic = read('p8-001-independent-stdlib.json')
    assert historic['exit_code'] == 0
    historical_log = (here / 'p8-001-independent-stdlib.log').read_bytes()
    assert digest(historical_log) == historic['log_sha256']
    assert re.search(rb'Ran 14 tests in [\d.]+s', historical_log) and historical_log.rstrip().endswith(b'OK')

    concrete = read('019-input-lock-receipt.json')
    lock = read('019-input-lock.json')
    assert digest((here / '019-input-lock.json').read_bytes()) == concrete['lock_sha256'] == task['lock_sha256']
    assert lock['specification'] == read('input-spec.json')
    assert digest((here / 'input-spec.json').read_bytes()) == concrete['input_spec_sha256']
    assert concrete['source_commit'] == task['source_commit']
    assert len(lock['records']) == 26 and len(lock['specification']['bindings']) == 116
    assert lock['release_candidate'] is False and lock['execution_attestation'] is False
    assert concrete['new_benchmark_run_count'] == 0 and not concrete['rust_or_product_executed_by_this_lock_task']
    for action in ['prepare', 'verify']:
        row = concrete[action]
        assert row['exit_code'] == 0
        assert digest((here / row['log']).read_bytes()) == row['log_sha256']
        assert read(row['log']) == row['result']
    copy_control = read('real-copy-drift-receipt.json')
    assert [row['exit_code'] for row in copy_control['commands']] == [0, 2]
    for row in copy_control['commands']:
        assert digest((here / row['log']).read_bytes()) == row['log_sha256']
    assert copy_control['original_added_path_absent_before_and_after'] and copy_control['temporary_copy_removed']
    review = read(task['specific_input_lock_independent_review'])
    assert review['decision'] == 'accepted_scoped_preparation_lock' and not review['findings']
    assert review['lock']['sha256'] == task['lock_sha256']
    assert review['tool_source_commit'] == task['source_commit']
    assert review['bound_identity']['benchmark_source_commit'] == task['prior_benchmark_source']
    assert review['bound_identity']['prior_evidence_commit'] == task['prior_benchmark_evidence_commit']
    for field in ['independent_identity_check', 'independent_verification', 'prior_storage_review', 'author_prepare_verify_receipt']:
        row = review[field]
        assert digest((here / Path(row['path']).name).read_bytes()) == row['sha256']
    identity = read(Path(review['independent_identity_check']['path']).name)
    assert identity['source_inputs'] == 772 and identity['source_input_set_equals_commit']
    assert identity['source_input_hashes_equal_commit'] and identity['source_lock_map_equals_commit']
    assert identity['selected_original_files_equal_archive_index'] == len(identity['matched_originals']) == 120
    assert identity['report_files'] == 102 and identity['all_selected_report_files_equal_original_archive']
    verified = read(Path(review['independent_verification']['path']).name)
    assert verified['exit_code'] == 0 and verified['source_unchanged']
    assert verified['lock_sha256_before'] == verified['lock_sha256_after'] == task['lock_sha256']
    assert digest((here / Path(verified['log']).name).read_bytes()) == verified['log_sha256']
    assert verified['verification_result']['input_count'] == 26
    assert verified['verification_result']['checked_receipt_relationships'] == 116
    assert not verified['product_execution'] and not verified['rust_execution']
    assert not task['full_P8_001_accepted'] and task['dependency_preserved'] == ['P7-020']
    if args.source_root:
        for name, expected in task['source_paths_sha256'].items():
            assert digest((args.source_root / name).read_bytes()) == expected, name
    if args.input_root:
        root = args.source_root or here.parents[2]
        locker_path = root / script
        assert digest(locker_path.read_bytes()) == task['source_paths_sha256'][script]
        subprocess.run([
            sys.executable, '-B', str(locker_path), 'verify', '--root', str(args.input_root),
            '--lock', str(here / '019-input-lock.json'),
            '--expected-lock-sha256', task['lock_sha256'],
        ], check=True)
    print(json.dumps({
        'stored_evidence_valid': True, 'files': len(index['files']),
        'tool_source': task['source_commit'], 'final_author_functions': 15,
        'original_frozen_baseline': {'passed': 14, 'failed': 1},
        'lock_sha256': task['lock_sha256'], 'actual_input_verification_requested': bool(args.input_root),
        'new_product_or_benchmark_execution': False, 'release_candidate': False,
        'P8_001_status': 'todo_groundwork_only',
    }, sort_keys=True))


if __name__ == '__main__':
    main()
