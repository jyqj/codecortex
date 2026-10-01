#!/usr/bin/env python3
"""CAS record only accepted 016-018 tasks after the independent audit passes."""
from pathlib import Path
import collections,hashlib,json,re,subprocess,sys
R=Path.cwd();B=R/'artifacts/benchmarks/p5d-20260930-resume';name=sys.argv[1] if len(sys.argv)>1 else 'final-v2';O=B/name;D=R/'docs/roadmap/code-index-v2'
def load(p): return json.loads(p.read_text())
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def put(p,v): p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
A=load(O/'audit.json');P=load(O/'paired/summary.json');V=load(O/'validation.json');M=load(O/'source-manifest.json')
assert A['reviewer']=='acceptance_auditor' and A['status']=='passed_P5D_016_018_local_scope'
assert A['accepted_tasks']==['P5-016','P5-017','P5-018']
assert A['source_digest_sha256']==V['source_digest_sha256']==M['source_digest_sha256']
assert V['status']=='passed_P5D_016_018_local_scope' and V['source_unchanged']
# Independent audit remains immutable and reviewer-owned. Presentation counts
# come from its audited validation, never overwrite reviewer files.
A=dict(A,source_bytes=M['total_bytes'],archive_sha256=V['archive_sha256'])
commands={c['label']:c for c in V['commands']}
A['tests']={tc:{k:commands[tc+'-'+k]['tests'] for k in ['workspace','http','focused','protocol','real-mcp','watcher']} for tc in ['stable','1.95.0']}
import statistics
lifecycle=load(O/'observations/release-cost/p5d-idle-cost.json')
C={'scope':lifecycle['scope'],'groups':[{'pinned_views':pins,'samples':len(rows),'first_sweep_median_us':statistics.median(x['first_sweep_us'] for x in rows),'released_sweep_median_us':statistics.median(x['released_sweep_us'] for x in rows),'all_db_reclaimed':True} for pins in [0,4,8,16] for rows in [[x for x in lifecycle['samples'] if x['pinned_views']==pins]]],'prior_query_cost_samples':90,'admission_requests':192}

assert sha(D/'tasks.json')==sha(B/'pending-tasks.json'),'task state changed; reconcile'
T=load(D/'tasks.json');ids=['P5-016','P5-017','P5-018'];chosen=[t for t in T['tasks'] if t['id'] in ids]
assert len(chosen)==3 and all(t['status']=='in_progress' for t in chosen)
assert all(t['status']=='todo' for t in T['tasks'] if t['id'] in ['P5-019','P5-020'])
assert not (D/'P5-D-RUNTIME-GATE.json').exists() and not (D/'P5-D-RUNTIME-IMPLEMENTATION.md').exists()
rows=['| '+tc+' | '+' | '.join(str(values[k]['passed'])+' passed / '+str(values[k]['ignored'])+' ignored' for k in ['workspace','http','focused','protocol','real-mcp','watcher'])+' |' for tc,values in A['tests'].items()]
costs=['| '+str(v['pinned_views'])+' | '+str(v['samples'])+' | '+str(v['first_sweep_median_us'])+' | '+str(v['released_sweep_median_us'])+' |' for v in C['groups']]
report=(B/'report-template.md').read_text()
for key,value in {'HEAD':A['head'],'SOURCE':A['source_digest_sha256'],'FILES':A['source_files'],'BYTES':A['source_bytes'],'COMMANDS':A['command_count'],'TEST_ROWS':'\n'.join(rows),'COST_ROWS':'\n'.join(costs),'FULL_GATE':A['full_retrieval_gate'],'OUT':str(O.relative_to(R)),'ARCHIVE':A['archive_sha256']}.items():
 assert '__'+key+'__' in report,key
 report=report.replace('__'+key+'__',str(value))
assert not re.search(r'__[A-Z_]+__',report)
artifacts=[str((O/n).relative_to(R)) for n in ['validation.json','audit.json','source-review.json','additive-contract.json','paired/summary.json','lifecycle-cost-summary.json','source-manifest.json','source.tar.gz']]+['docs/roadmap/code-index-v2/P5-D-RUNTIME-GATE.json','docs/roadmap/code-index-v2/P5-D-RUNTIME-IMPLEMENTATION.md','docs/internals/QUERY_LIFECYCLE.md']
gate={'batch':'P5-D','accepted_tasks':ids,'pending_tasks':['P5-019','P5-020'],'status':'passed_declared_local_016_018_scope','G5':'not_run','M2':'incomplete','target_sha':A['head'],'covered_source_digest_sha256':A['source_digest_sha256'],'covered_files':A['source_files'],'toolchain_tests':A['tests'],'full_retrieval_gate':A['full_retrieval_gate'],'input_contract':'14 tools and existing properties preserved, two optional retrieval_strategy fields added','artifacts':artifacts,'limitations':A['limits'],'next_task':'P5-019','publication':'none'}
notes={'P5-016':'能力状态区分无项目/关闭/空库/可用/错误，dense明确disabled；复用既有诊断来源，不把fake端口或局部ready冒充完整覆盖。','P5-017':'查询视图租约、LRU弱登记、热路由快路径、worker-owned冷初始化与取消、非阻塞空闲清理、会话后台任务生命周期均接入；原生通知与同步工作仍不可强制抢占。','P5-018':'search/context的可选retrieval_strategy贯穿schema/sanitize/dispatch/handler/status/docs/真实stdio，显式context策略不被快捷路径绕过；保留14工具旧字段与mode语义。'}
if '--write' not in sys.argv:
 print('VERIFIED_ONLY: expected 118 done / 74 todo; P5-019 remains next');raise SystemExit(0)
assert not (O/'lifecycle-cost-summary.json').exists()
put(O/'lifecycle-cost-summary.json',C)
put(D/'P5-D-RUNTIME-GATE.json',gate);(D/'P5-D-RUNTIME-IMPLEMENTATION.md').write_text(report)
for t in chosen:
 t['status']='done';t['implementation_notes']=notes[t['id']]
 t.setdefault('evidence',[]).append({'target_sha':A['head'],'worktree_digest':A['source_digest_sha256'],'status':'passed_declared_local_016_018_scope','run_id':str(O.relative_to(R)),'artifacts':artifacts,'commands_receipt':str((O/'validation.json').relative_to(R)),'limitations':A['limits']})
T.update(status='p5d_runtime_contract_subset_complete_next_p5_019',next_task='P5-019',current_phase='P5',last_implementation_date='2026-09-30',execution_note='P5-016/017/018完成本地实现/契约/回归子集；118done/74todo，P5为18/20。P5-019/020、G5/M2及完整检索门禁未完成，原Partial与S11保留。未提交发布。')
assert collections.Counter(t['status'] for t in T['tasks'])=={'done':118,'todo':74}
put(D/'tasks.json',T);subprocess.run(['python3','scripts/code_index_plan.py','--write'],check=True)
print('TASKS_AND_PARTIAL_GATE_WRITTEN; update active summaries next')
