#!/usr/bin/env python3
"""Frozen P5-D tasks 016-018, not G5 or whole-phase acceptance."""
from pathlib import Path
import os,sys,json,hashlib,subprocess,time,re,tarfile,shutil,collections
ROOT=Path.cwd();BASE=ROOT/'artifacts/benchmarks/p5d-20260930-resume'
OUT=BASE/(sys.argv[1] if len(sys.argv)>1 else 'final-v1');OUT.mkdir(exist_ok=False)
ENV=os.environ.copy();ENV.update(PATH=str(Path.home()/'.cargo/bin')+os.pathsep+ENV.get('PATH',''),SDKROOT='/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk',CARGO_BUILD_JOBS='4',RUST_LOG='error',CODECORTEX_BENCH_PROCESS_PROBE='0')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def load(p):return json.loads(p.read_text())
def write(p,data):
 p.parent.mkdir(parents=True,exist_ok=True);tmp=p.with_name(p.name+'.tmp');tmp.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n');tmp.replace(p)
def inventory():
 ps={r['path'] for r in load(BASE/'entry-source.json')['files']}
 for folder in ['crates','scripts','docs','.github']:
  for p in (ROOT/folder).rglob('*'):
   if p.is_file() and 'roadmap' not in p.parts and not any(x in p.parts for x in ['target','.codecortex','__pycache__','.git']) and p.suffix in ['.rs','.toml','.lock','.py','.sh','.md','.json','.jsonl','.sql','.yml','.yaml','.ts','.js','.txt','.html','.vue','.svelte','.go','.c','.cpp','.h']:ps.add(str(p.relative_to(ROOT)))
 return [{'path':p,'bytes':(ROOT/p).stat().st_size,'sha256':sha(ROOT/p)} for p in sorted(ps) if (ROOT/p).is_file()]
SOURCE=inventory();DIGEST=hashlib.sha256(json.dumps(SOURCE,sort_keys=True,separators=(',',':')).encode()).hexdigest();HEAD=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
write(OUT/'source-manifest.json',{'schema_version':1,'head':HEAD,'source_digest_sha256':DIGEST,'digest_method':'SHA256(canonical sorted-key JSON file rows)','file_count':len(SOURCE),'total_bytes':sum(r['bytes'] for r in SOURCE),'files':SOURCE,'scope':'implementation/config/tests/fixtures/internal docs/CI/scripts; excludes artifacts and mutable roadmap'})
with tarfile.open(OUT/'source.tar.gz','w:gz') as tar:
 for row in SOURCE:tar.add(ROOT/row['path'],arcname=row['path'],recursive=False)
DATA={'accepted_task_scope':['P5-016','P5-017','P5-018'],'pending_tasks':{},'status':'running','head':HEAD,'source_digest_sha256':DIGEST,'source_file_count':len(SOURCE),'commands':[],'binaries':{},'compilers':{},'archive_sha256':sha(OUT/'source.tar.gz')}
def unchanged():
 assert inventory()==SOURCE,'covered source content/membership changed'
 assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==HEAD,'HEAD changed'
def save():write(OUT/'validation.json',DATA)
def run(label,argv,env=None,cwd=ROOT,allowed=(0,),tests=False):
 unchanged();p=OUT/(label+'.log');p.parent.mkdir(parents=True,exist_ok=True);e=(env or ENV).copy();started=time.monotonic();print('RUN',label,flush=True)
 with p.open('w') as log:
  try:code=subprocess.run([str(a) for a in argv],env=e,cwd=cwd,stdout=log,stderr=subprocess.STDOUT,timeout=1200).returncode
  except subprocess.TimeoutExpired:code=124
 text=p.read_text(errors='replace');matches=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)
 counts={k:sum(int(row[i]) for row in matches) for i,k in enumerate(['passed','failed','ignored'])}
 row={'label':label,'argv':[str(a) for a in argv],'cwd':str(cwd.relative_to(ROOT)) if cwd!=ROOT else '.', 'environment':{k:e[k] for k in ['SDKROOT','CARGO_BUILD_JOBS','CARGO_TARGET_DIR','CODECORTEX_BENCH_BINARY','CODECORTEX_BENCH_OBSERVATIONS','CODECORTEX_CONTRACT_RECEIPT','RUST_LOG','CODECORTEX_BENCH_PROCESS_PROBE'] if k in e},'exit_code':code,'allowed_exit_codes':list(allowed),'seconds':time.monotonic()-started,'tests':counts,'log':str(p.relative_to(ROOT)),'log_sha256':sha(p)}
 DATA['commands'].append(row);save();unchanged();print(json.dumps({'label':label,'exit_code':code,'tests':counts,'seconds':round(row['seconds'],3)}),flush=True)
 if code not in allowed or (tests and (not matches or counts['passed']==0 or counts['failed'])):
  print(text[-12000:],flush=True);raise RuntimeError('failed: '+label)
 return code
MCP=['benchmark_adapters','p1a_retrieval','p1b_retrieval','p1c_contract','p1c_retrieval','p1d_concurrency','p1d_cost','p1d_docs','p2a_incremental','p2b_incremental','p2c_reconcile','p2d_mutations','p3a_project_model','p3b_modules','p3c_modules','p3d_modules','p4a_source','p4b_chunks','p4c_documents','p5a_retrieval','p5b_execution','p5c_budget','p5d_contract']
try:
 DATA['validation_scheduling']={'stable_workspace':'new current-source serial test-harness run; no earlier full run substituted','http_and_msrv_workspace':'serial test-harness execution to avoid timing-test contention; internal explicit concurrency tests unchanged','retained_failure':'artifacts/benchmarks/p5d-20260930-runtime/final-v2/stable-http.log','isolation_receipt':'artifacts/benchmarks/p5d-20260930-runtime/development/performance-isolation/receipt.json','performance_claim':'not tail latency or concurrent-load certification'}
 save();run('format',['cargo','+stable','fmt','--all','--','--check'])
 for domain in ['module','source']:run(domain+'-architecture',['python3','scripts/check_'+domain+'_architecture.py'])
 for tc,target in [('stable','p0-dev'),('1.95.0','p0-msrv-1.95')]:
  env=ENV.copy();env['CARGO_TARGET_DIR']=str(ROOT/'target'/target)
  DATA['compilers'][tc]=subprocess.check_output(['rustc','+'+tc,'--version','--verbose'],env=env,text=True);save()
  def cargo(label,args,tests=False):
   e=env.copy();obs=OUT/'observations'/(tc+'-'+label);obs.mkdir(parents=True,exist_ok=True)
   e['CODECORTEX_BENCH_OBSERVATIONS']=str(obs);e['CODECORTEX_CONTRACT_RECEIPT']=str(OUT/(tc+'-contract.json'))
   if tc in DATA['binaries']:e['CODECORTEX_BENCH_BINARY']=str(ROOT/DATA['binaries'][tc]['codecortex']['path'])
   return run(tc+'-'+label,['cargo','+'+tc]+args,e,tests=tests)
  cargo('strict',['clippy','--workspace','--all-targets','--features','cc-eval/eval-http','--locked','--offline','--','-D','warnings'])
  cargo('bins',['build','--workspace','--bins','--features','cc-eval/eval-http','--locked','--offline'])
  binary_dir=OUT/'binaries'/tc;binary_dir.mkdir(parents=True)
  for name in ['codecortex','cc-eval']:shutil.copy2(ROOT/'target'/target/'debug'/name,binary_dir/name)
  DATA['binaries'][tc]={name:{'path':str((binary_dir/name).relative_to(ROOT)),'sha256':sha(binary_dir/name),'immutable_attempt_copy':True} for name in ['codecortex','cc-eval']};save()
  cargo('workspace',['test','--workspace','--no-fail-fast','--locked','--offline'] + (['--','--test-threads=1']),True)
  cargo('http',['test','-p','cc-eval','--features','eval-http','--locked','--offline','--','--test-threads=1'],True)
  cargo('focused',['test','-p','cc-eval','--test','p5d_runtime','--test','p5d_contract','--test','p5c_selection','--test','p5c_budget','--test','p5c_hydration','--test','p5b_execution','--test','p5a_boundaries','--test','benchmark_scoring','--locked','--offline'],True)
  cargo('protocol',['test','-p','cc-server','--lib','query_tests','--locked','--offline'],True)
  args=['test','-p','cc-eval','--features','eval-http']
  for t in MCP:args+=['--test',t]
  cargo('real-mcp',args+['--locked','--offline','--','--ignored','--nocapture'],True)
  cargo('watcher',['test','-p','cc-server','--lib','watcher','--locked','--offline'],True)
 env=ENV.copy();env.update(CARGO_TARGET_DIR=str(ROOT/'target/p0-dev'),CODECORTEX_BENCH_OBSERVATIONS=str(OUT/'observations/release-cost'))
 run('release-cost',['cargo','+stable','test','-p','cc-eval','--test','p5a_cost','--test','p5b_cost','--test','p5c_cost','--test','p5d_cost','--release','--locked','--offline','--','--ignored','--nocapture'],env,tests=True)
 runner=ROOT/DATA['binaries']['stable']['cc-eval']['path'];man=ROOT/'crates/cc-eval/benchmarks/manifests'
 args=[runner,'validate']
 for name in ['p0-smoke','p0-codecortex-subset','p1b-exact','p1c-intents','p1c-bm25','p1c-softscope']:args+=['--suite',man/(name+'.json')]
 run('corpus-locks',args)
 oldroot=ROOT/'artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930'
 oldvalidation=load(oldroot/'validation.json');oldmanifest=load(oldroot/'source-manifest.json')
 archive=oldroot/'source.tar.gz';assert sha(archive)==oldvalidation['archive_sha256']
 oldrow=oldvalidation['binaries']['stable']['codecortex'];oldpath=ROOT/oldrow['path'];assert sha(oldpath)==oldrow['sha256']
 oldbinary=OUT/'binaries/p5b-codecortex';shutil.copy2(oldpath,oldbinary)
 DATA['baseline']={'archive':str(archive.relative_to(ROOT)),'archive_sha256':sha(archive),'source_digest_sha256':oldmanifest['source_digest_sha256'],'binary':str(oldbinary.relative_to(ROOT)),'binary_sha256':sha(oldbinary),'kind':'retained immutable P5-C full accepted executable; comparator tag p5b is legacy file naming only'};save()
 pair=OUT/'paired';pair.mkdir();results=[];deltas=[];gate_regressions=[];no_answer_regressions=[];row_maps={};budget_omissions=[];unexpected_transitions=[]
 bins={'p5b':oldbinary,'p5c':ROOT/DATA['binaries']['stable']['codecortex']['path']};hashes={k:sha(p) for k,p in bins.items()};runner_hash=sha(runner)
 frozen=ROOT/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs';suites=[('source',frozen/'p0-codecortex-subset/suite.json'),('smoke',frozen/'p0-smoke/suite.json'),('exact',man/'p1b-exact.json'),('intents',man/'p1c-intents.json')]
 for name,suite in suites:
  metrics={};gates={}
  for variant,binary in bins.items():
   dest=pair/(variant+'-'+name)
   code=run('paired/'+variant+'-'+name,[runner,'run','--backend','mcp-stdio','--binary',binary,'--suite',suite,'--output',dest,'--profile','smoke'],allowed=(0,1))
   replay_hashes={f:sha(dest/f) for f in ['metrics.json','query-slices.json','costs.jsonl','normalized.jsonl']}
   run('paired/replay-'+variant+'-'+name,[runner,'replay','--run',dest],allowed=(code,))
   assert replay_hashes=={f:sha(dest/f) for f in replay_hashes}
   m=load(dest/'metrics.json');metrics[variant]=m;g=load(dest/'gate.json');gates[variant]=g
   rows=[json.loads(line) for line in (dest/'normalized.jsonl').read_text().splitlines()]
   assert all(r['status'] in ['success','no_match','partial'] for r in rows),'unexpected protocol/timeout/runtime benchmark failure'
   row_maps[(name,variant)]={}; reasons=collections.Counter(); packing_counts=collections.Counter()
   for row in rows:
    raw=load(dest/row['raw_path'])
    row_maps[(name,variant)][(row['case_id'],row['repetition'])]=(row,raw)
    packing=raw.get('evidence_summary',{}).get('packing')
    if variant=='p5c' and raw.get('machine_pack',{}).get('kind')=='code_index_context':
     assert packing and packing['spec']=='whole-json-priority-evidence-before-references-v2'
     assert packing['used_bytes']<=packing['limit_bytes']<=raw['token_budget']*4
     assert packing['used_bytes']<=packing['configured_max_bytes']
     packing_counts['bounded_responses']+=1;packing_counts['partial']+=int(packing['partial'])
    retrieval=raw.get('evidence_summary',{}).get('retrieval',{})
    for lane in retrieval.get('lanes',retrieval.get('lane_receipts',[])):
     if lane.get('truncation_reason'):reasons[lane['truncation_reason']]+=1
   results.append({'dataset':name,'variant':variant,'questions':m['queries'],'requests':m['measured_rows'],'top1':m['mean_top1'],'ndcg10':m['mean_ndcg10'],'invalid_hits':m['invalid_hits'],'statuses':dict(collections.Counter(r['status'] for r in rows)),'lane_reasons':dict(reasons),'packing':dict(packing_counts),'gate':g,'replay_identical':True})
  for key,(newrow,newraw) in row_maps[(name,'p5c')].items():
   oldrow,_=row_maps[(name,'p5b')][key]
   if oldrow['status'] != newrow['status']:
    transition={'dataset':name,'case_id':key[0],'repetition':key[1],'before':oldrow['status'],'after':newrow['status']}
    if newrow['status']=='partial' and newraw.get('evidence_summary',{}).get('packing',{}).get('partial') is True:
     budget_omissions.append(transition)
    else:unexpected_transitions.append(transition)
  old={c['id']:c for c in metrics['p5b']['cases']};new={c['id']:c for c in metrics['p5c']['cases']};assert set(old)==set(new)
  for cid,c in new.items():deltas.append({'dataset':name,'id':cid,'top1_delta':c['top1']-old[cid]['top1'],'ndcg10_delta':c['ndcg10']-old[cid]['ndcg10']})
  added=set(gates['p5c']['reasons'])-set(gates['p5b']['reasons'])
  gate_regressions.extend({'dataset':name,'reason':x} for x in sorted(added))
 regressions=[r for r in deltas if r['top1_delta'] < -1e-12 or r['ndcg10_delta'] < -1e-12]
 summary={'status':'passed_ranking_source_and_bounded_output_scope' if not regressions and not unexpected_transitions else 'regression','source_digest_sha256':DIGEST,'questions':sum(r['questions'] for r in results if r['variant']=='p5c'),'requests':sum(r['requests'] for r in results),'rows':results,'case_deltas':deltas,'case_regressions':regressions,'new_gate_failures':gate_regressions,'new_budget_partial_requests':budget_omissions,'unexpected_status_transitions':unexpected_transitions,'full_retrieval_gate':'not_passed' if any(r['gate']['exit_code'] for r in results) else 'not_certified','baseline':DATA['baseline'],'binaries':hashes,'runner_sha256':runner_hash,'limitations':['51 authored development questions; not holdout','retained Partial and S11 are not whole retrieval success','no gold/scoring changes','not 100k/peak RSS/tail/cross-platform/provider certification']}
 write(pair/'summary.json',summary)
 assert not regressions and not unexpected_transitions and not any(r['invalid_hits'] for r in results),'paired ranking/source/runtime regression'
 assert not gate_regressions and not budget_omissions,'new completeness failure in runtime-only change'
 assert all(next(x for x in results if x['dataset']==r['dataset'] and x['variant']=='p5b')['statuses']==r['statuses'] for r in results if r['variant']=='p5c'),'status counts changed'
 assert all(any(t['dataset']==x['dataset'] and x['reason']==t['case_id']+' Partial' for t in budget_omissions) for x in gate_regressions),'unexplained completeness change'
 assert all(sha(p)==hashes[k] for k,p in bins.items()) and sha(runner)==runner_hash
 oldenv=ENV.copy();oldenv.update(CARGO_TARGET_DIR=str(ROOT/'target/p0-dev'),CODECORTEX_BENCH_BINARY=str(oldbinary),CODECORTEX_CONTRACT_RECEIPT=str(OUT/'baseline-contract.json'),CODECORTEX_BENCH_OBSERVATIONS=str(OUT/'observations/baseline-contract'))
 run('baseline-contract',['cargo','+stable','test','-p','cc-eval','--features','eval-http','--test','p1c_contract','--locked','--offline','--','--ignored','--nocapture'],oldenv,tests=True)
 assert load(OUT/'stable-contract.json')==load(OUT/'1.95.0-contract.json'),'toolchain MCP contract differs'
 old_contract={r['tool']:r for r in load(OUT/'baseline-contract.json')}
 new_contract={r['tool']:r for r in load(OUT/'stable-contract.json')}
 assert set(old_contract)==set(new_contract) and len(new_contract)==14
 additive=[]
 for name,row in new_contract.items():
  current=json.loads(json.dumps(row))
  if name in ['search','context']:
   schema=current['input_schema'];prop=schema['properties'].pop('retrieval_strategy')
   assert 'retrieval_strategy' not in schema.get('required',[])
   assert 'local' in prop.get('description','') and 'semantic' in prop.get('description','')
   additive.append({'tool':name,'optional_property':'retrieval_strategy','schema':prop})
  assert current==old_contract[name],name+' existing contract changed'
 write(OUT/'additive-contract.json',{'status':'passed','tools':14,'changes':additive,'all_old_properties_required_fields_and_modes_unchanged':True})
 cost=load(OUT/'observations/release-cost/p5b-execution-cost.json');assert cost['status']=='passed' and len(cost['rounds'])==3
 for row in cost['rounds']:assert row['accepted']==36 and row['rejected']==28 and row['peak_workers']<=4 and row['settled']['cpu_admitted']==0
 assert len(load(OUT/'observations/release-cost/p5a-cost.json')['samples'])==60
 final_cost=load(OUT/'observations/release-cost/p5c-final-evidence-cost.json')
 assert final_cost['status']=='passed' and len(final_cost['samples'])==30
 assert all(x['bytes']<=16000 for x in final_cost['samples'])
 lifecycle=load(OUT/'observations/release-cost/p5d-idle-cost.json')
 assert lifecycle['status']=='passed' and len(lifecycle['samples'])==12
 assert all(x['first_closed']==16-x['pinned_views'] and x['released_closed']==x['pinned_views'] and x['all_db_resources_reclaimed'] for x in lifecycle['samples'])
 for key in ['native_queries','historical_inputs']:
  for path,expected in load(BASE/'development/source-lock-refresh.json')[key].items():assert sha(ROOT/path)==expected,path
 for tc,values in DATA['binaries'].items():
  for name,row in values.items():assert sha(ROOT/row['path'])==row['sha256'],tc+'/'+name
 unchanged();DATA.update(status='passed_P5D_016_018_local_scope',source_unchanged=True,input_contract_backwards_compatible=True,paired_summary=str((pair/'summary.json').relative_to(ROOT)),full_retrieval_gate=summary['full_retrieval_gate'])
except Exception as error:
 DATA['status']='failed';DATA['error']=str(error);save();raise
save();print('FINAL',DATA['status'],len(SOURCE),DIGEST,flush=True)
