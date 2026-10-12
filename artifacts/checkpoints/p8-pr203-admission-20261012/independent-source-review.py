#!/usr/bin/env python3
"""Independent exact-object source review and harmless custody control.

This runs outside the repository's protected source roots, does not update Git
refs/index/worktree, and does not execute a native study or remote upload.
"""
import collections
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import yaml

ROOT = Path('/workspace/scratch/2509e1756d27/codecortex')
OUT = Path('/workspace/scratch/2509e1756d27/pr-audit-20261012')
P = '0692ed27c4f86a254484e76155fe07a55ab2e09c'
PTREE = '13ab60305b8513dbb89f037d5e9bbc9d89443a8e'
MAIN = 'c2b726538cacfdf6ae9f01977a7b93da48d2e2b2'
PR = '797e9e60516e9bb8cc3c35f14a6ec7dc0092aa4f'
OLD = '3481da0baf46bf510580951a4f5fbc9cb2255ae5'
BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
OLD_REVIEW = '533e900cf83900f726e92835956137356081861f'
OLD_REVIEW_PATH = 'artifacts/checkpoints/p8-task-profile-format-fix-bfcc-20261010/independent-source-review.json'
WORKFLOW = '.github/workflows/p8-task-profile.yml'
VROOTS = ('scripts', '.github/workflows', 'tests/source_integrity')
EXCLUSIONS = {'scripts/verify_reviewed_source_v15.py', 'scripts/reviewed-source-registry-v15.json', '.github/workflows/ci.yml'}
commands = []


def git(*args):
    commands.append(['git', *args])
    return subprocess.check_output(['git', *args], cwd=ROOT)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def blob(ref, path):
    return git('show', f'{ref}:{path}')


def inventory(ref, roots):
    out = {}
    for row in git('ls-tree', '-r', '-z', ref, '--', *roots).split(b'\0'):
        if row:
            header, path = row.decode().split('\t', 1)
            mode, kind, oid = header.split()
            assert kind == 'blob' and mode in {'100644', '100755'}, (path, mode, kind)
            out[path] = dict(mode=mode, oid=oid)
    return out


def validation(ref):
    return {p: row for p, row in inventory(ref, VROOTS).items()
            if p not in EXCLUSIONS
            and not ('__pycache__' in Path(p).parts and p.endswith('.pyc'))}


def diff(a, b, roots=()):
    return set(git('diff', '--name-only', a, b, '--', *roots).decode().splitlines())


def run():
    header = git('cat-file', '-p', P).decode().split('\n\n', 1)[0].splitlines()
    assert next(x[5:] for x in header if x.startswith('tree ')) == PTREE
    assert [x[7:] for x in header if x.startswith('parent ')] == [PR, MAIN]
    assert diff(MAIN, P) == {WORKFLOW, 'docs/roadmap/code-index-v2/PLAN-CHECK.json'}
    product = inventory(P, ('crates', 'Cargo.toml', 'Cargo.lock'))
    prior_product = inventory(OLD, ('crates', 'Cargo.toml', 'Cargo.lock'))
    assert product == prior_product and len(product) == 1096
    assert diff(BASE, P, ('crates', 'Cargo.toml', 'Cargo.lock')) == diff(BASE, OLD, ('crates', 'Cargo.toml', 'Cargo.lock'))
    guard_raw = blob(P, 'scripts/verify_reviewed_source_v15.py')
    assert guard_raw == blob(MAIN, 'scripts/verify_reviewed_source_v15.py')
    registry_raw = blob(P, 'scripts/reviewed-source-registry-v15.json')
    assert registry_raw == blob(MAIN, 'scripts/reviewed-source-registry-v15.json')
    assert sha(registry_raw) == 'cce1f24ebccdf007117004b7d7e9cec8377fdb8aa106e33d379e0c5b9d95354f'
    registry = json.loads(registry_raw)
    prior_review_raw = blob(OLD_REVIEW, OLD_REVIEW_PATH)
    assert sha(prior_review_raw) == registry['review_sha256']
    prior_review = json.loads(prior_review_raw)
    assert prior_review['source'] == registry['product_source'] == OLD
    assert prior_review['base'] == registry['base_source'] == BASE
    assert prior_review['verdict'] == 'accepted_scoped'
    assert prior_review['complete_inputs'] == registry['complete_inputs']
    assert prior_review['paths'] == registry['delta']
    assert prior_review['validation_inputs'] == registry['validation_inputs']
    assert set(registry['complete_inputs']) == set(product)
    actual_delta = diff(BASE, P, ('crates', 'Cargo.toml', 'Cargo.lock'))
    assert set(registry['delta']) == actual_delta and len(actual_delta) == 80
    new_validation = validation(P)
    old_validation = validation(OLD)
    assert set(new_validation) == set(old_validation) == set(registry['validation_inputs'])
    assert len(new_validation) == 157
    changed_validation = {p for p in new_validation if new_validation[p] != old_validation[p]}
    assert changed_validation == {WORKFLOW}
    for p in EXCLUSIONS:
        assert blob(P, p) == blob(MAIN, p), p
    frozen = ('scripts/verify_reviewed_source_v14.py', 'scripts/reviewed-source-registry-v14.json',
              'tests/source_integrity/test_reviewed_source_v14.py', 'scripts/v14_historical_context.py',
              'tests/source_integrity/v14_historical_test_adapter.py', 'tests/source_integrity/test_v14_historical_context.py')
    for p in frozen:
        assert blob(P, p) == blob(BASE, p), p
    workflow_raw = blob(P, WORKFLOW)
    assert workflow_raw == blob(PR, WORKFLOW)
    assert sha(workflow_raw) == '2dfcab748378de13e2bc5ffef3a35bb08ff4448921496c1119d8b4c4e552c344'
    assert sha(blob(OLD, WORKFLOW)) == registry['validation_inputs'][WORKFLOW]
    workflow = yaml.safe_load(workflow_raw)
    measure = workflow['jobs']['measure']
    steps = measure['steps']
    by_id = {s['id']: s for s in steps if 'id' in s}
    assert 'upload_19' not in by_id
    assert by_id['freeze_19']['env'] == {
        'PREVIOUS_ARTIFACT_ID': '${{ steps.upload_18.outputs.artifact-id }}',
        'PREVIOUS_ARTIFACT_DIGEST': '${{ steps.upload_18.outputs.artifact-digest }}'}
    assert 'env' not in by_id['finish']
    for env in (workflow.get('env', {}), measure.get('env', {})):
        assert 'PREVIOUS_ARTIFACT_ID' not in env and 'PREVIOUS_ARTIFACT_DIGEST' not in env
    assert '--index 20' in by_id['finish']['run']
    assert ' finish ' in by_id['finish']['run']
    cell_count = len(measure['strategy']['matrix']['include'])
    assert cell_count == 45
    progress_uploads = [s for s in steps if s.get('uses') == 'actions/upload-artifact@v4'
                        and s.get('with', {}).get('name', '').startswith('p8-task-progress-')]
    assert len(progress_uploads) == 20
    assert 1 + cell_count + len(progress_uploads) * cell_count + cell_count + 1 == 992
    task_raw = blob(P, 'docs/roadmap/code-index-v2/tasks.json')
    receipt_raw = blob(P, 'docs/roadmap/code-index-v2/PLAN-CHECK.json')
    receipt = json.loads(receipt_raw)
    assert blob(MAIN, 'docs/roadmap/code-index-v2/tasks.json') == task_raw
    tasks = json.loads(task_raw)['tasks']
    counts = dict(collections.Counter(t['status'] for t in tasks))
    assert receipt['task_count'] == len(tasks) == 192
    assert receipt['states'] == counts == {'done': 176, 'in_progress': 3, 'todo': 12, 'blocked': 1}
    assert receipt['task_sha256'] == sha(task_raw)

    # Load only the two byte-capture modules, after comparing exact source bytes.
    for name in ('p8_scale_diagnostic_capture.py', 'p8_task_profile_capture.py'):
        assert (ROOT / 'scripts' / name).read_bytes() == blob(P, 'scripts/' + name)
    spec = importlib.util.spec_from_file_location('review_task_capture', ROOT / 'scripts/p8_task_profile_capture.py')
    capture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(capture)
    with tempfile.TemporaryDirectory(prefix='pr203-independent-custody-') as temp:
        root = Path(temp)
        capture.core.write_new(root / 'configuration.json', {'task_protocol': capture.SCHEMA, 'synthetic_review_fixture': True})
        raw = root / 'shard/native-shard/native/raw.jsonl'
        raw.parent.mkdir(parents=True)
        at18 = b'{"checkpoint":18}\n'
        tail19 = b'{"partial":19'
        tail20 = b',"finish":20}\n'
        raw.write_bytes(at18)
        _, m18 = capture.core.snapshot(root, 18)
        raw.write_bytes(at18 + tail19)
        _, m19 = capture.core.snapshot(root, 19, previous_id='18001', previous_digest='a' * 64)
        raw.write_bytes(at18 + tail19 + tail20)
        final_bundle, m20 = capture.core.snapshot(root, 20)
        source = raw.relative_to(root).as_posix()
        assert m19['acknowledged_prefix_bytes_before_this_upload'][source] == len(at18)
        assert m20['acknowledged_prefix_bytes_before_this_upload'][source] == len(at18)
        rows = {c['name']: c for c in m20['all_captured_chunks']}
        ordered = sorted(m20['included_chunk_names'], key=lambda n: rows[n]['start'])
        reconstructed_tail = b''.join((final_bundle / n).read_bytes() for n in ordered)
        assert reconstructed_tail == tail19 + tail20
        assert [u['checkpoint'] for u in m20['acknowledged_uploads_before_this_upload']] == [18]
        assert not m20['native_EOF_claimed']
        state = capture.core.read_json(root / 'capture-state.json')
        assert [u['checkpoint'] for u in state['uploads']] == [18]
        assert all(rows[n]['end'] <= len(at18) for n in state['acknowledged_chunks'])
        control = {'synthetic_fixture_only': True, 'native_measurement_executed': False,
                   'remote_upload_executed': False, 'checkpoint18_bytes': len(at18),
                   'final_tail_bytes': len(reconstructed_tail), 'final_tail_sha256': sha(reconstructed_tail),
                   'last_acknowledged_checkpoint': 18, 'final_does_not_self_acknowledge': True,
                   'retained_tail_matches_19_plus_20': True, 'native_EOF_claimed': False,
                   'lost_runner_tail_policy': m20['lost_runner_tail']}

    complete = registry['complete_inputs']
    validation_digests = dict(registry['validation_inputs'])
    validation_digests[WORKFLOW] = sha(workflow_raw)
    review = {
        'schema_version': 10, 'source': P, 'source_tree': PTREE, 'source_parents': [PR, MAIN],
        'base': BASE, 'scope': 'independently_reviewed_source_and_validation_inputs',
        'verdict': 'accepted_scoped', 'independent_reviewers': ['/root/pr_audit'],
        'unresolved_blockers': [],
        'scoped_acceptance_boundary': 'Exact source/validation admission only. Existing performance CI failure remains unresolved and merge stays held until new-head checks actually pass; this review is not runtime, quality, release, or original-TODO approval.',
        'paths': registry['delta'], 'complete_inputs': complete, 'validation_inputs': validation_digests,
        'prior_approval': {'source': OLD, 'review_commit': OLD_REVIEW, 'review_path': OLD_REVIEW_PATH,
                           'review_sha256': sha(prior_review_raw), 'registry_sha256': sha(registry_raw),
                           'continuity': 'Every one of the 1096 product Git mode/blob identities equals the accepted product. All 80 original-BASE product paths and their before/after identities are inherited unchanged. Complete validation mode/blob inventory equals the accepted inventory except the one reviewed workflow leaf.'},
        'changed_validation_leaves': {WORKFLOW: {'before_sha256': registry['validation_inputs'][WORKFLOW], 'sha256': sha(workflow_raw),
                                                'mode': new_validation[WORKFLOW]['mode'], 'blob_oid': new_validation[WORKFLOW]['oid']}},
        'guard_integrity': {'base_unchanged': True, 'version_unchanged': True, 'frozen_inputs_equal_original_base': list(frozen),
                            'selected_guard_algorithm_unchanged': True, 'ci_selector_unchanged': True,
                            'P_uses_old_pins_and_requires_subsequent_G_identity_binding': True},
        'workflow_semantics': {'freeze19_acknowledges_real_upload18': True, 'no_upload19': True,
                               'finish_receives_no_previous_artifact_identity': True, 'unacknowledged_chunks_repackaged_in_final': True,
                               'complete_scheduled_artifact_count': 992, 'matrix_cells': 45,
                               'limitation': 'This limits the fixed complete-run artifact count; it is not pagination support or a bound covering future extra artifacts/retries. Loss before final upload leaves the last remotely proven prefix at18.'},
        'plan_receipt': {'path': 'docs/roadmap/code-index-v2/PLAN-CHECK.json', 'sha256': sha(receipt_raw),
                         'task_sha256': sha(task_raw), 'counts': counts, 'main_task_bytes_unchanged': True,
                         'reviewer_validator_rerun': False, 'note': 'Receipt values independently matched to immutable P task bytes. Root supplied actual unchanged-validator stdout; this reviewer does not relabel that as an independent validator run.'},
        'actual_harmless_custody_control': control,
        'external_runtime_blockers': [{'pr': 203, 'head': PR, 'job_url': 'https://github.com/jyqj/codecortex/actions/runs/38035469451/job/114164870834',
                                        'failure': 'benchmark_fixture context warm p95 1.72s exceeds500ms; complete table also index4.23s/search659.53ms',
                                        'gate_threshold_unchanged': True, 'new_head_CI': 'not_run_at_review'}],
        'original_TODO_completion_credit': 0,
        'commands': commands,
        'inventory_proof': {'product_mode_oid_sha256': sha(json.dumps(product, sort_keys=True, separators=(',', ':')).encode()),
                            'validation_mode_oid_sha256': sha(json.dumps(new_validation, sort_keys=True, separators=(',', ':')).encode()),
                            'product_count': len(product), 'base_delta_count': len(actual_delta), 'validation_count': len(new_validation),
                            'maps_sha256_inherited_only_after_exact_Git_mode_blob_identity_comparison': True},
    }
    (OUT / 'round2-source-review.json').write_text(json.dumps(review, indent=2, sort_keys=True) + '\n')
    (OUT / 'round2-mode-oid-inventories.json').write_text(json.dumps({'source': P, 'product': product, 'validation': new_validation}, indent=2, sort_keys=True) + '\n')
    print(json.dumps({'status': 'accepted_scoped', 'source': P, 'product': len(product), 'base_delta': len(actual_delta),
                      'validation': len(new_validation), 'changed_validation': sorted(changed_validation),
                      'synthetic_custody_control': control, 'TODO_credit': 0,
                      'review_sha256': sha((OUT / 'round2-source-review.json').read_bytes())}, sort_keys=True))


if __name__ == '__main__':
    run()
