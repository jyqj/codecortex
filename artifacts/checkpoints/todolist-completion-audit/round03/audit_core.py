#!/usr/bin/env python3
"""Independent 21-command current engineering core; never accepts G5 tasks."""
from pathlib import Path
import json,hashlib,tarfile,re,subprocess,datetime,sys
R=Path(__file__).resolve().parents[4];O=R/'artifacts/benchmarks/p5e-g5-20261001'/sys.argv[1]
def load(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def need(c,w):
 if not c:raise AssertionError(w)
V=load(O/'validation.json');M=load(O/'source-manifest.json');C=load(R/'artifacts/benchmarks/p5e-candidate-20261001'/(sys.argv[2] if len(sys.argv)>2 else 'final-source-v1')/'source-manifest.json');review=load(O/'source-review.json')
need(V['status']=='passed_current_engineering_core_only_not_G5','core not terminal passed')
need(V['accepted_task_scope']==[] and V['requested_task_scope']==['P5-019','P5-020'],'scope overstated')
need(V['source_unchanged'] and M['files']==C['files'],'candidate closure mismatch')
need(M['source_digest_sha256']==V['source_digest_sha256']==review['source_digest_sha256']==C['source_digest_sha256'],'source digest mismatch')
need(hashlib.sha256(json.dumps(M['files'],sort_keys=True,separators=(',',':')).encode()).hexdigest()==M['source_digest_sha256'],'canonical digest mismatch')
for x in M['files']:need((R/x['path']).is_file() and (R/x['path']).stat().st_size==x['bytes'] and sha(R/x['path'])==x['sha256'],'source drift '+x['path'])
need(sha(O/'source.tar.gz')==V['archive_sha256'],'archive hash mismatch')
with tarfile.open(O/'source.tar.gz') as a:
 need(all(x.isfile() for x in a.getmembers()),'archive non-file')
 need({x.name for x in a.getmembers()}=={x['path'] for x in M['files']},'archive membership mismatch')
 for x in M['files']:need(hashlib.sha256(a.extractfile(x['path']).read()).hexdigest()==x['sha256'],'archive byte mismatch')
need(subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==V['head'],'HEAD drift')
expected={'format','module-architecture','source-architecture','release-cost','corpus-locks'}
for tc in ['stable','1.95.0']:expected.update(tc+'-'+x for x in ['strict','bins','workspace','http','focused','protocol','real-mcp','watcher'])
commands={x['label']:x for x in V['commands']};need(set(commands)==expected and len(V['commands'])==21,'core command matrix mismatch')
for x in V['commands']:
 need('reused_from' not in x,'old receipt substitution')
 need(x['exit_code']==0 and sha(R/x['log'])==x['log_sha256'],'command/log failure '+x['label'])
 need(x['environment'].get('CODECORTEX_BENCH_PROCESS_PROBE')=='0','core probe disable absent')
 if 'test' in x['argv']:
  log=(R/x['log']).read_text(errors='replace');rows=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',log);counts={k:sum(int(a[i]) for a in rows) for i,k in enumerate(['passed','failed','ignored'])};need(counts==x['tests'] and counts['passed']>0 and counts['failed']==0,'test receipt mismatch '+x['label'])
for tc,bs in V['binaries'].items():
 for name,x in bs.items():need(x['immutable_attempt_copy'] and sha(R/x['path'])==x['sha256'],'binary mutation '+tc+'/'+name)
new=load(O/'stable-contract.json');need(new==load(O/'1.95.0-contract.json'),'toolchain tool contract disagreement')
old=load(R/'artifacts/benchmarks/p5d-20260930-resume/final-v3/stable-contract.json');need(new==old and len(new)==14,'14-tool backward contract drift')
locked=commands['corpus-locks']['argv'];need(str(R/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-smoke/suite.json') in locked and str(R/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-codecortex-subset/suite.json') in locked,'original frozen inputs substituted')
for x in load(R/'artifacts/checkpoints/todolist-completion-audit/round03/task-obligations-v2.json')['locked_inputs']:
 need(sha(R/x['suite_path'])==x['suite_sha256'] and sha(R/x['query_path'])==x['query_sha256'],'gold/suite drift')
 for f in x['files']:need(sha(R/x['source_root']/f['path'])==f['sha256'],'source input drift')
for tc in ['stable','1.95.0']:
 p=load(O/f'observations/{tc}-real-mcp/p5d-public-policy.json');need(p['capabilities']['retrieval']['dense_state']=='disabled' and p['semantic_unavailable_tested'] and p['legacy_modes_preserved'],'MCP false ready/compatibility')
 life=load(O/f'observations/{tc}-focused/p5d-lifecycle.json');need(all(life[k] for k in ['same_runtime_after_lru','idle_skips_pin','db_reclaimed_after_release']) and life['fake_calls']==1,'query lease/LRU resources fail')
rounds=load(O/'observations/release-cost/p5b-execution-cost.json')['rounds'];need(len(rounds)==3 and all(x['accepted']==36 and x['rejected']==28 and x['peak_workers']<=4 and x['settled']['cpu_admitted']==0 for x in rounds),'bounded workers fail')
life=load(O/'observations/release-cost/p5d-idle-cost.json');need(life['status']=='passed' and len(life['samples'])==12 and all(x['first_closed']==16-x['pinned_views'] and x['released_closed']==x['pinned_views'] and x['all_db_resources_reclaimed'] for x in life['samples']),'release resource lifecycle failure')
packing=load(O/'observations/release-cost/p5c-final-evidence-cost.json');need(packing['status']=='passed' and len(packing['samples'])==30 and all(x['bytes']<=16000 for x in packing['samples']),'source budget cost failure')
need(len(load(O/'observations/release-cost/p5a-cost.json')['samples'])==60,'lane sample omission')
summary={'schema_version':1,'reviewer':'acceptance_auditor','status':'passed_independent_current_engineering_core_only_not_G5','observed_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'head':V['head'],'source_digest_sha256':M['source_digest_sha256'],'source_files':M['file_count'],'source_bytes':M['total_bytes'],'archive_sha256':V['archive_sha256'],'command_count':21,'source_archive_logs_binaries_equal':True,'tool_contracts_preserved':14,'tests':{tc:{g:commands[tc+'-'+g]['tests'] for g in ['workspace','http','focused','protocol','real-mcp','watcher']} for tc in ['stable','1.95.0']},'accepted_tasks':[],'pending_required_tasks':['P5-019','P5-020'],'limitations':['No formal quality/performance/ablation/graph/110span/mixed resource CI matrix results in this core.','Current dual-toolchain receipts actually rechecked; no older full tests substituted.','Original fixed gold/suites/source hashes retained; known strict inventory gate and real NL/packing misses not washed green.','Reviewer verified actual logs/source/closure and negative test coverage; did not independently rerun the entire Cargo workload.','Source snapshot is explicit dirty checkout, not new commit or release/cross-platform/paid-provider/all-todolist certification.']}
p=O/'audit.json';need(not p.exists(),'refuse audit overwrite');p.write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n');print(json.dumps(summary,ensure_ascii=False,indent=2))
