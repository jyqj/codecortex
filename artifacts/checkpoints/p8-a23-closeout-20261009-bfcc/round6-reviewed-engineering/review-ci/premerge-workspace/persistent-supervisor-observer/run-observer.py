from pathlib import Path
import datetime,hashlib,importlib.util,json,os,subprocess,sys,time
sys.dont_write_bytecode=True
ROOT=Path.cwd(); BASE=ROOT/'review-ci/premerge-workspace'; DEST=BASE/'persistent-supervisor-observer'
SOURCE=ROOT/'source'; TARGET=ROOT/'targets/premerge-workspace-a23-rust195-sdk154-round5'; DEPS=TARGET/'debug/deps'
def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  while True:
   chunk=f.read(1024*1024)
   if not chunk: break
   h.update(chunk)
 return h.hexdigest()
def entry(path): return {'path':str(path),'bytes':path.stat().st_size,'sha256':sha(path)}
def write(path,value):
 with path.open('x') as f: json.dump(value,f,ensure_ascii=False,sort_keys=True,indent=2);f.write('\n')
def native(argv,name,cwd,env):
 started=now();t=time.monotonic_ns()
 with (DEST/(name+'.stdout.log')).open('xb') as out,(DEST/(name+'.stderr.log')).open('xb') as err:
  child=subprocess.Popen(argv,cwd=cwd,env=env,stdin=subprocess.DEVNULL,stdout=out,stderr=err)
  write(DEST/(name+'.start.json'),{'started_at':started,'pid':child.pid,'argv':argv,'cwd':str(cwd)})
  code=child.wait()
 receipt={'argv':argv,'cwd':str(cwd),'started_at':started,'finished_at':now(),'elapsed_ns':time.monotonic_ns()-t,'pid':child.pid,'actual_exit_code':code,'stdout':entry(DEST/(name+'.stdout.log')),'stderr':entry(DEST/(name+'.stderr.log'))}
 write(DEST/(name+'.exit.json'),receipt)
 return receipt
write(DEST/'diagnostic-authorization-used.json',{'started_at':now(),'scope':'One authorized persistent original run_supervised observation; no original test/workspace retry. Existing full-workspace and exact isolated failures remain failures. #167 externally merged at 04:16:47 before the original test failure; this is subsequent investigation of the same product on main.','a23':'a23bb72d3c954f385b99fe81ce9189885c208557','main_merge_commit_reported_by_parent':'b21cce4c8661589267ad5719f850accbec088d2f'})
spec=importlib.util.spec_from_file_location('original_premerge_capture',BASE/'attempt02-sdk154/run-original-workspace.py')
capture=importlib.util.module_from_spec(spec);spec.loader.exec_module(capture)
before=capture.identity()
assert before==json.loads((BASE/'attempt02-sdk154/source-after.json').read_text())['identity']
assert before['source_commit']=='a23bb72d3c954f385b99fe81ce9189885c208557' and before['git_status_porcelain_v1']==''
write(DEST/'source-before.json',{'captured_at':now(),'identity':before})
effective=json.loads((BASE/'attempt02-sdk154/toolchain.json').read_text())['effective_selected_environment']
env=os.environ.copy()
for key,value in effective.items():
 if value is None: env.pop(key,None)
 else:env[key]=value
env['CARGO_MANIFEST_DIR']=str(SOURCE/'crates/cc-eval')
env['CARGO_MANIFEST_PATH']=str(SOURCE/'crates/cc-eval/Cargo.toml')
rustc=Path(effective['RUSTC'])
assert sha(rustc)=='b829b733131d4e1673eeebd1f34d06ae1e9ff4977b051313cf42e2a9e79ecf1c'
rlibs=list(DEPS.glob('libcc_eval-*.rlib'));assert len(rlibs)==1
rlib=rlibs[0];assert sha(rlib)=='9852a0ccdfda09d483bb443b5a218453219ab4a334195a70379d52105fc75f44'
abi_paths=sorted(p for p in DEPS.iterdir() if p.suffix in ('.rlib','.rmeta','.dylib','.a','.so'))
abi_before=[entry(p) for p in abi_paths]
write(DEST/'retained-abi-inputs-before.json',{'schema_version':1,'cc_eval_rlib':entry(rlib),'files':abi_before,'scope':'All retained target dependency ABI candidates; rustc metadata resolves exact transitives. No dependencies rebuilt.'})
test_source=(SOURCE/'crates/cc-eval/tests/p8_scale.rs').read_text()
lines=test_source.splitlines()
literal=lines[487].strip().rstrip(',')
helper_bytes=json.loads(literal).encode()
assert helper_bytes.startswith(b'#!/bin/sh\n') and b'sleep 20 &\n' in helper_bytes
helper=DEST/'stderr-holder.sh'
with helper.open('xb') as f:f.write(helper_bytes)
helper.chmod(0o700)
toolchain={'rustc':entry(rustc),'version':subprocess.check_output([str(rustc),'-vV'],env=env,text=True),'sysroot':subprocess.check_output([str(rustc),'--print','sysroot'],env=env,text=True).strip(),'effective_selected_environment':effective,'runtime_extra_env':{'CARGO_MANIFEST_DIR':env['CARGO_MANIFEST_DIR'],'CARGO_MANIFEST_PATH':env['CARGO_MANIFEST_PATH']},'native_interpreter':entry(Path('/bin/sh')),'sleep_executable':entry(Path('/bin/sleep'))}
write(DEST/'toolchain.json',toolchain)
write(DEST/'binding.json',{'schema_version':1,'source_sha':before['source_commit'],'source_tree':before['source_tree'],'original_test_path':'crates/cc-eval/tests/p8_scale.rs','original_test_source':entry(SOURCE/'crates/cc-eval/tests/p8_scale.rs'),'exact_helper_literal_line':488,'exact_helper_bytes':entry(helper),'original_supervisor_source':entry(SOURCE/'crates/cc-eval/src/benchmark/p8_scale.rs'),'retained_cc_eval_rlib':entry(rlib),'retained_original_test_binary':entry(DEPS/'p8_scale-a2f51b686c6f3d87'),'observer_source':entry(DEST/'observer.rs'),'original_limits':{'deadline_ms':250,'total_elapsed_upper_seconds':2,'expected_exit_code':0,'stderr_complete':True,'release_certification':'not_run'},'fresh_process_group':'Unmodified retained run_supervised uses process_group(0); ChildGuard signals only -worker_pid with SIGKILL, then kills/waits own child.','limitations':['Persistent helper/output directory differs from tempfile-owned original test, so result is diagnostic only.','No worker.stderr or native summary survives from original TempDir test failures; this observer provides newly retained diagnostics only.','No original test or product/source altered.'],'prior_failures_unchanged':['attempt02-sdk154 actual cargo exit101','isolated-negative-once actual original test exit101']})
compile_argv=[str(rustc),'--edition=2021','--crate-name','a23_supervisor_observer',str(DEST/'observer.rs'),'--extern','cc_eval='+str(rlib),'-L','dependency='+str(DEPS),'-o',str(DEST/'observer')]
compiled=native(compile_argv,'compile',DEST,env)
if compiled['actual_exit_code']!=0:
 print(json.dumps({'stage':'compile','receipt':compiled,'stderr':(DEST/'compile.stderr.log').read_text()}),flush=True);sys.exit(compiled['actual_exit_code'])
write(DEST/'observer-binary.json',entry(DEST/'observer'))
executed=native([str(DEST/'observer'),str(helper),str(DEST/'run')],'observer',SOURCE/'crates/cc-eval',env)
raw_report_path=DEST/'run/report.json'
raw_report=json.loads(raw_report_path.read_text()) if raw_report_path.is_file() else None
cleanup={'captured_at':now(),'worker_pid':raw_report.get('worker_pid') if raw_report else None,'method':'Immediate after-call killpg(worker_pid,0) observation plus native ps PID/PPID/PGID snapshot. No diagnostic signal sent.'}
ps=subprocess.run(['/bin/ps','-axo','pid=,ppid=,pgid=,state=,command='],capture_output=True,text=True)
pid=cleanup['worker_pid']
selected=[]
for line in ps.stdout.splitlines():
 cols=line.strip().split(None,4)
 if len(cols)>=5 and ((pid is not None and (cols[0]==str(pid) or cols[2]==str(pid))) or str(helper) in cols[4]): selected.append(line)
cleanup['ps_exit_code']=ps.returncode;cleanup['owned_group_or_helper_rows']=selected
if pid is not None:
 try:os.killpg(pid,0);cleanup['process_group_exists']=True
 except ProcessLookupError:cleanup['process_group_exists']=False
 except PermissionError:cleanup['process_group_exists']='permission_denied'
cleanup['no_remaining_owned_sleep_proven_at_snapshot']=bool(pid is not None and cleanup.get('process_group_exists') is False and not selected)
write(DEST/'cleanup-observation.json',cleanup)
after=capture.identity()
write(DEST/'source-after.json',{'captured_at':now(),'identity':after})
abi_after=[entry(p) for p in abi_paths]
write(DEST/'retained-abi-inputs-after.json',{'schema_version':1,'files':abi_after,'identical_to_before':abi_after==abi_before})
preserved=[entry(p) for p in sorted((DEST/'run').glob('*')) if p.is_file()] if (DEST/'run').is_dir() else []
receipt={'schema_version':1,'status':'diagnostic_only','source_sha':before['source_commit'],'source_unchanged':before==after,'retained_abi_unchanged':abi_before==abi_after,'helper_unchanged':helper.read_bytes()==helper_bytes,'compile':compiled,'observer':executed,'raw_report':raw_report,'preserved_original_output_files':preserved,'worker_summary_present':(DEST/'run/worker-summary.json').is_file(),'worker_stderr_text':(DEST/'run/worker.stderr').read_text(errors='replace') if (DEST/'run/worker.stderr').is_file() else None,'cleanup':cleanup,'original_full_workspace_pass':False,'original_isolated_test_pass':False,'TODO_credit':False}
write(DEST/'execution-receipt.json',receipt)
print(json.dumps(receipt,ensure_ascii=False),flush=True)
print((DEST/'observer.stdout.log').read_text(),flush=True)
print((DEST/'observer.stderr.log').read_text(),file=sys.stderr,flush=True)
sys.exit(executed['actual_exit_code'] if executed['actual_exit_code']!=0 else 0 if before==after and abi_before==abi_after and cleanup['no_remaining_owned_sleep_proven_at_snapshot'] else 86)
