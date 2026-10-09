from pathlib import Path
import datetime,hashlib,importlib.util,json,os,subprocess,sys,time
sys.dont_write_bytecode=True
ROOT=Path.cwd();BASE=ROOT/'review-ci/premerge-workspace';DEST=BASE/'isolated-negative-once'
spec=importlib.util.spec_from_file_location('premerge_original_capture',BASE/'attempt02-sdk154/run-original-workspace.py')
capture=importlib.util.module_from_spec(spec);spec.loader.exec_module(capture)
source=ROOT/'source';binary=ROOT/'targets/premerge-workspace-a23-rust195-sdk154-round5/debug/deps/p8_scale-a2f51b686c6f3d87'
args=[str(binary),'subprocess_descendant_cannot_hold_stderr_past_worker_deadline','--exact','--nocapture']
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,d):
 with p.open('x') as f:json.dump(d,f,ensure_ascii=False,sort_keys=True,indent=2);f.write('\n')
before=capture.identity()
previous=json.loads((BASE/'attempt02-sdk154/source-after.json').read_text())['identity']
assert before==previous and before['source_commit']=='a23bb72d3c954f385b99fe81ce9189885c208557' and before['git_status_porcelain_v1']==''
binary_digest=sha(binary);assert binary_digest=='d6cc286b3d031bd796b41a29c82669b51fa0011e1ce8f4d9fe86943859196ad7'
assert not (DEST/'execution-start.json').exists(),'Single authorized isolated diagnosis; never redispatch'
env=os.environ.copy()
effective=json.loads((BASE/'attempt02-sdk154/toolchain.json').read_text())['effective_selected_environment']
for key,value in effective.items():
 if value is None:env.pop(key,None)
 else:env[key]=value
env['CARGO_MANIFEST_DIR']=str(source/'crates/cc-eval')
env['CARGO_MANIFEST_PATH']=str(source/'crates/cc-eval/Cargo.toml')
assert env['RUSTUP_TOOLCHAIN']=='1.95.0' and env['SDKROOT']=='/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk'
write(DEST/'source-before.json',{'captured_at':now(),'identity':before})
write(DEST/'command.json',{'schema_version':1,'source_sha':before['source_commit'],'source_tree':before['source_tree'],'actual_argv':args,'cwd':str(source/'crates/cc-eval'),'binary_sha256':binary_digest,'binary_bytes':binary.stat().st_size,'reused_compiled_target':effective['CARGO_TARGET_DIR'],'compile_performed':False,'selected_original_test_count':1,'unchanged_original_limits':{'deadline_ms':250,'total_elapsed_upper_seconds':2,'expected_report_exit_code':0,'expected_stderr_complete':True},'effective_selected_environment':effective,'package_runtime_environment':{'CARGO_MANIFEST_DIR':env['CARGO_MANIFEST_DIR'],'CARGO_MANIFEST_PATH':env['CARGO_MANIFEST_PATH']},'scope':'Single authorized isolated diagnosis of the unchanged original negative test. Neither an isolated pass nor a failure rewrites the original full workspace failure; no complete workspace/scale/TODO/release credit.'})
started=now();clock=time.monotonic_ns()
with (DEST/'stdout.log').open('xb') as so,(DEST/'stderr.log').open('xb') as se:
 child=subprocess.Popen(args,cwd=source/'crates/cc-eval',env=env,stdin=subprocess.DEVNULL,stdout=so,stderr=se)
 write(DEST/'execution-start.json',{'started_at':started,'pid':child.pid,'actual_argv':args,'status':'running'})
 code=child.wait()
elapsed=time.monotonic_ns()-clock;finished=now()
write(DEST/'native-exit.json',{'actual_exit_code':code,'started_at':started,'finished_at':finished,'elapsed_ns':elapsed,'pid':child.pid,'recorded_before_postrun_identity_verification':True})
after=capture.identity();stable=after==before;binary_stable=sha(binary)==binary_digest
write(DEST/'source-after.json',{'captured_at':now(),'identity':after})
receipt={'schema_version':1,'status':'isolated_original_test_passed_diagnostic_only' if code==0 and stable and binary_stable else 'isolated_original_test_failed_or_identity_changed','source_sha':before['source_commit'],'actual_argv':args,'cwd':str(source/'crates/cc-eval'),'actual_exit_code':code,'started_at':started,'finished_at':finished,'elapsed_ns':elapsed,'source_unchanged':stable,'binary_unchanged':binary_stable,'binary_sha256':binary_digest,'stdout':{'bytes':(DEST/'stdout.log').stat().st_size,'sha256':sha(DEST/'stdout.log')},'stderr':{'bytes':(DEST/'stderr.log').stat().st_size,'sha256':sha(DEST/'stderr.log')},'original_workspace_result':'failure_exit_101_retained','full_workspace_gate':False,'flaky_defect_proven_fixed':False,'new_original_primary_measurement_credit':False}
write(DEST/'execution-receipt.json',receipt)
print(json.dumps(receipt),flush=True)
print((DEST/'stdout.log').read_text(),flush=True)
print((DEST/'stderr.log').read_text(),file=sys.stderr,flush=True)
sys.exit(code if code!=0 else 0 if stable and binary_stable else 86)
