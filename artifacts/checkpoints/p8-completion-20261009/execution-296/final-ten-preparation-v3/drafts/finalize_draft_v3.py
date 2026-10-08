#!/usr/bin/env python3
"""Reference-only final draft update; never executes an acceptance workload."""
import copy
import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path('/dev/shm/a217aaae3bde')
OUT = ROOT / 'runtime-review/final-ten-draft'
REPO = ROOT / 'codecortex'
CORRECTION = ROOT / 'scale-round5-review/original-acceptance-mapping-scope-correction-v2.json'
EXPECTED = '355c0dd5a7ed3d11ccb4fdfae1d082217d111e1d998c57ed10e50818e6a5f925'

def sha(data):
    return hashlib.sha256(data).hexdigest()

def ref(path):
    data = path.read_bytes()
    return {'local_path': str(path), 'bytes': len(data), 'sha256': sha(data)}

def read(name):
    return json.loads((OUT / name).read_bytes())

def save(name, obj):
    target = OUT / name
    data = (json.dumps(obj, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
    if target.exists():
        assert target.read_bytes() == data, f'refuse to overwrite {target}'
    else:
        target.write_bytes(data)
    return ref(target)

existing = {p.name: ref(p) for p in OUT.iterdir() if p.is_file() and '-v3.' not in p.name and p.name != 'finalize_draft_v3.py'}
head_before = subprocess.check_output(['git', '-C', str(REPO), 'rev-parse', 'HEAD'], text=True).strip()
tasks_path = REPO / 'docs/roadmap/code-index-v2/tasks.json'
tasks_before = tasks_path.read_bytes()
assert head_before == '29682890c89511dd6f477a6bf48bd969aa1537af'
assert sha(tasks_before) == '6e2ab2e90cbd820228e8ff397d70c770561ba20d3110e6eca31338f29a945207'
assert ref(CORRECTION)['sha256'] == EXPECTED
correction = json.loads(CORRECTION.read_bytes())
assert correction['source_commit'] == '599a7050e7d52b5b7b93975c419138e175b3f754'
assert correction['status'] == 'accepted_documentation_scope_correction_only'
assert correction['criteria_changed'] is False
assert correction['native_workloads_executed'] == 0
assert correction['product_or_observer_files_modified'] == []
assert correction['task_statuses_changed'] is False
assert ref(pathlib.Path(correction['original_mapping']['path']))['sha256'] == correction['original_mapping']['sha256']
for source in correction['source_files_checked']:
    actual = subprocess.check_output(['git', '-C', str(REPO), 'show', correction['source_commit'] + ':' + source['path']])
    assert sha(actual) == source['sha256']

correction_ref = dict(ref(CORRECTION), state=correction['status'], source_commit=correction['source_commit'],
    workflow_run=37835810882, artifact_ids=[], criteria_changed=False, native_workloads_executed=0,
    scope_zh='仅纠正派生映射的逐阶段公开查询过度主张，并补明原phase计数；不改变原任务或既有验收结果。')

catalog = read('accepted-evidence-index-v2.json')
catalog['revision'] = 3
catalog['previous_draft'] = ref(OUT / 'accepted-evidence-index-v2.json')
catalog['sources']['scale_scope_correction'] = correction_ref
catalog_ref = save('accepted-evidence-index-v3.json', catalog)

mapping = read('final-ten-acceptance-mapping-v2.json')
mapping['revision'] = 3
mapping['previous_draft'] = ref(OUT / 'final-ten-acceptance-mapping-v2.json')
mapping['accepted_evidence_index'] = catalog_ref
mapping['scale_scope_correction'] = correction_ref
for task in mapping['tasks']:
    assert task['formal_task_completion'] is False
    if task['task_id'] in ('P8-005', 'P8-006'):
        task['evidence_ids'].append('scale_scope_correction')
        task['scale_scope_correction'] = correction_ref
        for criterion in task['criteria']:
            if 'scale_mapping' in criterion['evidence_ids']:
                criterion['evidence_ids'].append('scale_scope_correction')
mapping_ref = save('final-ten-acceptance-mapping-v3.json', mapping)

draft = read('task-append-drafts-v2.json')
draft['revision'] = 3
draft['previous_draft'] = ref(OUT / 'task-append-drafts-v2.json')
draft['scale_scope_correction'] = correction_ref
for task in draft['tasks']:
    assert task['status_action_now'] == 'no_change'
    assert task['final_status_not_prepopulated'] is None
    if task['task_id'] in ('P8-005', 'P8-006'):
        task['evidence_append_draft']['review_references']['scale_scope_correction'] = correction_ref
        task['evidence_append_draft']['scale_scope_correction_note'] = correction_ref['scope_zh']
draft_ref = save('task-append-drafts-v3.json', draft)

findings = read('independent-findings-v2.json')
findings['revision'] = 3
findings['previous_draft'] = ref(OUT / 'independent-findings-v2.json')
findings['scale_scope_correction'] = correction_ref
findings['reference_only_revision_zh'] = 'v3仅将已接受的规模范围纠正原件及四个固定599 Git blob绑定到已收紧的v2文字；不追加测量或接受结论。'
findings_ref = save('independent-findings-v3.json', findings)

for name, expected in existing.items():
    assert ref(OUT / name) == expected, f'historical draft changed: {name}'
assert tasks_path.read_bytes() == tasks_before
assert subprocess.check_output(['git', '-C', str(REPO), 'rev-parse', 'HEAD'], text=True).strip() == head_before
assert mapping['formal_task_completion'] is False and draft['formal_task_completion'] is False
assert mapping['tasks'][0]['original_definition']['acceptance_subgates'] == read('final-ten-acceptance-mapping-v2.json')['tasks'][0]['original_definition']['acceptance_subgates']

manifest = {'revision': 3, 'active_drafts': [mapping_ref, draft_ref, findings_ref, catalog_ref],
    'scale_scope_correction': correction_ref,
    'previous_delivery': ref(OUT / 'delivery-manifest-v2.json'),
    'preserved_historical_files': existing,
    'added_source_records': read('delivery-manifest-v2.json')['added_source_records'],
    'all_v1_v2_bytes_preserved': True, 'original_task_definitions_unchanged': True,
    'p8_005_entire_historical_acceptance_subgates_unchanged': True,
    'source_git_blobs_rechecked': correction['source_files_checked'],
    'no_acceptance_rerun': True, 'no_repository_or_refs_changed': True,
    'repository_HEAD_before_after': head_before, 'tasks_json_unchanged': True,
    'task_status_action_now': 'no_change', 'formal_task_completion': False, 'done': 163, 'remaining': 29,
    'application_note': 'v3为当前准备草案；v1/v2保留为历史。仅在所有原依赖与门实际通过、最终独审后由root追加原task允许字段；本草案不将任何任务改done。',
    'updater': ref(pathlib.Path(__file__))}
print(json.dumps(save('delivery-manifest-v3.json', manifest), ensure_ascii=False))
for item in manifest['active_drafts']:
    print(json.dumps(item, ensure_ascii=False))
