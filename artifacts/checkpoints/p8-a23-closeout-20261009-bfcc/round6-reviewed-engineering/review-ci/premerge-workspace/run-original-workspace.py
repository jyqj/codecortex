from pathlib import Path
import datetime,hashlib,importlib.util,json,os,subprocess,sys,time,traceback
sys.dont_write_bytecode=True
HERE=Path(__file__).resolve().parent
ROOT=HERE.parent.parent
SOURCE=ROOT/'source'
A='a23bb72d3c954f385b99fe81ce9189885c208557'
TREE='58147c952505c44da1f41eb4b9c31643f2303b96'
TOOLCHAIN=Path('/Users/jin/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin')
TARGET=ROOT/'targets/premerge-workspace-a23-rust195-round5'
ARGV=[str(TOOLCHAIN/'cargo'),'test','--workspace']
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  for piece in iter(lambda:f.read(1048576),b''):h.update(piece)
 return h.hexdigest()
def write_new(path,doc):
 with path.open('x',encoding='utf-8') as f:json.dump(doc,f,ensure_ascii=False,sort_keys=True,indent=2);f.write('\n')
def captured(argv,cwd=None,env=None):
 start=now();p=subprocess.run(argv,cwd=cwd,env=env,text=True,capture_output=True)
 return {'argv':argv,'started_at':start,'finished_at':now(),'exit_code':p.returncode,'stdout':p.stdout,'stderr':p.stderr}
sys.path.insert(0,str(SOURCE/'scripts'))
from p7_build_identity import source_snapshot
def identity():
 head=captured(['git','rev-parse','HEAD'],SOURCE);tree=captured(['git','rev-parse','HEAD^{tree}'],SOURCE)
 status=captured(['git','status','--porcelain=v1','--untracked-files=all'],SOURCE)
 assert head['exit_code']==tree['exit_code']==status['exit_code']==0
 validation=json.loads((ROOT/'review-next-candidate/retained-a23-validation-inputs.json').read_text())
 observed={path:sha(SOURCE/path) for path in validation}
 docs={path:sha(SOURCE/path) for path in ['CONTRIBUTING.md','docs/TEST_PLAN.md','docs/BENCHMARK.md']}
 return {'source_commit':head['stdout'].strip(),'source_tree':tree['stdout'].strip(),'git_status_porcelain_v1':status['stdout'],'crate_cargo_source_snapshot':source_snapshot(SOURCE),'validation_sha256':observed,'validation_manifest_matched':observed==validation,'policy_document_sha256':docs}
def main():
 assert Path.cwd().resolve()==ROOT
 assert not TARGET.exists(),'Use a new target; never reuse another validation target'
 assert not (HERE/'execution-start.json').exists(),'Never redispatch the same original command'
 env=os.environ.copy()
 inherited_selected={k:env.get(k) for k in ['PATH','SDKROOT','RUSTUP_TOOLCHAIN','RUSTC','RUSTDOC','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','CARGO_BUILD_TARGET','CARGO_BUILD_JOBS','CARGO_TARGET_DIR','RUST_TEST_THREADS','CARGO_NET_OFFLINE','CODECORTEX_WRITE_REAL_BENCHMARK','CODECORTEX_WRITE_BENCHMARK','CODECORTEX_BENCH_BINARY','CODECORTEX_BENCH_OBSERVATIONS']}
 env['PATH']=str(TOOLCHAIN)+os.pathsep+str(Path.home()/'.cargo/bin')+os.pathsep+env.get('PATH','')
 env['RUSTUP_TOOLCHAIN']='1.95.0'
 env['RUSTC']=str(TOOLCHAIN/'rustc')
 env['RUSTDOC']=str(TOOLCHAIN/'rustdoc')
 env['CARGO_TARGET_DIR']=str(TARGET)
 env['CARGO_BUILD_JOBS']='4'
 for k in ['CODECORTEX_WRITE_REAL_BENCHMARK','CODECORTEX_WRITE_BENCHMARK']:
  env.pop(k,None)
 selected_env={k:env.get(k) for k in inherited_selected}
 versions={}
 for name in ['cargo','rustc','rustdoc']:
  binary=TOOLCHAIN/name
  p=captured([str(binary),'--version'],SOURCE,env)
  assert p['exit_code']==0 and p['stdout'].startswith(name+' 1.95.0'),p
  versions[name]={'path':str(binary),'sha256':sha(binary),'version':p}
 write_new(HERE/'toolchain.json',{'schema_version':1,'captured_at':now(),'toolchain':'1.95.0-aarch64-apple-darwin','executables':versions,'inherited_selected_environment':inherited_selected,'effective_selected_environment':selected_env,'environment_scope':'Only this original engineering command and its descendants; no global settings changed. CARGO_BUILD_JOBS limits compilation jobs, not test threads. SDKROOT retains the observed parent value.'})
 before=identity()
 assert before['source_commit']==A and before['source_tree']==TREE and before['git_status_porcelain_v1']=='' and before['validation_manifest_matched']
 write_new(HERE/'source-before.json',{'captured_at':now(),'identity':before})
 TARGET.mkdir(parents=True)
 command={'schema_version':1,'source_sha':A,'source_tree':TREE,'argv':ARGV,'documented_command':'cargo test --workspace','cwd':str(SOURCE),'cargo_target_dir':str(TARGET),'cargo_build_jobs':4,'toolchain_file':'toolchain.json','source_before_file':'source-before.json','stdout_file':'stdout.log','stderr_file':'stderr.log','ignored_tests_explicitly_enabled':False,'original_ignored_benchmark_write_flags_removed':['CODECORTEX_WRITE_REAL_BENCHMARK','CODECORTEX_WRITE_BENCHMARK'],'purpose':'Fill the documented original engineering workspace test command and doctest evidence gap. No scale/soak/TODO/release credit.','allowed_scope':'Read-only fixed a23 source; writes limited to isolated target, Cargo dependency cache needed for this build, system temporary test directories, and this evidence directory.'}
 write_new(HERE/'command.json',command)
 wall_start=now();mono=time.monotonic_ns()
 with (HERE/'stdout.log').open('xb') as stdout,(HERE/'stderr.log').open('xb') as stderr:
  proc=subprocess.Popen(ARGV,cwd=SOURCE,env=env,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr)
  write_new(HERE/'execution-start.json',{'started_at':wall_start,'monotonic_start_ns':mono,'wrapper_pid':os.getpid(),'cargo_pid':proc.pid,'argv':ARGV,'cwd':str(SOURCE),'target_dir':str(TARGET),'status':'running','terminal_exit_code':None})
  print(json.dumps({'status':'original_cargo_workspace_started','cargo_pid':proc.pid,'argv':ARGV,'cwd':str(SOURCE),'target_dir':str(TARGET),'started_at':wall_start}),flush=True)
  while True:
   try:
    returncode=proc.wait(timeout=30)
    break
   except subprocess.TimeoutExpired:
    print(json.dumps({'status':'running','cargo_pid':proc.pid,'observed_at':now(),'elapsed_seconds':round((time.monotonic_ns()-mono)/1e9,3),'stdout_bytes':(HERE/'stdout.log').stat().st_size,'stderr_bytes':(HERE/'stderr.log').stat().st_size}),flush=True)
 elapsed=time.monotonic_ns()-mono
 finished=now()
 write_new(HERE/'native-exit.json',{'started_at':wall_start,'finished_at':finished,'elapsed_ns':elapsed,'actual_cargo_exit_code':returncode,'cargo_pid':proc.pid,'actual_argv':ARGV,'cwd':str(SOURCE),'target_dir':str(TARGET),'recorded_before_postrun_identity_verification':True})
 after=identity()
 write_new(HERE/'source-after.json',{'captured_at':now(),'identity':after})
 stable=before==after
 receipt={'schema_version':1,'source_sha':A,'source_tree':TREE,'actual_argv':ARGV,'cwd':str(SOURCE),'target_dir':str(TARGET),'started_at':wall_start,'finished_at':finished,'elapsed_ns':elapsed,'actual_cargo_exit_code':returncode,'source_before_after_identical':stable,'source_after_clean':after['git_status_porcelain_v1']=='','source_before':{'path':'source-before.json','bytes':(HERE/'source-before.json').stat().st_size,'sha256':sha(HERE/'source-before.json')},'source_after':{'path':'source-after.json','bytes':(HERE/'source-after.json').stat().st_size,'sha256':sha(HERE/'source-after.json')},'complete_stdout':{'path':'stdout.log','bytes':(HERE/'stdout.log').stat().st_size,'sha256':sha(HERE/'stdout.log')},'complete_stderr':{'path':'stderr.log','bytes':(HERE/'stderr.log').stat().st_size,'sha256':sha(HERE/'stderr.log')},'toolchain_receipt':{'path':'toolchain.json','sha256':sha(HERE/'toolchain.json')},'native_ignored_benchmarks_not_enabled':True,'status':'native_command_success_source_unchanged' if returncode==0 and stable else 'native_command_failure_or_source_identity_issue','does_not_certify_original_scale_or_todos':True,'task_ledger_changes':0}
 write_new(HERE/'execution-receipt.json',receipt)
 print(json.dumps({'status':receipt['status'],'actual_cargo_exit_code':returncode,'source_before_after_identical':stable,'elapsed_seconds':elapsed/1e9,'receipt':str(HERE/'execution-receipt.json')}),flush=True)
 if returncode==0 and not stable:return 86
 return returncode if 0<=returncode<=255 else 1
if __name__=='__main__':
 try:
  code=main()
 except BaseException as error:
  if isinstance(error,SystemExit):raise
  failure={'captured_at':now(),'wrapper_error_type':type(error).__name__,'wrapper_error':str(error),'traceback':traceback.format_exc(),'actual_native_exit_code':'See execution-receipt.json if present; wrapper exception is not a fabricated Cargo exit code.'}
  p=HERE/'wrapper-error.json'
  if not p.exists():write_new(p,failure)
  print(json.dumps(failure),flush=True)
  raise
 sys.exit(code)
