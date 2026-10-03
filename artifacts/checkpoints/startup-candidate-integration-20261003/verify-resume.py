import os, subprocess, json, pathlib, hashlib, time
root=pathlib.Path('/workspace/codecortex-startup'); out=root/'artifacts/checkpoints/startup-candidate-integration-20261003'; out.mkdir(exist_ok=True)
tc='/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin'; env=os.environ.copy(); env.update(PATH=tc+':'+env['PATH'], CARGO_HOME='/workspace/.cargo', CARGO_TARGET_DIR='/workspace/codecortex/target', RUSTC=tc+'/rustc', RUSTDOC=tc+'/rustdoc', TMPDIR='/tmp'); receipts=json.loads((out/"receipts.json").read_text()); identities=json.loads((out/"binaries.json").read_text())
source=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
def sha(p): return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def run(name,args):
 print('START',name,flush=True); start=time.time()
 with (out/(name+'.log')).open('w') as f: p=subprocess.run(args,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT)
 r=dict(name=name,argv=args,exit_code=p.returncode,seconds=round(time.time()-start,2),log=name+'.log',source_commit=source); receipts.append(r); (out/'receipts.json').write_text(json.dumps(receipts,indent=2)+'\n'); print('END',name,p.returncode,flush=True)
 if p.returncode: raise SystemExit(p.returncode)
 return out/(name+'.log')
def build(name,args):
 log=run(name,[tc+'/cargo',*args,'--locked','--offline','--message-format=json'])
 for line in log.read_text().splitlines():
  try: a=json.loads(line)
  except ValueError: continue
  if a.get('reason')=='compiler-artifact' and a.get('executable'):
   identities.append(dict(build=name,target=a['target'],package_id=a['package_id'],features=a['features'],profile=a['profile'],executable=a['executable'],sha256=sha(a['executable']),source_commit=source))
 (out/'binaries.json').write_text(json.dumps(identities,indent=2)+'\n')
run('check-semantic-http-all-targets',[tc+'/cargo','check','--workspace','--all-targets','--features','cc-server/semantic-http,cc-eval/semantic','--locked','--offline'])
build('compile-semantic-http',['test','-p','cc-server','-p','cc-eval','-p','cc-db','-p','cc-semantic','--lib','--test','mcp_startup','--test','p5d_runtime','--test','p5d_cost','--test','p7_query_production','--test','p7_v18_fault_contract','--test','bounded_parallel','--test','queue_worker','--test','semantic_outbox','--no-run','--features','cc-server/semantic-http,cc-eval/semantic'])
run('clippy-semantic-http',[tc+'/cargo-clippy','clippy','--workspace','--all-targets','--features','cc-server/semantic-http,cc-eval/semantic','--locked','--offline','--','-D','warnings'])
import shutil
snap=pathlib.Path('/workspace/startup-candidate-binaries/semantic-http/codecortex');snap.parent.mkdir(parents=True,exist_ok=True);shutil.copy2('/workspace/codecortex/target/debug/codecortex',snap)
# All requested compilation and strict Clippy complete before any regression execution.
import tempfile
cache=tempfile.mkdtemp(prefix='startup-candidate-cache-',dir='/tmp'); env['CODECORTEX_SEMANTIC_CACHE_ROOT']=cache
(out/'cache-ownership.json').write_text(json.dumps({'owner':'this verification process','path':cache,'fixtures':'isolated startup children and stdio children also own subpaths'},indent=2)+'\n')
def binary(buildname,name,kind=None):
 matches=[a for a in identities if a['build']==buildname and a['target']['name']==name and (kind is None or kind in a['target']['kind'])]
 assert len(matches)==1,(name,len(matches)); return matches[0]['executable']
server=binary('compile-semantic-http','cc_server','lib')
run('startup-isolated',[server,'--exact','project_session::startup_tests::isolated_startup_regressions','--nocapture'])
for label in ['default','semantic-http']:
 shutil.copy2('/workspace/startup-candidate-binaries/'+label+'/codecortex','/workspace/codecortex/target/debug/codecortex')
 run('startup-stdio-'+label,[binary('compile-'+label,'mcp_startup','test'),'--nocapture'])
for name in ['shared_provider_gate_cases_in_fresh_processes','recoverable_retry_drains_other_ready_documents','recoverable_retry_width_zero_drains_other_ready_documents','bounded_parallel_pins_running_and_factory_survive_until_physical_join']:
 prefix='semantic_runtime::shared_provider_gate_tests::' if name.startswith('shared_provider') else 'semantic_runtime::tests::'
 run(name,[server,'--exact',prefix+name,'--nocapture'])
for name in ['bounded_parallel','queue_worker']:
 run(name,[binary('compile-semantic-http',name,'test'),'--nocapture'])
run('fifo',[binary('compile-semantic-http','semantic_outbox','test'),'--nocapture'])
for name in ['direct_rebuild_canonical_schema_matches_normal_temp_path','direct_rebuild_callback_and_index_errors_leave_live_database_unchanged']:
 run(name,[binary('compile-semantic-http','cc_db','lib'),'--exact','index_db::tests::'+name,'--nocapture'])
run('fmt',[tc+'/cargo-fmt','fmt','--all','--','--check'])
run('diff-check',['git','diff','--check'])
(out/'rust-sources.json').write_text(json.dumps({str(p.relative_to(root)):sha(p) for p in sorted((root/'crates').rglob('*.rs'))},indent=2)+'\n')
print('ALL REQUESTED CHECKS COMPLETE',flush=True)
