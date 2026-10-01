#!/usr/bin/env python3
"""Current source-v4 dual-toolchain engineering core; independent formal matrix required."""
from pathlib import Path
import os,sys,json,hashlib,subprocess,time,re,tarfile,shutil,collections
ROOT=Path.cwd();BASE=ROOT/'artifacts/benchmarks/p5e-g5-20261001'
OUT=BASE/(sys.argv[1] if len(sys.argv)>1 else 'core-v5');OUT.mkdir(exist_ok=False)
# Progress is observability, not a pipe-dependent test gate. Command output
# remains in its immutable per-command file; all results are atomically saved.
def progress(*values, **unused):
 with (OUT/'progress.log').open('a',encoding='utf8') as stream:
  stream.write(' '.join(str(value) for value in values)+'\n');stream.flush()
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
SOURCE=inventory();assert SOURCE==load(ROOT/'artifacts/benchmarks/p5e-candidate-20261001/final-source-v4/source-manifest.json')['files'],'candidate source membership/content drift';DIGEST=hashlib.sha256(json.dumps(SOURCE,sort_keys=True,separators=(',',':')).encode()).hexdigest();HEAD=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
write(OUT/'source-manifest.json',{'schema_version':1,'head':HEAD,'source_digest_sha256':DIGEST,'digest_method':'SHA256(canonical sorted-key JSON file rows)','file_count':len(SOURCE),'total_bytes':sum(r['bytes'] for r in SOURCE),'files':SOURCE,'scope':'implementation/config/tests/fixtures/internal docs/CI/scripts; excludes artifacts and mutable roadmap'})
with tarfile.open(OUT/'source.tar.gz','w:gz') as tar:
 for row in SOURCE:tar.add(ROOT/row['path'],arcname=row['path'],recursive=False)
DATA={'accepted_task_scope':[],'requested_task_scope':['P5-019','P5-020'],'scope':'current dual-toolchain engineering core; independent performance/quality matrix required separately','pending_tasks':{},'status':'running','head':HEAD,'source_digest_sha256':DIGEST,'source_file_count':len(SOURCE),'commands':[],'binaries':{},'compilers':{},'archive_sha256':sha(OUT/'source.tar.gz')}
def unchanged():
 assert inventory()==SOURCE,'covered source content/membership changed'
 assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==HEAD,'HEAD changed'
def save():write(OUT/'validation.json',DATA)
def run(label,argv,env=None,cwd=ROOT,allowed=(0,),tests=False):
 unchanged();p=OUT/(label+'.log');p.parent.mkdir(parents=True,exist_ok=True);e=(env or ENV).copy();started=time.monotonic();progress('RUN',label)
 with p.open('w') as log:
  try:code=subprocess.run([str(a) for a in argv],env=e,cwd=cwd,stdout=log,stderr=subprocess.STDOUT,timeout=1200).returncode
  except subprocess.TimeoutExpired:code=124
 text=p.read_text(errors='replace');matches=re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)
 counts={k:sum(int(row[i]) for row in matches) for i,k in enumerate(['passed','failed','ignored'])}
 row={'label':label,'argv':[str(a) for a in argv],'cwd':str(cwd.relative_to(ROOT)) if cwd!=ROOT else '.', 'environment':{k:e[k] for k in ['SDKROOT','CARGO_BUILD_JOBS','CARGO_TARGET_DIR','CODECORTEX_BENCH_BINARY','CODECORTEX_BENCH_OBSERVATIONS','CODECORTEX_CONTRACT_RECEIPT','RUST_LOG','CODECORTEX_BENCH_PROCESS_PROBE'] if k in e},'exit_code':code,'allowed_exit_codes':list(allowed),'seconds':time.monotonic()-started,'tests':counts,'log':str(p.relative_to(ROOT)),'log_sha256':sha(p)}
 DATA['commands'].append(row);save();unchanged();progress(json.dumps({'label':label,'exit_code':code,'tests':counts,'seconds':round(row['seconds'],3)}))
 if code not in allowed or (tests and (not matches or counts['passed']==0 or counts['failed'])):
  progress(text[-12000:]);raise RuntimeError('failed: '+label)
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
  cargo('focused',['test','-p','cc-eval','--test','p5d_runtime','--test','p5d_contract','--test','p5c_selection','--test','p5c_budget','--test','p5c_hydration','--test','p5b_execution','--test','p5a_boundaries','--test','p5e_identifier','--test','p5e_graph_anchor','--test','p5e_context_facets','--test','p5e_path_domain','--test','p5e_priority_pressure','--test','p5e_ablation','--test','benchmark_scoring','--locked','--offline'],True)
  cargo('protocol',['test','-p','cc-server','--lib','query_tests','--locked','--offline'],True)
  args=['test','-p','cc-eval','--features','eval-http']
  for t in MCP:args+=['--test',t]
  cargo('real-mcp',args+['--locked','--offline','--','--ignored','--nocapture'],True)
  cargo('watcher',['test','-p','cc-server','--lib','watcher','--locked','--offline'],True)
 env=ENV.copy();env.update(CARGO_TARGET_DIR=str(ROOT/'target/p0-dev'),CODECORTEX_BENCH_OBSERVATIONS=str(OUT/'observations/release-cost'))
 run('release-cost',['cargo','+stable','test','-p','cc-eval','--test','p5a_cost','--test','p5b_cost','--test','p5c_cost','--test','p5d_cost','--release','--locked','--offline','--','--ignored','--nocapture'],env,tests=True)
 runner=ROOT/DATA['binaries']['stable']['cc-eval']['path'];man=ROOT/'crates/cc-eval/benchmarks/manifests'
 args=[runner,'validate']
 for suite in [ROOT/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-smoke/suite.json',ROOT/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-codecortex-subset/suite.json']+[man/(name+'.json') for name in ['p1b-exact','p1c-intents','p1c-bm25','p1c-softscope']]:args+=['--suite',suite]
 run('corpus-locks',args)

 unchanged();DATA.update(status='passed_current_engineering_core_only_not_G5',source_unchanged=True,quality_performance_matrix='not_run_by_this_script',strict_inventory_gate='preserved_separate_known_not_passed')
except Exception as error:
 DATA['status']='failed';DATA['error']=str(error);save();raise
save();progress('FINAL',DATA['status'],len(SOURCE),DIGEST)
