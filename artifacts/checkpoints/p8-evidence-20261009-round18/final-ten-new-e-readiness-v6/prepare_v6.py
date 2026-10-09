"""Add a factual readiness snapshot. Never mutate the task ledger or prior records."""
import copy, datetime, hashlib, json
from pathlib import Path

OUT=Path(__file__).parent
OLD=Path('/workspace/scratch/a217aaae3bde/checkpoint-round17-prep/final-ten-new-e-readiness')
AUDIT=Path('/dev/shm/a217aaae3bde/runtime-review/e-a23-artifact-review')
SCALE=Path('/dev/shm/a217aaae3bde/scale-combined-E-review')
ART=Path('/workspace/scratch/a217aaae3bde/ci-artifacts')
HEAD='a23bb72d3c954f385b99fe81ce9189885c208557'
def ref(p):
    p=Path(p); data=p.read_bytes()
    return dict(path=str(p),bytes=len(data),sha256=hashlib.sha256(data).hexdigest(),git_blob_sha1=hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest())
def read(p):return json.loads(Path(p).read_text())
def write(n,r):
    p=OUT/n
    with p.open('x') as f:json.dump(r,f,indent=2,sort_keys=True,ensure_ascii=False);f.write('\n')
    return ref(p)
before={p.name:ref(p) for p in OLD.iterdir() if p.is_file()}
old=read(OLD/'final-ten-acceptance-mapping-v5.json')
r=copy.deepcopy(old)
e=read(OLD/'accepted-new-E-evidence-index-v5.json')
assert r['execution_head']==HEAD and r['counts']==dict(done=163,remaining=29)
review=AUDIT/'accepted/artifact-11594089439-a23bb72d-independent-final/independent-review.json'
soak=read(review)
assert soak['head']==HEAD and soak['status']=='accepted_scoped' and soak['offered']==3601
assert soak['helper']['sha256']=='476d19598c2674c3461e49ef975709d5ee6f1456da9db2465e7f97fe3e3d9e42'
assert read(AUDIT/'soak-11594089439-review-process.json')['exit_code']==0
assert ref(SCALE/'admitted-shards-004.json')['sha256']=='09b86a181b524bcdf9c41d066e8db7f77693d256011bef56f398d8100691350b'
assert ref(SCALE/'validated/shards/11593548201/review.json')['sha256']=='44baa425e6e46b2f9d18ccd5eaa81cb4c613dd0a458894267c44ed5cb63e7e08'
e['previous_v5']=ref(OLD/'accepted-new-E-evidence-index-v5.json')
e['evidence']['soak']={
    'artifact_ids':[11594089439], 'current_E_credit_only':True,
    'execution_head':HEAD,'workflow_run':37871838957,
    'record':ref(review),'process':ref(AUDIT/'soak-11594089439-review-process.json'),
    'official_metadata':[ref(ART/'11594089439/artifact-metadata.json')],
    'scope':'Actual fixed-E original sealed one-hour observation and complete cache denominator accepted; serialized soak maximum1, no saturated-C4 claim. Full Rust statistics and no-repair 15-table oracle replay actually passed.'}
e['evidence']['scale_11593548201']={
    'artifact_ids':[11593548201], 'current_E_credit_only':True,
    'execution_head':HEAD,'workflow_run':37872522779,
    'record':ref(SCALE/'validated/shards/11593548201/review.json'),
    'process':ref(SCALE/'shard-11593548201-execution.json'),
    'official_metadata':[ref(ART/'11593548201/artifact-metadata.json')],
    'scope':'Accepted original 50k repetition0 shard,9 samples only; full matrix pending.'}
e['evidence']['scale_admitted_004']={
    'artifact_ids':[11591367982,11591464502,11591693031,11593548201],
    'current_E_credit_only':True,'execution_head':HEAD,'workflow_run':37872522779,
    'record':ref(SCALE/'admitted-shards-004.json'),
    'official_metadata':[ref(ART/str(i)/'artifact-metadata.json') for i in [11591367982,11591464502,11591693031,11593548201]],
    'scope':'Immutable four-shard checkpoint:4/150 shards and41/1500 samples. 1k14+5k9+10k9+50k9; five fanout cases only from1k. Original combine/CIaggregate and100k/all remaining repetitions pending.'}
e['evidence']['scale_state_snapshot']['scope']='Read-only initialization state reference; immutable admitted-shards-004 is the current cumulative checkpoint, not this initialization file.'
r['revision']=6;r['prepared_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
r['previous_v5']=ref(OLD/'final-ten-acceptance-mapping-v5.json')
r['evidence_index']=write('accepted-new-E-evidence-index-v6.json',e)
r['scale_scope_note']='四片41样本为1k14+5k9+10k9+50k9；五个fanout仅由1k片提供，不把各大规模片宣称已经完成fanout。150片/1500样本、全局N30、原combine与CIaggregate仍待。'
for gate in r['pending_final_gates']:
    if gate['id']=='scale_full_matrix':
        gate.update(accepted_samples=41,accepted_shards=4,note='1k14+5k9+10k9+50k9 only;100k and all remaining repetitions pending. Original five-hour/native and350minute workflow budgets unchanged.')
    if gate['id']=='new_E_soak_and_cache':
        gate.update(status='accepted_scoped',artifact_id=11594089439,record=ref(review),note='Original raw acceptance completed once with actual helper exit0:3601 successes,3600.053467347s,2400 compound reads/all9600RPC, completion-time quarters, cache generations/invalidation/refill/hits, original RSS/queue rule and15-table parity. Serial maximum1 explicitly retained; formal task closure remains pending.')
common='新E十九job原CI/P7日志、四P7原件及新E一小时cache soak完整原件已有独审；仍须完整新E规模矩阵/原combine、持久原raw交付，以及root对fresh main/最终PR必要门和合并的真实核对。当前不关闭任务；163 done/29 remain。旧599/296/e95不计新E样本。'
scale005='新E release build11591482043及实际binary5a13565e81636ef3ebd0ff206c1188b1ab20d80ef858af54590a35fb27a95086已由原init准入；当前1k/5k/10k/50k四片41样本，完整五档×30的150片/1500样本尚待。'
scale006='新E原协议cold/no_op/body/api/config/batch_1/10/100/1000和fanout_1/4/16/64/128逐样本原件已有四片41样本；五个fanout仅来自1k片。须完整矩阵、首次incomplete及之后闭包，不用局部样本代表整体。'
soaktexts=[
 '新E原soak11594089439实际3601唯一成功操作、3600.053467347秒，持续六类修改、200次真实分支切换和25次catalog压实；2400复合read覆盖全部9600RPC，逐原wire核对得到1400hit/1000miss/999失效，各真实完成时间quarter600read/350hit/250miss。保持同一native进程和共享query pool复用；不声称逐OS线程身份或semantic worker复用，后者由单独768请求backfill限定覆盖。',
 '新E原增量终点不补修，与fresh full全部15表oracle实际重放相同；原Rust统计器真实exit0且输出逐字相同。原17/2110 seals前后验证、1087产品输入/4e0与9观察器均绑定实际E Git。旧296成功范围保持旧来源，不代替本次。',
 '3594次资源采样最大间隔1.028141448秒，满足原5秒覆盖上限；原RSS warmed115081216/tail144728064 <= allowed177405952，规则独立重算；终点queue/CPU/async占用及query pins均0，所有owned worker/sampler/product停止且construction_pending=false。观测有限一小时与原median规则，不宣称任意时长统计无泄漏；配置C4但隔离cache读写实际串行peak1，无饱和并发主张。'
]
for task in r['tasks']:
    assert task['formal_task_completion'] is False and task['status_action_now']=='no_change'
    if task['task_id'] in ('P8-005','P8-006'):
        task['evidence_ids']=[x if x!='scale_admitted_003' else 'scale_admitted_004' for x in task['evidence_ids']]
        text=scale005 if task['task_id']=='P8-005' else scale006
        task['criteria'][0]['mapped_observation_zh']=text;task['scope_limits_zh'][0]=text
        for c in task['criteria']:
            c['evidence_ids']=[x if x!='scale_admitted_003' else 'scale_admitted_004' for x in c['evidence_ids']]
    for c in task['criteria']:
        if c['literal']=='相关旧功能回归通过；没有证据的项标not_run/blocked而非done。':
            c['mapped_observation_zh']=common
            if 'soak' not in c['evidence_ids']:c['evidence_ids'].append('soak')
    if task['task_id']=='P8-010':
        task['own_scope_state']='accepted_scoped_original_E_observation'
        task['evidence_ids']=['soak','backfill']
        task['scope_limits_zh']=soaktexts
        for i,c in enumerate(task['criteria']):
            c['scope_state']='accepted_scoped_with_dependencies_and_final_gates_pending'
            if 'soak' not in c['evidence_ids']:c['evidence_ids'].insert(0,'soak')
            if i<3:c['mapped_observation_zh']=soaktexts[i]
    prev=next(t for t in old['tasks'] if t['task_id']==task['task_id'])
    for key in ['original_definition','original_definition_sha256','hard_dependencies','status_now','status_action_now','formal_task_completion','validation_mapping']:
        assert task[key]==prev[key],(task['task_id'],key)
    assert [(c['original_field'],c['index'],c['literal']) for c in task['criteria']]==[(c['original_field'],c['index'],c['literal']) for c in prev['criteria']]
assert r['tasks'][0]['acceptance_subgates_immutable_history']==old['tasks'][0]['acceptance_subgates_immutable_history']
mapping=write('final-ten-acceptance-mapping-v6.json',r)
after={p.name:ref(p) for p in OLD.iterdir() if p.is_file()};assert before==after
write('v6-additive-readiness-receipt.json',dict(schema=1,status_action='no_change',formal_task_completion=False,counts=dict(done=163,remaining=29),previous_v5=ref(OLD/'final-ten-acceptance-mapping-v5.json'),mapping=mapping,evidence_index=r['evidence_index'],preparer=ref(__file__),prior_directory_bytes_unchanged=True,all_original_definitions_literals_hard_dependencies_and_005_history_unchanged=True,native_guard_tests_rerun=False,accepted_new_observation='soak11594089439',scale_snapshot='4/150 shards,41/1500 samples',scope='Conditional readiness only; full matrix, original combine, durable raw benchmark navigation and final actual PR/main closure remain pending.'))
print(json.dumps(mapping))
