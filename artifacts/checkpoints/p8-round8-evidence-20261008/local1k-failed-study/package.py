#!/usr/bin/env python3
"""Preserve all completed local cells and the terminal original cleanup failure."""
import datetime
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil

HERE = Path(__file__).resolve().parent
COHORT = Path('/workspace/scratch/2eaa00d0f93a/p8-local-G-1k-20261008')
EARLY = Path('/dev/shm/p8-round8-evidence-publication-root')

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def main():
    helper = EARLY / 'package.py'
    assert sha(helper.read_bytes()) == 'c4ade1626bc06c1ba72fc54873851e74381b33552983f9989153a9630a72dadd'
    spec = importlib.util.spec_from_file_location('verified_archive_helper', helper)
    package = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(package)
    package.HERE = HERE
    records = [json.loads((COHORT / f'controls/{index:02}/execution.json').read_text()) for index in range(8)]
    assert [row['shard_index'] for row in records] == list(range(8))
    assert all(row['status'] == 'passed_original_cell_and_validation' for row in records[:7])
    assert records[7]['status'] == 'failed_or_not_run_preserved' and records[7]['cli_exit_code'] == 1
    assert all(row['cli_stopped_confirmed'] for row in records)
    for index in range(8, 30):
        assert not (COHORT / f'controls/{index:02}').exists()
        assert not (COHORT / f'shard-1000-{index:02}').exists()
    sequence = COHORT / 'controls/remaining-sequence'
    assert not (sequence / 'finished.json').exists()
    progress = [json.loads(line) for line in (sequence / 'completed.jsonl').read_text().splitlines()]
    assert [row['shard_index'] for row in progress] == list(range(1, 8))
    assert [row['wrapper_exit_code'] for row in progress] == [0] * 6 + [1]
    review = Path('/dev/shm/p8-local-G-cell7-failure-independent-review/inspection.json')
    assert sha(review.read_bytes()) == '8cb8f8f566a259b71480d38eac437582351636d53827c1d1f525e9d648c41fe7'
    diagnosis = json.loads(review.read_text())
    report = json.loads((COHORT / 'shard-1000-07/native/report.json').read_text())
    assert report['worker_exit_code'] == 0 and report['exit_code'] == 2
    assert report['status'] == 'measurement_failed' and report['summary']['passed'] is True
    assert 'Directory not empty (os error 39)' in report['fixture_cleanup']['error']
    outcome = {
        'schema': 'p8-registered-local1k-terminal-outcome-v1',
        'observed_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'study': records[7]['study'], 'registration_publication': records[7]['registration_publication'],
        'registration_sha256': records[7]['registration_sha256'],
        'status': 'failed_and_incomplete_no_replacement',
        'registered_local_cells': 30, 'accepted_indices': list(range(7)),
        'failed_indices': [7], 'not_run_indices': list(range(8, 30)),
        'accepted_cell_scope': 'Original unchanged per-cell CLI and validate_build/validate_shard passed. Cell0 has an additional independent full validation; cells1..6 are not claimed to have a second non-author replay.',
        'failure_chain': diagnosis['failure_chain'],
        'cell7_original_validate_shard_actual_rejection': diagnosis['precise_original_gate']['original_receiver_actual_rejection'],
        'cell7_raw_protocol_pass_does_not_override_original_failure': True,
        'other120_registered_primary_cells': 'OriginalG Actions sources remain fixed. No status inferred here and no replacement of failed local cell7 is allowed.',
        'original_github_only_study_unchanged': True,
        'cleanup_cause_beyond_actual_os_error': 'unproven',
        'native_or_cargo_reexecution_after_failure': False,
        'original_source_or_gate_modifications': False,
        'no_successful_cohort_seal': True,
        'private_temporary_fixtures': 'Remain untouched locally; not included in these evidence archives or used as replacement final endpoints.',
        'original_todos': {'total': 192, 'done': 163, 'remaining': 29, 'newly_closed': 0},
    }
    (HERE / 'outcome.json').open('x').write(json.dumps(outcome, indent=2, sort_keys=True) + '\n')
    archives = []
    for index in range(1, 8):
        cell = f'shard-1000-{index:02}'
        control = f'controls/{index:02}'
        paths = package.collect(COHORT / cell, cell, True, False)
        paths += package.collect(COHORT / control, control, True, False)
        archives.append(package.archive(f'local1k-repetition-{index}.tar.gz', paths))
    paths = package.collect(sequence, 'controls/remaining-sequence', True, False)
    paths.append((HERE / 'outcome.json', 'outcome.json'))
    paths.append((review, 'cell7-independent-review/inspection.json'))
    paths += package.collect(Path('/dev/shm/p8-local-G-cell0-storage-review'), 'cell0-storage-review', True, False)
    paths += package.collect(Path('/dev/shm/p8-local-G-cohort-offline-validator'), 'suspended-unreviewed-cohort-validator', True, False)
    archives.append(package.archive('terminal-reviews.tar.gz', paths))
    manifest = {
        'schema': 'p8-local1k-failed-study-artifacts-v1', 'created_utc': outcome['observed_utc'],
        'parent': '75495895dc456d356c4f1fdedc9dc7b3065d830d', 'archives': archives,
        'previous_cell0_archive': {
            'commit': '75495895dc456d356c4f1fdedc9dc7b3065d830d',
            'path': 'artifacts/checkpoints/p8-round8-evidence-20261008/local1k-repetition-0.tar.gz',
            'bytes': 979061, 'sha256': 'ea64f2727568f67bbf7e72ca63244fa02d783f2270d56fd3b989aaafdb759ba2'},
        'original_build_reference': json.loads((EARLY / 'manifest.json').read_text())['local1k_original_build'],
        'package_helper': {'source': str(helper), 'sha256': sha(helper.read_bytes()),
                           'git_path': 'artifacts/checkpoints/p8-round8-evidence-20261008/package.py'},
        'outcome_sha256': sha((HERE / 'outcome.json').read_bytes()),
        'original_todos': outcome['original_todos'],
        'scope': 'Exact completed original cells1..7 and terminal failed-study controls, with cell0 retained by parent. No overall pass, replacement, new native run, or original-source change.'
    }
    (HERE / 'manifest.json').open('x').write(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
    shutil.copy2(EARLY / 'verify.py', HERE / 'verify.py')
    print(json.dumps({'archives': [{key: row[key] for key in ('path', 'bytes', 'sha256', 'git_blob')} for row in archives],
                      'manifest_sha256': sha((HERE / 'manifest.json').read_bytes()), 'outcome': outcome['status'], 'remaining_todos': 29}))

if __name__ == '__main__':
    main()
