from pathlib import Path
import json,hashlib,datetime,copy,collections
root=Path.cwd();review=root/'review-runtime';source=root/'source'
prefix='artifacts/benchmarks/p8-bfccb8494ba0'
def read(p):return json.loads(p.read_text())
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for part in iter(lambda:f.read(1048576),b''):h.update(part)
 return h.hexdigest()
def ref(relative):
 p=root/relative
 return {'path':prefix+'/'+relative,'bytes':p.stat().st_size,'sha256':sha(p)}
pending='review-runtime/p8-007-010-evidence-append-draft.json'
pending_md='review-runtime/p8-007-010-evidence-append-draft.md'
assert sha(root/pending)=='828e4468feaf6bc7d9eea1d35387e75ee37a09e357025e4c3cd8cf538a3367fc'
assert sha(root/pending_md)=='e95c4d74fcbce4b0fc69301d8cafbc57a8b3f4b147e419600cffb1447e9901c8'
draft=read(root/pending);identity=draft['frozen_source']
expected='a23bb72d3c954f385b99fe81ce9189885c208557'
assert identity['source_sha']==expected
task_path=source/'docs/roadmap/code-index-v2/tasks.json';task_sha=sha(task_path)
tasks_doc=read(task_path);tasks={t['id']:t for t in tasks_doc['tasks']}
assert task_sha==draft['frozen_tasks_sha256']
counts=dict(collections.Counter(t['status'] for t in tasks.values()))
assert counts=={'done':163,'in_progress':16,'todo':12,'blocked':1}
a='raw/11594089439/extracted/'
audit=read(review/'runtime-11594089439-review-v2.json')
binding=read(review/'soak-11594089439-stdio-binding-review.json')
native=read(root/(a+'p8-runtime/report.json'));build=read(root/(a+'p8-build/build-receipt.json'))
terminal=read(review/'soak-11594089439-github-terminal.json')
assert audit['review_status']=='raw_and_receipt_review_passed' and audit['errors']==[]
assert binding['review_status']=='original_stdio_binding_passed' and binding['errors']==[]
assert audit['source_sha']==binding['source_sha']==expected
assert native['status']=='passed_observation' and native['exit_code']==0 and native['failures']==[]
assert terminal['job']['conclusion']=='success' and terminal['job']['head_sha']==expected
assert terminal['artifact']['id']==11594089439
assert build['source_before']==build['source_after'] and build['source_before']['source_commit']==expected
assert build['observer_before']==build['observer_after']
assert native['rss']['passed'] and native['resource_time_coverage']['passed'] and native['cache_reuse']['passed']
assert native['observed_work_ns']==3600053467347
assert native['outcomes']=={'success':3601}
assert native['task_complete'] is False and native['release_approval'] is False
assert native['actual_concurrency']['maximum']==1 and native['actual_concurrency']['read_build_overlap'] is False
assert len(audit['parity_tables'])==15 and all(t['equal'] for t in audit['parity_tables'])
soak={
 'source_sha':expected,'run_id':37871838957,'run_attempt':1,'job_id':113631481157,'artifact_id':11594089439,
 'job_url':terminal['job']['html_url'],
 'artifact_url':'https://github.com/jyqj/codecortex/actions/runs/37871838957/artifacts/11594089439',
 'archive_download_url':terminal['artifact']['archive_download_url'],
 'archive_bytes':51069087,'archive_sha256':'d6debfa5e4a51bb59902cdb69a35ae7626a1e20b09bcaa5ab6cfda9ee3237086',
 'artifact_path':prefix+'/raw/11594089439',
 'transport_receipt':ref('raw/11594089439/transport-receipt.json'),
 'github_metadata':ref('raw/11594089439/github-metadata.json'),
 'zip_members':ref('raw/11594089439/zip-members.json'),
 'native_terminal':{'status':terminal['job']['status'],'conclusion':terminal['job']['conclusion'],'job_started_at':terminal['job']['started_at'],'job_completed_at':terminal['job']['completed_at'],'timing':terminal['timing']},
 'original_plan':{'files':1000,'configured_concurrency':4,'offered_operations':3601,'builds':1201,'compound_reads':2400,'read_rpc_roles':['before_status','symbol','hybrid','after_status'],'interval_ms':1000,'minimum_actual_work_ns':3600000000000},
 'observed_work_ns':native['observed_work_ns'],'outcomes':native['outcomes'],
 'actual_workload_concurrency':native['actual_concurrency'],
 'observer_concurrency_scope':native['observer_concurrency_scope'],
 'soak_admission_scope':native['cache_reuse']['isolation'],
 'original_rss_gate':native['rss'],'original_resource_coverage_gate':native['resource_time_coverage'],
 'original_cache_gate':native['cache_reuse'],
 'branch_switches':native['real_branch_switches'],'catalog_compactions':native['observed_catalog_compactions'],
 'owned_cleanup':native['owned_cleanup'],
 'semantic_backfill':native['semantic_backfill'],
 'all_attempt_latency':audit['latency'],'separate_operation_and_mutation_statistics':audit['statistics_groups'],
 'original_double_statistics_replay':native['statistics'],
 'all_original_15_table_parity':audit['parity_tables'],
 'original_raw_kind_counts':audit['raw_kind_counts'],
 'original_actual_stdio_counts':audit['stdio_counts'],
 'original_status_binding':{k:binding[k] for k in ['compound_reads','compound_rpc_roles','status_probe_responses','background_resource_snapshots','all_status_rpcs','actual_tool_counts']},
 'original_runtime_report_flags':{'task_complete':False,'release_approval':False,'interpretation':'These original observation flags are preserved; task completion depends on the separate complete integration/dependency decision, not on mutating this native report.'},
 'observer_before':build['observer_before'],'observer_after':build['observer_after'],
 'source_input_count':audit['source_input_count'],'observer_count':audit['observer_count'],
 'runtime_seal_file_count':audit['seal_file_count'],'build_seal_file_count':audit['build_seal_file_count'],
 'raw':ref(a+'p8-runtime/raw.jsonl'),'actual_stdio':ref(a+'p8-runtime/product/rpc.jsonl'),
 'full_control_stdio':ref(a+'p8-runtime/full-product/rpc.jsonl'),
 'plan':ref(a+'p8-runtime/plan.json'),'report':ref(a+'p8-runtime/report.json'),
 'parity':ref(a+'p8-runtime/parity.json'),
 'statistics':ref(a+'p8-runtime/statistics.json'),'statistics_replay':ref(a+'p8-runtime/statistics-replay.json'),
 'statistics_execution':ref(a+'p8-runtime/statistics-execution.json'),
 'runtime_seal':ref(a+'p8-runtime/seal.json'),'build_receipt':ref(a+'p8-build/build-receipt.json'),
 'build_seal':ref(a+'p8-build/seal.json'),
 'independent_raw_review':ref('review-runtime/runtime-11594089439-review-v2.json'),
 'independent_stdio_binding_review':ref('review-runtime/soak-11594089439-stdio-binding-review.json'),
 'original_decoded_github_log':ref('review-runtime/github-job-113631481157-soak.log'),
 'terminal_metadata_review':ref('review-runtime/soak-11594089439-github-terminal.json'),
 'scope_limits':[
  'Configured C4 had actual workload call peak 1 and no read/build overlap; preserve original shared admission lock. Separate status sampler is outside workload C and workload latency.',
  'RSS original rule compares first/last sample-sequence quarters; cache gate quarters use actual completion times. These quarter definitions are not substituted for each other.',
  'Observed finite one-hour RSS/coverage gates do not claim unbounded-time memory stability or a sampled RSS value as a guaranteed process lifetime maximum.',
  'Runtime records server native SELF current RSS and separate runner lifetime high-water; it does not collect a complete owned process tree. Lifecycle owns separate stage tree observations.',
  'Each compound read contributes one offered latency outcome including all four roles; 9600 role RPCs are not 9600 extra latency samples.',
  'Latency strata and original binomial-order-statistics IID assumption remain separate and descriptive. Original null p99 confidence upper endpoints for individual mutations remain null.',
  'The final fresh-full control starts only after original incremental work is drained. It does not repair the incremental index before parity comparison.',
  'Original semantic backfill is not run in this default-product soak profile; the separately retained original fake-provider worker experiment supplies its declared contention scope.',
  'No a23 execution credit is transferred to e95, D0, 34aa, G1/G2, or new P305bf145. No new primary execution was made during these read-only reviews.'
 ]
}
entries=copy.deepcopy(draft['proposals'])
for tid,entry in entries.items():
 entry['existing_task_status']=tasks[tid]['status']
 entry['proposed_task_status']=tasks[tid]['status']
 entry['evidence_to_append']['review_round']=5
 entry['evidence_to_append']['assessment_time']=datetime.datetime.now(datetime.timezone.utc).isoformat()
 entry['evidence_to_append']['integration_state']='complete_original_component_evidence_available_pending_dependency_and_regression_integration'
 entry['evidence_to_append']['task_status_not_changed']=True
 entry['evidence_to_append']['frozen_dependency_state_interpretation']='Frozen source task states are recorded, not an extra rerun gate; the parent integration may close genuinely satisfied dependencies together using their complete fixed-source evidence.'
entries['P8-007']['evidence_to_append']['observations']['separate_completed_soak_reference']=ref('review-runtime/runtime-11594089439-review-v2.json')
entries['P8-007']['evidence_to_append']['observations']['separate_completed_soak_scope']='The four mixed profile distributions remain separate from the now-complete one-hour soak. Soak C4 actual 1 is not substituted for mixed actual 4 or used as overlap credit.'
entries['P8-009']['evidence_to_append']['observations']['separate_runtime_soak_resource_observations']={'artifact_id':11594089439,'original_rss_gate':native['rss'],'original_resource_coverage_gate':native['resource_time_coverage'],'runtime_process_tree_scope':audit['runtime_process_tree_scope'],'review':ref('review-runtime/runtime-11594089439-review-v2.json'),'scope':'Separate actual runtime server SELF and runner lifetime high-water profile. Do not add them to lifecycle owners or treat lifecycle stage tree observations as continuous runtime tree sampling.'}
e=entries['P8-010']['evidence_to_append']
e.update({
 'status':'original_component_observation_and_complete_raw_review_passed_not_task_closure',
 'summary':'The exact-a23 original one-hour soak reached native success and passed unchanged original raw/stdio/receipt/statistics/RSS/coverage/cache/full-15-table gates: 3601/3601 successful terminal operations, 1201 real mutations and 2400 compound reads. Full supplemental original stdio binding passed. This separately dated terminal assessment preserves the earlier pending snapshot.',
 'scope':'Original 1000-file fixed 3601-operation/1000ms schedule; observed actual work 3600053467347 ns; real Git/catalog work and cache reuse across all four actual-time quarters; complete unmodified all-attempt denominators, ownership cleanup and endpoint full-control parity.',
 'artifact_paths':[prefix+'/raw/11594089439',prefix+'/review-runtime/runtime-11594089439-review-v2.json',prefix+'/review-runtime/soak-11594089439-stdio-binding-review.json',prefix+'/review-runtime/soak-11594089439-github-terminal.json'],
 'observations':{'soak':soak,'prior_pending_snapshot_preserved':ref(pending),'primary_soak_execution_and_artifact_review_complete':True,'additional_primary_rerun_requested':False},
 'remaining_acceptance':['Integrate the complete original hard-dependency evidence for P8-009 and its dependency chain before task status change. The frozen source statuses remain historical, not a requirement to duplicate successful original work.','Map the complete fixed-source V07/V17/V20 and related regression evidence across the team. This runtime review does not certify unreviewed validation families, global release approval, or a different product source.']
})
component_reviews=[ref('review-runtime/'+name) for name in [
 'runtime-11592751905-review-v2.json','runtime-11591493782-review-v2.json',
 'runtime-11593165899-review-v2.json','runtime-11592154271-review-v2.json',
 'backfill-11591821446-review.json','lifecycle-11591607348-review-v2.json',
 'runtime-11594089439-review-v2.json','soak-11594089439-stdio-binding-review.json']]
for item in component_reviews:
 p=root/Path(item['path']).relative_to(prefix)
 rd=read(p)
 assert rd['review_status'] in ('raw_and_receipt_review_passed','original_stdio_binding_passed')
 assert rd['errors']==[]
doc={
 'schema_version':1,'document_kind':'round5_separately_appended_original_runtime_terminal_assessment',
 'generated_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'reviewer':'pr_triage','frozen_source':identity,'original_runtime_run_id':37871838957,
 'frozen_tasks_path':'docs/roadmap/code-index-v2/tasks.json','frozen_tasks_sha256':task_sha,
 'baseline':{'total':192,'done':163,'remaining':29,'status_counts':counts,'new_closures_by_this_review':0},
 'prior_pending_snapshot':ref(pending),'prior_pending_markdown':ref(pending_md),
 'all_seven_original_component_observations_reviewed':True,
 'original_component_names':['mixed_C1','mixed_C4','mixed_C8','mixed_C16','fake_backfill','lifecycle','one_hour_soak'],
 'component_review_references':component_reviews,
 'review_scope':'Original exact-a23 primary observations and original negative-control/collector artifacts; full independent raw/stdio/receipt reconstruction. No task ledger/source/original artifact changes, no primary rerun, no P execution credit.',
 'new_soak_terminal_assessment':soak,'proposals':entries,
 'task_integration_state':'review_only_ready_for_parent_fixed_source_dependency_and_regression_closeout',
 'task_status_mutations':False,'candidate_product_execution_credit':False,'release_approval':False
}
out=review/'round5-p8-007-010-terminal-evidence.json'
assert not out.exists(),'Preserve existing separately dated assessments'
out.write_text(json.dumps(doc,ensure_ascii=False,sort_keys=True,indent=2)+'\n')
md=review/'round5-p8-007-010-terminal-review.md'
assert not md.exists()
md.write_text('''# 第五轮：原 a23 runtime / lifecycle 终态独审

原 source 为 a23bb72d3c954f385b99fe81ce9189885c208557。七个原组件（C1/C4/C8/C16 mixed、fake-backfill、lifecycle、one-hour soak）均已有完整原件与独审通过证据。本文件只是追加终态评估；第四轮 pending 草案保持原字节，没有修改 tasks.json。原账本仍为 163 done / 29 剩余，本次审查未自行关闭任务。

## 原 soak 终态

GitHub 原 run 37871838957 / job 113631481157 于 2026-10-09 03:49:15 UTC success。工作流 workload step 为 02:48:58–03:49:03 UTC。原实际工作时长为 3,600,053,467,347 ns；driver-relative 开始/结束为 764,326,883 / 3,600,817,795,323 ns，两次不同 clock 调用形成的 1,093 ns 差异保留。step UTC 包含准备和收尾，不是精确工作边界的 UTC 映射。

全部 3,601 次 offered 操作成功：1,201 次 build / 2,400 次 compound read。四个 read 角色各 2,400 次，总 9,600 role RPC 仍只形成 2,400 个 read 延迟样本。实际调用峰值为 1，配置 C4，read/build overlap=false；这是原 soak 共用 admission lock 的真实结果。四档 mixed 的实际峰值仍为 1/4/7/12，绝不把 soak 结果并入它们的分布或升格为重叠认证。

200 次真实 Git 分支切换、25 次 catalog 压实、停止投递后完整排空与唯一 fresh-full 对照均已从原件复核。原 15 表全部一致。两个 product 均停止，sampler 停止，unfinished_work=0，两个 construction_pending=false；比较前无增量修复调用。

## 原 RSS、缓存与分母

| 原门 | 原实测值 | 原结论 |
|---|---|---|
| 实际工作时长 | 3,600.053467347 秒 | 通过原至少一小时门 |
| RSS 样本数 | 3,594 | 完整原分母 |
| RSS warmed / tail median | 115,081,216 / 144,728,064 bytes | tail 小于原 allowed 177,405,952 |
| RSS sampled peak | 149,659,648 bytes | 只是采样峰值 |
| 最大采样间隔 | 1,028,141,448 ns | 小于原 5,000,000,000 ns |
| cache hit / miss / invalidation | 1,400 / 1,000 / 999 | 原 cache identity 与四季度门通过 |
| 每实际时间季度 read | 各 600 | 每季度 hit 350 / miss 250；invalidation 249/250/250/250 |
| 原 status RPC | 8,395 | 4,800 compound probes + 3,594 resource + 1 endpoint |

RSS 的首尾样本序列季度与 cache 的实际完成时间季度保持不同定义。server SELF current RSS 与 runner lifetime high-water 保持不同归属，runtime 未采完整 process tree；lifecycle 的阶段树快照不能补成连续 runtime tree。原 unknown/unavailable 保留，不新增资源或付费 provider 门。

完整 stdio 补充绑定核验了 2,400 个 compound read 的四个角色原载荷、4,800 status probes、3,594 native resource projections，并逐一核清所有 8,395 status responses，不重用、不丢弃。原 native statistics 双 replay 字节一致；各 operation/mutation 的全部分母、IID 限制与原 null CI 上界都保留，不声称稳定 p99 或性能加速。

## 原任务 evidence 映射

| 原任务 | 可追加的已复核证据 | 集成时仍需连接的原依赖 |
|---|---|---|
| P8-007 | 四档 mixed 各 900 次及独立 fake-backfill 768 次；真实排队/尾部/全部终态 | P8-006 与完整 V11/V20、相关回归 |
| P8-008 | 30 cold、400 reopen、400 warm uncached、400 cache-hit；完整分层/CI/原 replay | P8-007 与完整 V20、相关回归 |
| P8-009 | lifecycle 1,261 阶段账本及独立 soak 3,594 server/runner 观测；物理/逻辑/费用归属 | P8-008 与完整 V20、相关回归 |
| P8-010 | 原一小时 3,601 操作、RSS/coverage/cache/实际修改/15 表完整终态 | P8-009 与完整 V07/V17/V20、相关回归 |

冻结源码里的依赖状态只是历史记录；不能据此另加“重复已经成功的原测量”要求。父级可以在完整固定 source 证据确实闭环后共同推进依赖和任务。独审没有自行授予未审验证族、发行批准或新 P 的执行信用；原 native report 的 task_complete=false / release_approval=false 也没有被改写。

原官方 artifact 11594089439 的 ZIP 为 51,069,087 bytes，SHA256 d6debfa5e4a51bb59902cdb69a35ae7626a1e20b09bcaa5ab6cfda9ee3237086。完整原件在 central raw 保留。新 JSON 为 round5-p8-007-010-terminal-evidence.json，所有字段含原 raw/stdio/build/observer/statistics/receipt 的具体路径与 SHA256。
''')
assert sha(task_path)==task_sha
assert sha(root/pending)=='828e4468feaf6bc7d9eea1d35387e75ee37a09e357025e4c3cd8cf538a3367fc'
assert sha(root/pending_md)=='e95c4d74fcbce4b0fc69301d8cafbc57a8b3f4b147e419600cffb1447e9901c8'
print(json.dumps({'files':[ref(str(p.relative_to(root))) for p in [out,md]],'tasks_unchanged':True,'round4_pending_unchanged':True,'soak_original_review_complete':True,'baseline':doc['baseline']},ensure_ascii=False))
