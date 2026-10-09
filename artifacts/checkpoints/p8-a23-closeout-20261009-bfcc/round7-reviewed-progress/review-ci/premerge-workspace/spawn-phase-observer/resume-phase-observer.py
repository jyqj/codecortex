from pathlib import Path
import datetime,hashlib,importlib.util,json,os,subprocess,sys,time
sys.dont_write_bytecode=True
ROOT=Path.cwd();BASE=ROOT/'review-ci/premerge-workspace';DEST=BASE/'spawn-phase-observer';PRIOR=BASE/'persistent-supervisor-observer'
SOURCE=ROOT/'source';DEPS=ROOT/'targets/premerge-workspace-a23-rust195-sdk154-round5/debug/deps'
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for c in iter(lambda:f.read(1024*1024),b''):h.update(c)
 return h.hexdigest()
def ent(p):return {'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p)}
def write(p,d):
 with p.open('x') as f:json.dump(d,f,ensure_ascii=False,sort_keys=True,indent=2);f.write('\n')
def native(args,name,cwd,env):
 st=now();clock=time.monotonic_ns()
 with (DEST/(name+'.stdout.log')).open('xb') as so,(DEST/(name+'.stderr.log')).open('xb') as se:
  child=subprocess.Popen(args,cwd=cwd,env=env,stdin=subprocess.DEVNULL,stdout=so,stderr=se)
  write(DEST/(name+'.start.json'),{'started_at':st,'argv':args,'cwd':str(cwd),'pid':child.pid})
  code=child.wait()
 r={'started_at':st,'finished_at':now(),'argv':args,'cwd':str(cwd),'elapsed_ns':time.monotonic_ns()-clock,'pid':child.pid,'actual_exit_code':code,'stdout':ent(DEST/(name+'.stdout.log')),'stderr':ent(DEST/(name+'.stderr.log'))}
 write(DEST/(name+'.exit.json'),r);return r
write(DEST/'authorization-resume-after-preflight.json',{'started_at':now(),'scope':'One separately authorized direct Command phase observation, preserving original helper bytes, child environment, stdio, process group, 250ms deadline and 2s elapsed limit. Prior failures preserved. No source or product changes.'})
spec=importlib.util.spec_from_file_location('fixed_source_capture',BASE/'attempt02-sdk154/run-original-workspace.py');cap=importlib.util.module_from_spec(spec);spec.loader.exec_module(cap)
before=cap.identity();assert before==json.loads((PRIOR/'source-after.json').read_text())['identity']
write(DEST/'source-before-resume.json',{'captured_at':now(),'identity':before})
tc=json.loads((PRIOR/'toolchain.json').read_text());env=os.environ.copy()
for k,v in tc['effective_selected_environment'].items():
 if v is None:env.pop(k,None)
 else:env[k]=v
env.update(tc['runtime_extra_env']);rustc=Path(env['RUSTC']);assert sha(rustc)==tc['rustc']['sha256']
helper=PRIOR/'stderr-holder.sh';literal=(SOURCE/'crates/cc-eval/tests/p8_scale.rs').read_text().splitlines()[487].strip().rstrip(',')
assert helper.read_bytes()==json.loads(literal).encode()
out=DEST/'run';assert out.is_dir()
assert (out/'plan.json').read_bytes()==(PRIOR/'run/plan.json').read_bytes()
tempfiles=list(DEPS.glob('libtempfile-*.rlib'))
ccfp=DEPS.parent/'.fingerprint/cc-eval-f0a4d576fe4e1690/lib-cc_eval.json'
expected=[x[3] for x in json.loads(ccfp.read_text())['deps'] if x[1]=='serde_json'];assert len(expected)==1
serdes=[]
for fp in (DEPS.parent/'.fingerprint').glob('serde_json-*/lib-serde_json.json'):
 if int.from_bytes(bytes.fromhex(fp.with_suffix('').read_text().strip()),'little')==expected[0]:
  artifact=DEPS/('libserde_json-'+fp.parent.name.split('-')[-1]+'.rlib')
  assert artifact.is_file()
  serdes.append(artifact)
  write(DEST/'serde-abi-selection.json',{'cc_eval_fingerprint':ent(ccfp),'expected_numeric_dependency_fingerprint':expected[0],'selected_fingerprint':ent(fp),'selected_fingerprint_value':fp.with_suffix('').read_text().strip(),'selected_rlib':ent(artifact),'reason':'Exact original cc_eval dependency fingerprint, decoded from Cargo fingerprint little-endian byte representation; excludes build profile variant.'})
assert len(tempfiles)==1 and len(serdes)==1
abi_paths=[Path(e['path']) for e in json.loads((PRIOR/'retained-abi-inputs-before.json').read_text())['files']]
abi_before=[ent(p) for p in abi_paths]
assert abi_before==json.loads((PRIOR/'retained-abi-inputs-before.json').read_text())['files']
write(DEST/'binding.json',{'source_sha':before['source_commit'],'source_tree':before['source_tree'],'rustc':ent(rustc),'prior_toolchain':ent(PRIOR/'toolchain.json'),'same_helper_bytes_and_path':ent(helper),'plan':ent(out/'plan.json'),'observer_source':ent(DEST/'phase-observer.rs'),'tempfile_rlib':ent(tempfiles[0]),'serde_json_rlib':ent(serdes[0]),'retained_abi_inventory':ent(PRIOR/'retained-abi-inputs-before.json'),'same_abi_before':True,'observation_differences':['Direct Command diagnostic, not the original run_supervised result.','Extra memory-only events and summary existence probes during the original 10ms polling cycle.','One extra try_wait at the original >=250ms deadline branch before identical owned-group kill/wait; no wait for eventual success.','Reuses exact persistent helper path and bytes; any change in OS launch state versus a newly written tempfile is not controlled.'],'does_not_change_sources_or_prior_results':True})
args=[str(rustc),'--edition=2021','--crate-name','a23_spawn_phase_observer',str(DEST/'phase-observer.rs'),'--extern','tempfile='+str(tempfiles[0]),'--extern','serde_json='+str(serdes[0]),'-L','dependency='+str(DEPS),'-o',str(DEST/'phase-observer')]
compiled=native(args,'compile',DEST,env)
if compiled['actual_exit_code']!=0:
 print(json.dumps({'stage':'compile','result':compiled,'stderr':(DEST/'compile.stderr.log').read_text()}));sys.exit(compiled['actual_exit_code'])
write(DEST/'observer-binary.json',ent(DEST/'phase-observer'))
run=native([str(DEST/'phase-observer'),str(helper),str(out)],'observer',SOURCE/'crates/cc-eval',env)
report=json.loads((out/'phase-report.json').read_text())
pid=report.get('worker_pid');cleanup={'captured_at':now(),'worker_pid':pid}
ps=subprocess.run(['/bin/ps','-axo','pid=,ppid=,pgid=,state=,command='],capture_output=True,text=True)
rows=[]
for line in ps.stdout.splitlines():
 cols=line.strip().split(None,4)
 if len(cols)>=5 and (cols[0]==str(pid) or cols[2]==str(pid) or str(helper) in cols[4]):rows.append(line)
try:os.killpg(pid,0);exists=True
except ProcessLookupError:exists=False
except PermissionError:exists='permission_denied'
cleanup.update({'ps_exit_code':ps.returncode,'owned_group_or_helper_rows':rows,'process_group_exists':exists,'no_remaining_owned_sleep_at_snapshot':exists is False and not rows,'no_diagnostic_cleanup_signal_sent':True})
write(DEST/'cleanup-observation.json',cleanup)
after=cap.identity();write(DEST/'source-after.json',{'captured_at':now(),'identity':after})
abi_after=[ent(p) for p in abi_paths]
r={'schema_version':1,'diagnostic_only':True,'compile':compiled,'observer':run,'source_unchanged':before==after,'abi_before_after_identical':abi_before==abi_after,'helper_unchanged':helper.read_bytes()==json.loads(literal).encode(),'phase_report':report,'worker_stderr':ent(out/'worker.stderr'),'worker_stderr_text':(out/'worker.stderr').read_text(errors='replace'),'summary_present':(out/'worker-summary.json').exists(),'cleanup':cleanup,'original_full_workspace_result':'failed_unmodified','original_negative_test_result':'failed_unmodified'}
write(DEST/'execution-receipt.json',r)
print(json.dumps(r),flush=True)
sys.exit(run['actual_exit_code'] if run['actual_exit_code']!=0 else 0 if before==after and abi_before==abi_after and cleanup['no_remaining_owned_sleep_at_snapshot'] else 86)
