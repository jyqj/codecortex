from pathlib import Path
import datetime,hashlib,importlib.util,json,os,subprocess,sys,time
sys.dont_write_bytecode=True
ROOT=Path.cwd();BASE=ROOT/'review-ci/premerge-workspace';DEST=BASE/'fresh-path-phase-observer';PRIOR=BASE/'spawn-phase-observer';PERSIST=BASE/'persistent-supervisor-observer';SOURCE=ROOT/'source'
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for c in iter(lambda:f.read(1024*1024),b''):h.update(c)
 return h.hexdigest()
def ent(p):return {'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p)}
def write(p,d):
 with p.open('x') as f:json.dump(d,f,ensure_ascii=False,sort_keys=True,indent=2);f.write('\n')
write(DEST/'authorization-used.json',{'started_at':now(),'scope':'Exactly one new-path same-byte helper observation using previously compiled same phase observer. Original limits 250ms/2s; all failed original tests and reused-path reading retained.'})
spec=importlib.util.spec_from_file_location('fixed_source_capture',BASE/'attempt02-sdk154/run-original-workspace.py');cap=importlib.util.module_from_spec(spec);spec.loader.exec_module(cap)
before=cap.identity();assert before==json.loads((PRIOR/'source-after.json').read_text())['identity']
write(DEST/'source-before.json',{'captured_at':now(),'identity':before})
tc=json.loads((PERSIST/'toolchain.json').read_text());env=os.environ.copy()
for k,v in tc['effective_selected_environment'].items():
 if v is None:env.pop(k,None)
 else:env[k]=v
env.update(tc['runtime_extra_env'])
binary=PRIOR/'phase-observer';binary_before=ent(binary);assert binary_before==json.loads((PRIOR/'observer-binary.json').read_text())
abi_paths=[Path(e['path']) for e in json.loads((PERSIST/'retained-abi-inputs-before.json').read_text())['files']]
abi_before=[ent(p) for p in abi_paths];assert abi_before==json.loads((PERSIST/'retained-abi-inputs-before.json').read_text())['files']
literal=(SOURCE/'crates/cc-eval/tests/p8_scale.rs').read_text().splitlines()[487].strip().rstrip(',')
helper_bytes=json.loads(literal).encode();assert len(helper_bytes)==159 and helper_bytes==(PERSIST/'stderr-holder.sh').read_bytes()
out=DEST/'run';out.mkdir()
with (out/'plan.json').open('xb') as f:f.write((PERSIST/'run/plan.json').read_bytes())
helper=DEST/'fresh-stderr-holder.sh';assert not helper.exists()
created=now();creation_clock=time.monotonic_ns()
with helper.open('xb') as f:f.write(helper_bytes)
helper.chmod(0o700)
args=[str(binary),str(helper),str(out)]
started=now();clock=time.monotonic_ns()
with (DEST/'observer.stdout.log').open('xb') as so,(DEST/'observer.stderr.log').open('xb') as se:
 child=subprocess.Popen(args,cwd=SOURCE/'crates/cc-eval',env=env,stdin=subprocess.DEVNULL,stdout=so,stderr=se)
 launch_elapsed=time.monotonic_ns()-creation_clock
 write(DEST/'observer.start.json',{'started_at':started,'argv':args,'cwd':str(SOURCE/'crates/cc-eval'),'pid':child.pid,'fresh_helper_created_at':created,'creation_to_observer_popen_return_ns':launch_elapsed})
 code=child.wait()
native={'started_at':started,'finished_at':now(),'elapsed_ns':time.monotonic_ns()-clock,'actual_exit_code':code,'pid':child.pid,'argv':args,'cwd':str(SOURCE/'crates/cc-eval'),'stdout':ent(DEST/'observer.stdout.log'),'stderr':ent(DEST/'observer.stderr.log')}
write(DEST/'observer.exit.json',native)
report=json.loads((out/'phase-report.json').read_text());pid=report.get('worker_pid')
cleanup={'captured_at':now(),'worker_pid':pid}
ps=subprocess.run(['/bin/ps','-axo','pid=,ppid=,pgid=,state=,command='],capture_output=True,text=True);rows=[]
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
binding={'source_sha':before['source_commit'],'source_tree':before['source_tree'],'toolchain':ent(PERSIST/'toolchain.json'),'same_observer_binary':binary_before,'same_observer_source':ent(PRIOR/'phase-observer.rs'),'abi_inventory':ent(PERSIST/'retained-abi-inputs-before.json'),'fresh_helper':ent(helper),'fresh_helper_created_at':created,'creation_to_observer_popen_return_ns':launch_elapsed,'exact_original_helper_literal_line':488,'original_helper_bytes':159,'same_helper_bytes_as_reused_path':helper.read_bytes()==helper_bytes,'different_helper_path':str(helper),'previous_reused_helper_path':str(PERSIST/'stderr-holder.sh'),'compile_performed':False,'original_250ms_2s_limits_preserved':True,'observation_scope':'Same compiled direct Command diagnostic as reused-path observation, not original test or run_supervised execution; sequential runs do not control all host/OS launch state.'}
write(DEST/'binding.json',binding)
r={'schema_version':1,'diagnostic_only':True,'observer':native,'source_unchanged':before==after,'abi_before_after_identical':abi_before==abi_after,'observer_binary_unchanged':ent(binary)==binary_before,'helper_unchanged':helper.read_bytes()==helper_bytes,'phase_report':report,'worker_stderr':ent(out/'worker.stderr'),'worker_stderr_text':(out/'worker.stderr').read_text(errors='replace'),'summary_present':(out/'worker-summary.json').exists(),'cleanup':cleanup,'original_full_workspace_result':'failed_unmodified','original_negative_test_result':'failed_unmodified'}
write(DEST/'execution-receipt.json',r)
print(json.dumps(r),flush=True)
sys.exit(code if code!=0 else 0 if before==after and abi_before==abi_after and cleanup['no_remaining_owned_sleep_at_snapshot'] else 86)
