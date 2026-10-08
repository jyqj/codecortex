#!/usr/bin/env python3
"""Verify the exact inherited PR153 input inventory without extending its review."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess

WORK = Path('/workspace/scratch/031390cf22eb')
REPO = WORK / 'codecortex-closeout'
OUT = WORK / 'round5-corpus-coexistence'
P = '3e3e7119ade9c16ccdabf8526ee73bd1077c645d'
R = '438b80cc90ff3b4842a0efbe4057253e73c95c72'
REVIEW_PATH = 'artifacts/checkpoints/p8-joint-native-external-20261008/independent-source-review.json'
REFS = [P, 'a8ea19cd0d628dc6a62f9b06bde0c5d58e041229',
        '56d15ed246eedd9add0c0d2843001b5f4e971ee8', '2d575c60040d852a7c02e2bf6fa68d21caecd62d']
BASE = '615662bd0e651dc40a9d1d0d6757bc28646b900d'
sha = lambda raw: hashlib.sha256(raw).hexdigest()


def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args])


def record(ref, path):
    assert 'holdout' not in path.lower() and 'protected' not in path.lower()
    entry = git('ls-tree', '-z', ref, '--', ':(literal)' + path).split(b'\0')
    assert len(entry) == 2 and not entry[-1]
    head, actual = entry[0].split(b'\t', 1)
    mode, kind, oid = head.decode().split()
    assert kind == 'blob' and mode in ('100644', '100755') and actual.decode() == path
    raw = git('show', ref + ':' + path)
    return {'sha256': sha(raw), 'bytes': len(raw), 'mode': mode, 'git_blob': oid}


delta_path = WORK / 'round4-pr153-review/pr153-complete-source-delta.json'
delta = json.loads(delta_path.read_bytes())
benchmarks = [v for v in delta['changes'] if v['kind'] == 'benchmark_inputs']
assert len(benchmarks) == 238
review_raw = git('show', R + ':' + REVIEW_PATH)
review = json.loads(review_raw)
assert review['source'] == P and review['verdict'] == 'accepted_scoped'
verified = {}
for entry in benchmarks:
    path = entry['path']
    versions = [record(ref, path) for ref in REFS]
    assert all(v == versions[0] == entry['after'] for v in versions)
    assert review['complete_inputs'][path] == versions[0]['sha256']
    verified[path] = versions[0]

helper_paths = ('scripts/p8_compat.py', 'scripts/p8_external_candidate.py', 'scripts/tests/test_p8_compat.py')
helpers = {}
for path in helper_paths:
    versions = [record(ref, path) for ref in REFS]
    assert all(v == versions[0] for v in versions)
    original_review_digest = review['validation_inputs'][path]
    if isinstance(original_review_digest, dict):
        original_review_digest = original_review_digest['sha256']
    assert original_review_digest == versions[0]['sha256']
    base_exists = bool(git('ls-tree', '-z', BASE, '--', ':(literal)' + path))
    helpers[path] = {'final_original_record': versions[0],
        'base_record': record(BASE, path) if base_exists else None,
        'byte_equal_to_original_PR153_review': True,
        'current_non_author_code_review': 'complete file and all P615 delta read',
        'disposition': 'confirmed path isolation defect; root minimal fix and independent re-review pending'
            if path.endswith('/p8_external_candidate.py') else 'accepted_declared_compat_profile_and_preserved_lock_test_scope'}

registry_paths = ('crates/cc-eval/benchmarks/public-dev-20261008/index.json',
                  'crates/cc-eval/benchmarks/manifests/public-dev-20261008.dataset-index.json')
registries = {}
for path in registry_paths:
    values = [record(ref, path) for ref in REFS[2:]]
    assert values[0] == values[1]
    registries[path] = values[0]
ours_helpers = {}
for path in ('scripts/p8_native_registry.py', 'scripts/p8_corpus_audit.py', 'scripts/p8_release_evidence.py'):
    values = [record(ref, path) for ref in (BASE, *REFS[2:])]
    assert all(v == values[0] for v in values)
    ours_helpers[path] = values[0]

refs = {}
for name in ('corpus-coexistence-independent.json', 'registry-input-validation-independent.json',
             'package-boundary-original/reproduction.json'):
    path = OUT / name
    refs[name] = {'path': str(path), 'sha256': sha(path.read_bytes())}
report = {'schema_version': 1, 'status': 'inherited_benchmark_bytes_verified_packager_fix_pending',
    'auditor': '/root/p8_corpus_closeout', 'at_utc': datetime.now(timezone.utc).isoformat(),
    'script_sha256': sha(Path(__file__).read_bytes()), 'fixed_comparison_refs': REFS,
    'original_review': {'commit': R, 'path': REVIEW_PATH, 'sha256': sha(review_raw),
        'reviewed_source': P, 'scope': review['scope'], 'verdict_as_recorded': review['verdict'],
        'independent_reviewers_as_recorded': review['independent_reviewers'],
        'authorship_and_independence_as_recorded': review['authorship_and_independence'],
        'scope_transfer': 'Exact unchanged inputs only; preserve the recorded original non-author source/gold and component decisions. No claim that the current thread re-read every historical author packet or all 26 question meanings.'},
    'complete_delta_inventory': {'path': str(delta_path), 'sha256': sha(delta_path.read_bytes())},
    'benchmark_input_count': len(verified), 'all_238_git_blob_mode_size_sha256_equal_across_four_refs': True,
    'all_238_bound_by_original_R_complete_inputs': True, 'benchmark_inputs': verified,
    'three_requested_PR153_helpers': helpers, 'two_current_registries': registries,
    'unchanged_DEV600_helpers': ours_helpers,
    'independent_data_scope': {'original_shared_native_rows': 301, 'our_new_native_rows': 299,
        'PR153_new_native_rows': 26, 'PR153_new_serde_rows': 14, 'PR153_new_vite_rows': 12,
        'unique_native_union': 626, 'compat_union': 580, 'not_927': True,
        'source_query_projection_and_declared_family_coverage_receipt': refs['corpus-coexistence-independent.json'],
        'current_manual_26_question_semantic_review_claim': False,
        'no_new_family_independence_or_holdout_overlap_claim': True},
    'actual_validation_receipt': refs['registry-input-validation-independent.json'],
    'new_known_defect_overrides_blanket_inheritance': refs['package-boundary-original/reproduction.json'],
    'remaining_source_review_owner': 'Root authors the minimal packager correction; current-thread pr_audit independently reviews the new bytes. Other profile/provenance helpers are build_validation scope.',
    'protected_or_fresh_holdout_body_reads': 0, 'repository_edits': 0,
    'limits': ['Reading/hashing the retained DEV intake copies does not increase the 626 count or replace original semantic review.',
              'This receipt binds original 2d/56d helper bytes. It must not be relabeled as approval of a later path-fix source commit.',
              'The exact final remote P source manifest must independently bind these unchanged bytes and any reviewed new validation code.']}
target = OUT / 'pr153-benchmark-byte-inheritance.json'
with target.open('x') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2, sort_keys=True)
    stream.write('\n')
print(json.dumps({'path': str(target), 'sha256': sha(target.read_bytes()), 'benchmark_inputs': len(verified),
                  'original_review_sha256': sha(review_raw), 'packager_fix_pending': True}))
