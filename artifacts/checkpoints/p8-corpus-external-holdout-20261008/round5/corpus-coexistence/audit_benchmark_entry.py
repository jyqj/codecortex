#!/usr/bin/env python3
"""Check the original P8 deliverable entry without opening a holdout body."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path('/workspace/scratch/031390cf22eb/codecortex-closeout')
WORK = Path('/workspace/scratch/031390cf22eb')
OUT = WORK / 'round5-corpus-coexistence'
SOURCE = '56d15ed246eedd9add0c0d2843001b5f4e971ee8'
PRIOR = '615662bd0e651dc40a9d1d0d6757bc28646b900d'
F = 'fb772551cff6b4620a6fcdb94c57b78350cebb33'
D = '87ddafb409e4854baaa20ac339e97260d2fcb531'
ENTRY = 'artifacts/benchmarks/p8-corpus-external-holdout-20261008/'
CHECKPOINT = 'artifacts/checkpoints/p8-corpus-external-holdout-20261008/'
sha = lambda data: hashlib.sha256(data).hexdigest()
read_inputs = {}


def tracked(path, commit=SOURCE):
    raw = subprocess.check_output(['git', '-C', str(ROOT), 'show', commit + ':' + path])
    read_inputs[commit + ':' + path] = {'sha256': sha(raw), 'bytes': len(raw)}
    return raw


def external(path):
    raw = Path(path).read_bytes()
    read_inputs[str(path)] = {'sha256': sha(raw), 'bytes': len(raw)}
    return raw


def jtracked(path):
    return json.loads(tracked(path))


index = jtracked(ENTRY + 'index.json')
readme = tracked(ENTRY + 'README.md').decode()
assert 'gate_failed' in readme and 'invalid_measurement' in readme
assert index['checkpoint'] + '/' == CHECKPOINT
assert len(index['runs']) == 6
assert len({r['run_id'] for r in index['runs']}) == 6
for path in (ENTRY + 'README.md', ENTRY + 'index.json'):
    assert (ROOT / path).read_bytes() == tracked(path)
all_paths = subprocess.check_output(['git', '-C', str(ROOT), 'ls-tree', '-r', '--name-only', SOURCE, '--', CHECKPOINT]).decode().splitlines()
for run in index['runs']:
    assert any(p == run['evidence'] or p.startswith(run['evidence'] + '/') for p in all_paths)

tasks_path = 'docs/roadmap/code-index-v2/tasks.json'
def task_map(raw):
    data = json.loads(raw)
    return {t['id']: t for t in (data['tasks'] if isinstance(data, dict) else data)}
prior_tasks, current_tasks = task_map(tracked(tasks_path, PRIOR)), task_map(tracked(tasks_path))
task_text = {}
for ident in ('P8-002', 'P8-003', 'P8-004'):
    fields = ('steps', 'acceptance', 'deliverables', 'depends_on')
    assert all(prior_tasks[ident].get(k) == current_tasks[ident].get(k) for k in fields)
    task_text[ident] = {'original_steps_acceptance_deliverables_dependencies_equal': True,
                       'status_in_prior_P': prior_tasks[ident]['status'],
                       'status_in_PR153_merge_proposal': current_tasks[ident]['status'],
                       'this_audit_does_not_edit_or_adopt_the_proposed_status': True}

registry_run = next(r for r in index['runs'] if r['task_ids'] == ['P8-002'])
registry_raw = tracked(registry_run['evidence'])
original_registry = external(WORK / 'round4-native-registry/validation-v2/registry-validation.json')
assert registry_raw == original_registry
registry = json.loads(registry_raw)
assert registry_run['actual_validator_source'] == F == registry['validator']['source_commit']
assert registry_run['native_questions'] == registry['coverage']['native_rows'] == 600
assert registry_run['compat_projections'] == registry['coverage']['compat_rows'] == 554
assert registry_run['native_and_compat_suites_validated'] == len(registry['native_validator_calls']) == 20
assert registry_run['exit_codes'] == [r['exit_code'] for r in registry['native_validator_calls']] == [0] * 20

canonical_audit_path = WORK / 'round4-validation/canonical-diagnostics-independent/canonical-diagnostics-audit.json'
canonical_audit = json.loads(external(canonical_audit_path))
canonical_by_id = {Path(r['run']).name: r for r in canonical_audit['runs']}
canonical_records = []
for run in [r for r in index['runs'] if r['task_ids'] == ['P8-003']]:
    original = canonical_by_id[run['run_id']]
    assert run['product_source'] == F and run['evaluator_source'] == D
    assert run['original_questions'] == original['query_count'] == 100
    assert run['measured_rows'] == original['measured_rows'] == 300
    assert run['exit_code'] == original['original_exit_code'] == 2
    assert run['gate'] == original['gate']['status'] == 'invalid_measurement'
    gate_raw = tracked(run['evidence'] + '/gate.json')
    assert gate_raw == external(Path(original['run']) / 'gate.json')
    manifest_path = Path(original['run']) / 'manifest.json'
    manifest = json.loads(external(manifest_path))
    assert run['warmups'] == original['query_count'] * manifest['suite']['warmup'] == 100
    canonical_records.append({'run_id': run['run_id'], 'actual_run_path': original['run'],
        'metadata_gate_sha256': sha(gate_raw), 'original_manifest_sha256': read_inputs[str(manifest_path)]['sha256'],
        'query_ids': 100, 'measured_rows': 300, 'warmups': 100, 'gate': run['gate'], 'exit_code': 2,
        'source_and_input_lock_audited_by': str(canonical_audit_path)})

# These are explicitly allowed metadata files. No suite/query/raw body or tar
# member is opened, including after the packet has been used and sealed.
fresh_files = {
    'execution-receipt.json': WORK / 'round4-holdout/once-20261008-F/execution-receipt.json',
    'aggregate-results.json': WORK / 'round4-holdout/postseal-analysis/aggregate-results.json',
    'evidence-package-receipt.json': WORK / 'round4-holdout/postseal-analysis/evidence-package-receipt.json',
}
fresh = {}
for name, original_path in fresh_files.items():
    copy = tracked(CHECKPOINT + 'holdout/' + name)
    assert copy == external(original_path)
    fresh[name] = json.loads(copy)
fresh_run = next(r for r in index['runs'] if r['task_ids'] == ['P8-004'])
assert fresh_run['product_source'] == F == fresh_run['evaluator_source']
assert fresh_run['measured_rows'] == 64 and fresh_run['families'] == 32
assert fresh_run['warmups'] == fresh_run['retries'] == 0
assert fresh_run['exit_codes'] == [1, 1] and fresh_run['gate'] == 'gate_failed'
assert fresh_run['no_answer_correct'] == 0 and fresh_run['no_answer_total'] == 8
assert fresh['aggregate-results.json']['candidate_commit'] == F
assert fresh['execution-receipt.json']['suites_attempted'] == 2
assert fresh['execution-receipt.json']['retries'] == 0

external_package = jtracked(CHECKPOINT + 'canonical/evidence-package-receipt.json')
delivery = jtracked(CHECKPOINT + 'evidence-delivery.json')
assert index['archives'] == delivery['archives']
archive_records = []
for archive in index['archives']:
    used_holdout = 'Holdout' in archive['filename']
    package = fresh['evidence-package-receipt.json'] if used_holdout else external_package
    for field, pfield in (('sha256', 'archive_sha256'), ('bytes', 'archive_bytes'),
                          ('members_including_manifest', 'members_including_manifest'), ('manifest_sha256', 'manifest_sha256')):
        assert archive[field] == package[pfield]
    original_path = Path(package['archive'])
    delivery_path = WORK / 'round4-delivery' / archive['filename']
    assert original_path.is_file() and delivery_path.is_file()
    assert original_path.stat().st_size == delivery_path.stat().st_size == archive['bytes']
    if not used_holdout:
        assert sha(external(original_path)) == archive['sha256']
    archive_records.append({**archive, 'package_metadata_matches_entry': True,
        'original_archive_path': str(original_path), 'delivery_archive_path': str(delivery_path),
        'both_archive_file_sizes_independently_observed': True,
        'compressed_archive_sha256_independently_recomputed': not used_holdout,
        'archive_members_or_bodies_opened_in_this_audit': False,
        'holdout_digest_basis': 'previous explicit metadata-only packaging receipt; contents deliberately not read' if used_holdout else None,
        'remote_user_delivery_checked_by_this_auditor': False})

prior_acceptance = WORK / 'round4-validation/task-acceptance-independent.json'
followup = WORK / 'round4-validation/task-acceptance-independent-default-followup.json'
for path in (prior_acceptance, followup):
    external(path)
report = {
    'schema_version': 1, 'status': 'accepted_original_benchmark_evidence_entry_scope',
    'auditor': '/root/p8_corpus_closeout', 'reviewed_source_commit': SOURCE,
    'observed_at_utc': datetime.now(timezone.utc).isoformat(), 'script_sha256': sha(Path(__file__).read_bytes()),
    'entry_readme': ENTRY + 'README.md', 'entry_index': ENTRY + 'index.json',
    'scope': 'Concrete artifacts/benchmarks/<run-id> entry, original task text and metadata-to-complete-evidence traceability',
    'original_task_text': task_text,
    'P8_002': {'original_registry_receipt_copy_exact': True, 'native_questions': 600, 'compat_projections': 554,
        'actual_original_F_validate_calls': 20, 'all_exit_zero': True, 'complete_600_retrieval_quality_run': 'not_run'},
    'P8_003': {'runs': canonical_records, 'total_measured': 1200, 'total_warmups': 400,
        'original_invalid_measurement_and_exit2_preserved': True, 'leaderboard_or_accepted_quality_claim': False},
    'P8_004': {'metadata_files_copy_exact': {name: sha(tracked(CHECKPOINT + 'holdout/' + name)) for name in fresh_files},
        'once_only_measured_rows': 64, 'family_labels': 32, 'source_linked_components': 7,
        'original_exit_codes': [1, 1], 'original_gate_failed': True, 'negative_cases_correct': 0,
        'negative_cases_total': 8, 'fresh_packet_is_used_not_unseen': True,
        'no_fresh_body_access_for_this_audit': True},
    'archives': archive_records, 'all_six_run_evidence_targets_exist_at_fixed_source': True,
    'acceptance_conclusion': 'The original benchmark deliverable now has substantive run entries and exact metadata links to complete sealed evidence. This supports the original task execution scope and does not substitute summaries for the retained raw/inputs.',
    'regression_condition': 'Original task acceptance and the additive default-failure followup remain binding. Final merged-source review/CI and explicit disposition of the preserved original Rust failures are separate; source-integrity success alone does not discharge failed basic regressions.',
    'earlier_acceptance_receipt': {'path': str(prior_acceptance), 'sha256': read_inputs[str(prior_acceptance)]['sha256']},
    'earlier_default_failure_followup': {'path': str(followup), 'sha256': read_inputs[str(followup)]['sha256']},
    'full_V19_G8_quality_or_release_certified': False, 'new_measurement_or_retrieval_executions': 0,
    'repository_edits': 0, 'fresh_or_protected_question_body_reads': 0, 'inputs': read_inputs,
    'limits': ['Archive member verification is the preserved packaging auditor attestation; this audit independently checks public external compressed bytes and both local file sizes.',
              'The holdout archive is deliberately not opened or hashed by this auditor; its full digest and member count are verified against the explicitly allowed packaging metadata.',
              'User-facing remote delivery availability is recorded by the delivery owner, not independently queried here.',
              'The original 600, 1200 and 64 scopes remain separately fixed; neither new PR153 rows nor later merged product sources are retroactively substituted.']
}
target = OUT / 'benchmark-entry-acceptance-independent.json'
with target.open('x') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2, sort_keys=True)
    stream.write('\n')
print(json.dumps({'path': str(target), 'sha256': sha(target.read_bytes()), 'status': report['status'], 'fresh_body_reads': 0}))
