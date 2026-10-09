#!/usr/bin/env python3
"""Preserve v1 and bind its new wrapper to the existing ablation control."""
from pathlib import Path
import datetime
import difflib
import hashlib
import json
import os
import subprocess

D = Path(__file__).resolve().parent
REPO = D.parents[1] / 'codecortex'
ENV = dict(os.environ, GIT_NO_LAZY_FETCH='1', GIT_OPTIONAL_LOCKS='0')
MAIN = 'b9412406e11422d7cf914458a8bfbbd58cf94eaa'
CONTROL = 'crates/cc-eval/src/benchmark/ablation/mechanism_controls.json'
LANES = 'crates/cc-search/src/lanes.rs'
POLICY = 'crates/cc-search/src/query_policy.rs'


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def blob(raw):
    return hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()


def git(*args):
    return subprocess.run(['git', *args], cwd=REPO, env=ENV, capture_output=True, check=True).stdout


v1_raw = (D / 'changed-files.json').read_bytes()
v1 = json.loads(v1_raw)
assert sha(v1_raw) == '9237b5a9a940128ccb1d162c6b1781ec83ada16a406dbb08af70fa9e2c2f6d60'
original, candidate = {}, {}
for row in v1['changed_files']:
    path = row['path']
    original[path] = (D / row['original_relative_path']).read_bytes()
    candidate[path] = (D / row['candidate_relative_path']).read_bytes()
    assert sha(original[path]) == row['before_sha256']
    assert sha(candidate[path]) == row['after_sha256']
    assert git('show', MAIN + ':' + path) == original[path]
original[CONTROL] = git('show', MAIN + ':' + CONTROL)
control = json.loads(original[CONTROL])
assert len(control) == 2 and control[0]['id'] == 'local_retrieval' and control[1]['id'] == 'semantic_dense'
assert original[LANES].decode().count(control[0]['on_text']) == 1
assert json.dumps(control, indent=2) + '\n' == original[CONTROL].decode()
old_control = json.loads(original[CONTROL])
control[0]['on_text'] = """pub(crate) fn default_lanes() -> Vec<&'static dyn RetrievalLane> {
    default_lane_registry().to_vec()
}"""
control[0]['off_text'] = """pub(crate) fn default_lanes() -> Vec<&'static dyn RetrievalLane> {
    // Counterfactual snapshot: preserve policy obligations and type references,
    // then remove every local lane before run_lanes can execute any of them.
    let mut lanes: Vec<&'static dyn RetrievalLane> = default_lane_registry().to_vec();
    lanes.clear();
    lanes
}"""
assert control[1] == old_control[1]
assert {k: v for k, v in control[0].items() if k not in ('on_text', 'off_text')} == {
    k: v for k, v in old_control[0].items() if k not in ('on_text', 'off_text')}
candidate[CONTROL] = (json.dumps(control, indent=2) + '\n').encode()
control_checks = []
for entry in control:
    text = candidate[entry['path']].decode()
    assert text.count(entry['on_text']) == 1
    assert entry['off_text'] not in text
    off = text.replace(entry['on_text'], entry['off_text'], 1)
    assert off.count(entry['off_text']) == 1 and entry['on_text'] not in off
    control_checks.append({'id': entry['id'], 'path': entry['path'],
                           'on_occurrences': 1, 'off_occurrences_before': 0,
                           'off_occurrences_after_exact_replacement': 1,
                           'on_source_sha256': sha(text.encode()), 'off_source_sha256': sha(off.encode())})
    if entry['id'] == 'local_retrieval':
        view = text.split('pub(crate) fn default_lane_registry()', 1)[1].split('\n}\n', 1)[0]
        assert view == off.split('pub(crate) fn default_lane_registry()', 1)[1].split('\n}\n', 1)[0]
        assert 'lanes.clear();\n    lanes' in entry['off_text']
        assert candidate[POLICY].decode().count('crate::lanes::default_lane_registry()') == 1

for path in (LANES, POLICY, CONTROL):
    old_target = D / 'original' / path
    old_target.parent.mkdir(parents=True, exist_ok=True)
    if old_target.exists():
        assert old_target.read_bytes() == original[path]
    else:
        old_target.write_bytes(original[path])
    new_target = D / 'candidate-v2' / path
    new_target.parent.mkdir(parents=True, exist_ok=True)
    assert not new_target.exists(), 'Do not overwrite the fixed v2 candidate'
    new_target.write_bytes(candidate[path])
patch = ''
for path in (LANES, POLICY, CONTROL):
    patch += 'diff --git a/' + path + ' b/' + path + '\n'
    patch += ''.join(difflib.unified_diff(original[path].decode().splitlines(True),
                                        candidate[path].decode().splitlines(True),
                                        fromfile='a/' + path, tofile='b/' + path))
(D / 'lane-ownership-v2.patch').write_text(patch)
manifest = {
    'schema': 'round10-isolated-lane-owner-candidate-v2', 'author': '/root/pr_audit',
    'base_source': MAIN, 'base_tree': git('rev-parse', MAIN + '^{tree}').decode().strip(),
    'created_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'v1_preserved': {'manifest': 'changed-files.json', 'sha256': sha(v1_raw),
                     'patch': v1['patch'], 'candidate_root': 'candidate'},
    'changed_files': [{
        'path': p, 'mode': '100644', 'before_bytes': len(original[p]), 'after_bytes': len(candidate[p]),
        'before_git_blob': blob(original[p]), 'after_git_blob': blob(candidate[p]),
        'before_sha256': sha(original[p]), 'after_sha256': sha(candidate[p]),
        'original_relative_path': 'original/' + p, 'candidate_relative_path': 'candidate-v2/' + p,
    } for p in (LANES, POLICY, CONTROL)],
    'patch': {'file': 'lane-ownership-v2.patch', 'bytes': len(patch.encode()), 'sha256': sha(patch.encode())},
    'Rust_candidate_bytes_equal_v1': True,
    'production_decision': v1['production_decision'],
    'necessary_compatibility_update': {
        'file': CONTROL, 'affected_control': 'local_retrieval',
        'changed_fields': ['on_text', 'off_text'],
        'reason': 'The existing consumer performs a strict exactly-once literal source replacement; the v1 wrapper changed its anchor.',
        'on': 'Match only the new default_lanes Vec-returning wrapper.',
        'off': 'Clone the shared registry references into the execution Vec, clear it, then return it. No local lane executes. The static registry and policy obligations remain available, as before the cleanup.',
        'unchanged': ['control id/path', 'semantic_dense complete entry', 'replacement validator', 'strict source-matching regression', 'all build/gate rules'],
        'shared_registry_not_replaced_or_cleared': True,
        'pure_offline_text_replacement_checks': control_checks,
        'product_or_counterfactual_execution': False},
    'actual_consumers_read': [
        'crates/cc-search/src/engine.rs',
        'crates/cc-search/src/engine_lane_tests.rs',
        'crates/cc-eval/src/benchmark/ablation.rs',
        'crates/cc-eval/src/benchmark/ablation/mechanism.rs',
        'scripts/p7_mechanism_build.py'],
    'root_existing_affected_regression_command': [
        'cargo', 'test', '-p', 'cc-eval', '--lib',
        'benchmark::ablation::mechanism::tests::fixed_controls_match_current_product_source_once',
        '--', '--exact'],
    'regressions_added': v1['regressions_added'],
    'isolated_format': v1['isolated_format'],
    'Cargo_build_test_or_Clippy_result': None,
    'project_or_source_guard_executed': False,
    'repository_worktree_index_HEAD_or_refs_changed': False,
    'GitHub_or_study_mutations': False,
    'independent_review': 'Pending /root/todo_audit for exact v2; author is recused from semantic source approval.',
    'task_status': v1['task_status'],
}
(D / 'changed-files-v2.json').write_text(json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + '\n')
print(json.dumps({'files': [LANES, POLICY, CONTROL], 'patch_sha256': sha(patch.encode()),
                  'manifest_sha256': sha((D / 'changed-files-v2.json').read_bytes()),
                  'control_after_sha256': sha(candidate[CONTROL]), 'tests_executed': False}))
