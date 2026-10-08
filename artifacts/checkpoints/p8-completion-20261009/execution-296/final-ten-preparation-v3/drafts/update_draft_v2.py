"""Append a newer draft revision from real accepted evidence; keep all v1 bytes."""
import copy
import datetime
import hashlib
import json
from pathlib import Path
import subprocess

BASE = Path('/dev/shm/a217aaae3bde')
ROOT = BASE / 'codecortex'
OUT = BASE / 'runtime-review/final-ten-draft'
CHECKPOINT = 'b687a0a3e3095ac913e6331a42e85d1e9f0722e5'
BENCH = 'artifacts/benchmarks/p8-completion-20261009/'
NEW = '29682890c89511dd6f477a6bf48bd969aa1537af'
SCOPE = '已接受当次精确296 checkout的P7 semantic offline实际回归日志；保留3个合理skipped步骤，不宣称新原artifact完整重放、live provider/费用/发行或TODO认证。'

def sha(data): return hashlib.sha256(data).hexdigest()
def read(path): return json.loads(Path(path).read_text())
def ref(path):
    path = Path(path)
    r = {'local_path': str(path), 'bytes': path.stat().st_size, 'sha256': sha(path.read_bytes())}
    if path.is_relative_to(ROOT): r['repository_path'] = str(path.relative_to(ROOT))
    return r
def write(name, value):
    p = OUT / name
    assert not p.exists(), p
    p.write_text(json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + '\n')
    return ref(p)
def git_ref(path):
    data = subprocess.check_output(['git','show',CHECKPOINT + ':' + path],cwd=ROOT)
    blob = subprocess.check_output(['git','rev-parse',CHECKPOINT + ':' + path],cwd=ROOT,text=True).strip()
    if (ROOT / path).exists(): assert (ROOT / path).read_bytes() == data
    return {'repository_path': path, 'commit': CHECKPOINT, 'git_blob': blob, 'bytes': len(data), 'sha256': sha(data), 'scope': '已存在原记录交付导航；不新增原测量或接受结论。'}

old_files = {p.name: ref(p) for p in OUT.iterdir() if p.is_file() and p.name != Path(__file__).name}
task_before = (ROOT / 'docs/roadmap/code-index-v2/tasks.json').read_bytes()
head_before = subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
semantic_dir = ROOT / 'artifacts/checkpoints/p8-completion-20261009/execution-296/p7-offline-semantic'
semantic_review = ref(semantic_dir / 'p7-offline-semantic-independent-review.json')
semantic_log = ref(semantic_dir / 'job-113541229427-original.log')
semantic_metadata = ref(semantic_dir / 'p7-offline-semantic-job-metadata.json')
assert semantic_review['sha256'] == '6274fac8f3dd62ab3e30bb18e7cfe8ef0d9717a6b14cecb99f11aaf970796440'
assert semantic_log['sha256'] == '01594007ca2782fb19bfd9553d8e3da8fbf998fb4eb0f84a90eb567f7ef5aff8' and semantic_log['bytes'] == 50826
r = read(semantic_review['local_path'])
assert r['execution_head'] == r['actual_checkout'] == NEW
assert r['status'] == 'accepted_scoped_actual_p7_offline_semantic_regression_log'
assert r['job_id'] == 113541229427 and r['run_id'] == 37844310901
assert r['actual_failure_markers'] == [] and r['rust_ignored'] == 0
assert semantic_metadata['sha256'] == r['job_metadata_sha256']
semantic = {'id': 'current296_p7_semantic', 'status': 'accepted_scoped_actual_regression_log',
    'execution_head': NEW, 'run_id': r['run_id'], 'job_id': r['job_id'], 'review': semantic_review,
    'original_log': semantic_log, 'job_metadata': semantic_metadata, 'scope': SCOPE,
    'artifact_id_observed_in_original_log': r['artifact_upload']['id'], 'fresh_artifact_full_replay_claimed': False}

raw_paths = {
 'P8-005': [BENCH+'scale-599/original-artifacts/11576810573/p8-scale-build-37835810882.zip.parts/archive-parts.json'],
 'P8-006': [BENCH+'scale-599/original-artifacts/11576810573/p8-scale-build-37835810882.zip.parts/archive-parts.json'],
 'P8-007': [BENCH+f'runtime-296/{name}/{aid}-non-elf-derivation.json' for name,aid in [('mixed-c1',11582280257),('mixed-c4',11581418255),('mixed-c8',11583115555),('mixed-c16',11582753157),('backfill',11581721952)]],
 'P8-008': [BENCH+'lifecycle-599/11576107053-non-elf-derivation.json', BENCH+'lifecycle-599/11576107053-non-elf-original-members.tar.gz.parts/archive-parts.json'],
 'P8-009': [BENCH+'lifecycle-599/11576107053-non-elf-derivation.json', BENCH+'lifecycle-599/11576107053-non-elf-original-members.tar.gz.parts/archive-parts.json'],
 'P8-010': [BENCH+'runtime-296/soak/11585132175-non-elf-derivation.json',BENCH+'runtime-296/soak/11585132175-non-elf-original-members.tar.gz.parts/archive-parts.json',BENCH+'runtime-296/backfill/11581721952-non-elf-derivation.json'],
 'P8-011': [BENCH+'platform-p7-gates-599/'+p for p in ['platform-recovery-manifest.json','p7-closeout-manifest.json','p7-engineering-manifest.json']],
 'P8-012': [BENCH+'platform-p7-gates-599/'+p for p in ['platform-recovery-manifest.json','p7-engineering-manifest.json']],
 'P8-013': [BENCH+'platform-p7-gates-599/gates-manifest.json'],
 'P8-016': [BENCH+'platform-p7-gates-599/'+p for p in ['platform-recovery-manifest.json','p7-closeout-manifest.json']],
}
for tid in ['P8-005','P8-006']:
    raw_paths[tid] += [BENCH+f'scale-599/original-artifacts/{aid}/{name}' for aid in [11577045484,11576619839,11577518538,11580138693] for name in ['artifact-metadata.json','review.json']]
for paths in raw_paths.values(): paths.append(BENCH+'ARCHIVE-FORMAT.md')
navigation = {p: git_ref(p) for paths in raw_paths.values() for p in paths}
nav_ref = write('benchmark-raw-navigation-v2.json', {'checkpoint': CHECKPOINT, 'paths': navigation, 'per_task_paths': raw_paths,
    'scope': '绑定原deliverables要求的benchmarks原记录分区/来源/遗漏清单；交付包不是完整ZIP，原binary重放必须恢复收据绑定的省略文件。',
    'current296_semantic_not_in_this_earlier_checkpoint': semantic})
update_ref = write('accepted-regression-update-v2.json', {'supersedes_only_current_pending_state': 'current296_p7_semantic',
    'accepted': semantic, 'still_pending_current_p7': ['mechanism'], 'v1_pending_snapshot_preserved': True,
    'formal_task_completion': False, 'done': 163, 'remaining': 29})

old_phrase = '默认/semantic包发布构建范围'
new_phrase = '默认/semantic两包的release配置冷构建覆盖'
old_pending_text = '296 P7 mechanism/semantic'
new_pending_text = '296 P7 mechanism（semantic已按原日志接受）'
def prose_update(value):
    if isinstance(value, str): return value.replace(old_phrase,new_phrase).replace(old_pending_text,new_pending_text)
    if isinstance(value, list): return [prose_update(x) for x in value]
    if isinstance(value, dict): return {k:prose_update(v) for k,v in value.items()}
    return value
def remove_pending_ids(rows):
    for row in rows:
        row['pending_final_gate_ids'] = [x for x in row['pending_final_gate_ids'] if x != 'current296_p7_semantic']

mapping = prose_update(read(OUT/'final-ten-acceptance-mapping.json'))
mapping['revision'] = 2
mapping['prepared_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
mapping['previous_draft'] = old_files['final-ten-acceptance-mapping.json']
mapping['scale_progress_observed_utc'] = '2026-10-08T23:23:48Z'
mapping['pending_final_gates'] = [p for p in mapping['pending_final_gates'] if p['id'] != 'current296_p7_semantic']
mapping['accepted_current_regression_gates'] = [semantic]
mapping['benchmark_raw_navigation'] = nav_ref
remove_pending_ids(mapping['tasks'])
for row in mapping['tasks']:
    row['benchmark_raw_paths'] = raw_paths[row['task_id']]
    row['accepted_current_regression_gate_ids'] = ['current296_p7_semantic']
    if row['task_id'] in ['P8-005','P8-006']: row['partial_sample_snapshot_utc'] = '2026-10-08T23:23:48Z'
    if row['task_id'] == 'P8-006':
        for criterion in row['criteria']:
            if criterion['original_field'] == 'steps' and criterion['index'] == 1:
                criterion['mapped_observation_zh'] = '逐build保留原raw IndexReport的files_*、dirty_plan、document_changes、project_model计数；同时保留phase_timing/build_timing与index/outer/full_control/parity计时，嵌套时间不重复相加。'
            if criterion['original_field'] == 'acceptance' and criterion['index'] == 0:
                criterion['mapped_observation_zh'] = '每检查点保留原15表完整canonical parity及integrity/FK；config独立SQL target与fanout手写事实见证。该scale路径没有逐阶段public search probes，公开查询旧回归另引实际P7/CI。中间incomplete及rebuild不丢弃，比较前不修增量侧；当前部分片不能代替完整接受。'

draft = prose_update(read(OUT/'task-append-drafts.json'))
draft['revision'] = 2
draft['previous_draft'] = old_files['task-append-drafts.json']
draft['scale_progress_observed_utc'] = '2026-10-08T23:23:48Z'
draft['accepted_current_regression_gates'] = [semantic]
for row in draft['tasks']:
    evidence = row['evidence_append_draft']
    evidence['pending_final_gate_ids'] = [x for x in evidence['pending_final_gate_ids'] if x != 'current296_p7_semantic']
    evidence['accepted_current_regression_gates'] = [semantic]
    evidence['artifacts'] += [p for p in raw_paths[row['task_id']] if p not in evidence['artifacts']]
    evidence['artifacts'] += [semantic_review['repository_path'],semantic_log['repository_path']]
    evidence['benchmark_raw_navigation'] = {p:navigation[p] for p in raw_paths[row['task_id']]}
    evidence['current_regression_scope_note'] = SCOPE
    if row['task_id'] in ['P8-005','P8-006']:
        evidence['scale_progress_observed_utc'] = '2026-10-08T23:23:48Z'
        evidence['summary'] = '截至2026-10-08T23:23:48Z的已读原件快照：' + evidence['summary']
        row['implementation_notes_append_draft'] += ' 规模部分计数对应2026-10-08T23:23:48Z原件快照，之后新增结果须另行读取。'
    if row['task_id'] == 'P8-006':
        evidence['measurement_detail'] = '原IndexReport files_*、dirty_plan、document_changes、project_model逐build计数与phase/build timing分别保留；15表canonical/integrity/FK、config SQL target及fanout事实见证。此scale路径未做逐阶段public search probes，公开查询回归另绑实际P7/CI。'
        row['implementation_notes_append_draft'] += ' 原phase计数含files_*、dirty_plan、document_changes、project_model；scale对账是完整15表canonical/integrity/FK及SQL/事实见证，不扩写成逐阶段公开查询。'

findings = prose_update(read(OUT/'independent-findings.json'))
findings['revision'] = 2
findings['previous_draft'] = old_files['independent-findings.json']
findings['actual_pending_blockers'] = [p for p in findings['actual_pending_blockers'] if p['id'] != 'current296_p7_semantic']
findings['accepted_current_regression_gates'] = [semantic]
findings['peer_review_corrections'] = [
    'scale_closeout独立发现早期派生map错误地把公开查询写到每scale阶段；v2已改为完整15表canonical/integrity/FK、config SQL target/fanout事实断言，public-query回归另归真实P7/CI。原tasks字面条款不变，旧map不改。',
    'P8-006原step要求各phase计数，v2明确列files_*/dirty_plan/document_changes/project_model，不再只写timing。',
    'pr_audit确认011/012/013/016无新增own-scope缺口，建议补benchmarks原raw导航；v2已绑定实际证据checkpoint原manifest与ARCHIVE-FORMAT。',
    'P8-016的发布构建歧义改为默认/semantic两包release配置冷构建覆盖，不声称已经发布。',
    '仅凭真实新增296 semantic原日志独审更新该pending；mechanism及其余门仍待。v1 pending快照完整保留。']

map_ref = write('final-ten-acceptance-mapping-v2.json', mapping)
draft_ref = write('task-append-drafts-v2.json', draft)
find_ref = write('independent-findings-v2.json', findings)
index = read(OUT/'accepted-evidence-index.json')
index['revision'] = 2
index['previous_draft'] = old_files['accepted-evidence-index.json']
index['sources']['current296_p7_semantic'] = semantic
index['benchmark_raw_navigation'] = nav_ref
index_ref = write('accepted-evidence-index-v2.json', index)

for tid in [r['task_id'] for r in mapping['tasks']]:
    old = next(r for r in read(OUT/'final-ten-acceptance-mapping.json')['tasks'] if r['task_id']==tid)
    new = next(r for r in mapping['tasks'] if r['task_id']==tid)
    assert old['original_definition'] == new['original_definition']
    assert old['status_now'] == new['status_now'] == 'in_progress'
assert all(row['status_action_now']=='no_change' and row['final_status_not_prepopulated'] is None for row in draft['tasks'])
assert mapping['formal_task_completion'] is draft['formal_task_completion'] is findings['formal_task_completion'] is False
assert (ROOT/'docs/roadmap/code-index-v2/tasks.json').read_bytes() == task_before
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()==head_before
for name,item in old_files.items(): assert sha((OUT/name).read_bytes())==item['sha256']
delivery = write('delivery-manifest-v2.json', {'revision':2,'active_drafts':[map_ref,draft_ref,find_ref,index_ref],
    'added_source_records':[nav_ref,update_ref],'preserved_v1_files':old_files,'updater':ref(Path(__file__)),
    'repository_HEAD_before_after':head_before,'tasks_json_unchanged':True,'all_v1_bytes_preserved':True,
    'no_acceptance_rerun':True,'no_repository_or_refs_changed':True,'formal_task_completion':False,'done':163,'remaining':29,
    'application_note':'以v2为当前准备草案；v1仅历史pending快照。任务仍不准自动done，原005全部历史subgates与所有原定义不改。'})
print(json.dumps(delivery,ensure_ascii=False))
