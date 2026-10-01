#!/usr/bin/env python3
"""Independent audit of P5-D 016-018 evidence. Never certifies G5."""
from pathlib import Path
import collections, hashlib, json, re, statistics, subprocess, sys, tarfile
R=Path.cwd(); B=R/'artifacts/benchmarks/p5d-20260930-resume'; O=B/(sys.argv[1] if len(sys.argv)>1 else 'final-v1')
def load(p): return json.loads(p.read_text())
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def put(p,x): p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
V=load(O/'validation.json'); M=load(O/'source-manifest.json'); P=load(O/'paired/summary.json')
assert V['status']=='passed_P5D_016_018_local_scope' and V['source_unchanged']
assert V['source_digest_sha256']==M['source_digest_sha256']==P['source_digest_sha256']
assert hashlib.sha256(json.dumps(M['files'],sort_keys=True,separators=(',',':')).encode()).hexdigest()==M['source_digest_sha256']
paths={r['path'] for r in load(B/'entry-source.json')['files']}
for directory in ['crates','scripts','docs','.github']:
 for p in (R/directory).rglob('*'):
  if p.is_file() and 'roadmap' not in p.parts and not any(x in p.parts for x in ['target','.codecortex','__pycache__','.git']) and p.suffix in ['.rs','.toml','.lock','.py','.sh','.md','.json','.jsonl','.sql','.yml','.yaml','.ts','.js','.txt','.html','.vue','.svelte','.go','.c','.cpp','.h']:
   paths.add(str(p.relative_to(R)))
assert {p for p in paths if (R/p).is_file()}=={r['path'] for r in M['files']}
for row in M['files']:
 p=R/row['path']; assert p.is_file() and sha(p)==row['sha256'] and p.stat().st_size==row['bytes'],row['path']
assert sha(O/'source.tar.gz')==V['archive_sha256']
with tarfile.open(O/'source.tar.gz','r:gz') as t:
 assert all(x.isfile() for x in t.getmembers())
 assert {x.name for x in t.getmembers()}=={x['path'] for x in M['files']}
 for row in M['files']: assert hashlib.sha256(t.extractfile(row['path']).read()).hexdigest()==row['sha256']
assert not any('reused_from' in c for c in V['commands'])
for c in V['commands']:
 assert c['environment'].get('CODECORTEX_BENCH_PROCESS_PROBE')=='0',c['label']+' missing explicit probe policy'
 assert sha(R/c['log'])==c['log_sha256'],c['label']
 assert c['exit_code'] in c['allowed_exit_codes'],c['label']
 if 'test' in c['argv']: assert c['tests']['passed']>0 and c['tests']['failed']==0 and c['exit_code']==0,c['label']
for binaries in V['binaries'].values():
 for binary in binaries.values(): assert binary['immutable_attempt_copy'] and sha(R/binary['path'])==binary['sha256']
assert sha(R/V['baseline']['binary'])==V['baseline']['binary_sha256']
assert V['input_contract_backwards_compatible']
old={r['tool']:r for r in load(O/'baseline-contract.json')}; new={r['tool']:r for r in load(O/'stable-contract.json')}
assert load(O/'stable-contract.json')==load(O/'1.95.0-contract.json') and len(old)==len(new)==14
for name,row in new.items():
 candidate=json.loads(json.dumps(row))
 if name in ['search','context']:
  schema=candidate['input_schema']; field=schema['properties'].pop('retrieval_strategy')
  assert 'retrieval_strategy' not in schema.get('required',[]) and 'local' in field['description'] and 'semantic' in field['description']
 assert candidate==old[name],name
assert load(O/'additive-contract.json')['all_old_properties_required_fields_and_modes_unchanged']
assert P['questions']==51 and P['requests']==306 and not P['case_regressions']
assert not P['new_gate_failures'] and not P['new_budget_partial_requests'] and not P['unexpected_status_transitions']
assert all(r['invalid_hits']==0 and r['replay_identical'] for r in P['rows'])
for r in P['rows']:
 if r['variant']=='p5c':
  previous=next(x for x in P['rows'] if x['dataset']==r['dataset'] and x['variant']=='p5b')
  assert previous['statuses']==r['statuses'] and previous['lane_reasons']==r['lane_reasons']
for dataset in ['source','smoke','exact','intents']:
 for variant in ['p5b','p5c']:
  resources=[json.loads(line) for line in (O/'paired'/(variant+'-'+dataset)/'resources.jsonl').read_text().splitlines()]
  assert resources
  for row in resources:
   assert 'explicitly disabled' in row['method']
   assert row['runner_rss_bytes'] is None and row['server_rss_bytes'] is None and row['server_tree_rss_bytes'] is None
for key in ['native_queries','historical_inputs']:
 for path,digest in load(B/'development/source-lock-refresh.json')[key].items(): assert sha(R/path)==digest,path
lifecycle=load(O/'observations/release-cost/p5d-idle-cost.json')
assert lifecycle['status']=='passed' and len(lifecycle['samples'])==12
for x in lifecycle['samples']:
 assert x['first_closed']==16-x['pinned_views'] and x['released_closed']==x['pinned_views'] and x['all_db_resources_reclaimed']
assert len(load(O/'observations/release-cost/p5a-cost.json')['samples'])==60
assert len(load(O/'observations/release-cost/p5c-final-evidence-cost.json')['samples'])==30
rounds=load(O/'observations/release-cost/p5b-execution-cost.json')['rounds']
assert len(rounds)==3 and all(x['accepted']==36 and x['rejected']==28 and x['peak_workers']<=4 and x['settled']['cpu_admitted']==0 for x in rounds)
for tc in ['stable','1.95.0']:
 observation=load(O/f'observations/{tc}-focused/p5d-lifecycle.json')
 assert observation['same_runtime_after_lru'] and observation['idle_skips_pin'] and observation['db_reclaimed_after_release'] and observation['fake_calls']==1
 policy=load(O/f'observations/{tc}-real-mcp/p5d-public-policy.json')
 assert policy['capabilities']['retrieval']['dense_state']=='disabled' and policy['semantic_unavailable_tested'] and policy['legacy_modes_preserved']
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==V['head']
subprocess.run(['git','diff','--check'],check=True)
commands={x['label']:x for x in V['commands']}
tests={tc:{name:commands[tc+'-'+name]['tests'] for name in ['workspace','http','focused','protocol','real-mcp','watcher']} for tc in ['stable','1.95.0']}
costs=[]
for pins in [0,4,8,16]:
 rows=[x for x in lifecycle['samples'] if x['pinned_views']==pins]
 costs.append({'pinned_views':pins,'samples':len(rows),'first_sweep_median_us':statistics.median(x['first_sweep_us'] for x in rows),'released_sweep_median_us':statistics.median(x['released_sweep_us'] for x in rows),'all_db_reclaimed':True})
limits=['P5-016/017/018 local implementation only; P5-019 and P5-020/G5/M2 remain incomplete','existing source/intent Partial and S11 failure retained','no real provider, vector publication, 100k, cross-platform, sustained-load or tail-latency certification','native watcher init/teardown is cooperative; 20s startup watchdog is not a latency acceptance bound','cold initialization serialized per session and worker-owned until publication; open cache hits bypass cold registry; reopen occurs under lifecycle write lock in blocking worker, never a network wait under a query guard','all samples retained, no best-of selection; optional ps process-tree RSS explicitly disabled and null, native current-process RSS separate','no commit, push, PR, merge or daily-index mutation']
summary={'status':'passed_P5D_016_018_local_scope','head':V['head'],'source_digest_sha256':M['source_digest_sha256'],'source_files':M['file_count'],'source_bytes':M['total_bytes'],'archive_sha256':V['archive_sha256'],'command_count':len(V['commands']),'source_archive_logs_binaries_equal':True,'source_drift':0,'tests':tests,'queries':51,'paired_requests':306,'ranking_regressions':0,'invalid_hits':0,'new_completeness_failures':0,'old_tool_contracts_preserved':14,'new_optional_fields':['search.retrieval_strategy','context.retrieval_strategy'],'full_retrieval_gate':P['full_retrieval_gate'],'lifecycle_samples':12,'limits':limits}
put(O/'audit.json',summary);put(O/'lifecycle-cost-summary.json',{'scope':lifecycle['scope'],'groups':costs,'prior_query_cost_samples':90,'admission_requests':192})
print(json.dumps(summary,ensure_ascii=False,indent=2))
