"""Execute original Express schedules once per fixed arm with original cc-eval."""
import hashlib,json,os,subprocess,sys
from pathlib import Path
from datetime import datetime,timezone
D=Path(__file__).resolve().parent.parent
sha=lambda b:hashlib.sha256(b).hexdigest()
plan=json.loads((D/'plan.json').read_bytes())
commands=[]
env=os.environ.copy();env['CODECORTEX_BENCH_PROCESS_PROBE']='0'
for arm in ['baseline','candidate']:
 root=Path('/workspace/express-runtime')/arm
 build_path=D/('build-'+arm+'02/build-receipt.json');build=json.loads(build_path.read_bytes())
 assert build['source_sha']==plan[arm] and build['build_exit_code']==0
 for n,a in build['artifacts'].items():
  assert sha(Path(a['copied_binary']).read_bytes())==a['binary_sha256']
  assert not any(f in a['features'] for f in ['semantic','eval-http'])
 for n,h in plan['input_file_sha256'].items():assert sha((root/'inputs'/n).read_bytes())==h
 binary=build['artifacts']['cc-eval']['copied_binary'];product=build['artifacts']['codecortex']['copied_binary']
 for e in plan['suite_entries']:
  # Exact suite bytes and relative queries/source stay untouched; arm-owned root only.
  suite=root/'inputs'/Path(e['suite_path']).relative_to('/tmp/group-js-current-inputs')
  assert sha(suite.read_bytes())==e['suite_sha256']
  dest=root/'full'/e['key'];dest.parent.mkdir(parents=True,exist_ok=True)
  assert not dest.exists()
  validate=[binary,'validate','--suite',str(suite)]
  v=subprocess.run(validate,env=env,capture_output=True)
  log=D/'execution'/arm;log.mkdir(parents=True,exist_ok=True)
  for ext,b in [('stdout',v.stdout),('stderr',v.stderr)]: (log/(e['key']+'.validate.'+ext)).write_bytes(b)
  assert v.returncode==0,'validation failure; stop arm before retrieval'
  command=[binary,'run','--backend','mcp-stdio','--binary',product,'--suite',str(suite),'--output',str(dest),'--profile','quality']
  started=datetime.now(timezone.utc).isoformat();proc=subprocess.run(command,env=env,capture_output=True)
  for ext,b in [('stdout',proc.stdout),('stderr',proc.stderr)]: (log/(e['key']+'.run.'+ext)).write_bytes(b)
  before={p.relative_to(dest).as_posix():sha(p.read_bytes()) for p in dest.rglob('*') if p.is_file()}
  replay_command=[binary,'replay','--run',str(dest)];replay=subprocess.run(replay_command,env=env,capture_output=True)
  for ext,b in [('stdout',replay.stdout),('stderr',replay.stderr)]: (log/(e['key']+'.replay.'+ext)).write_bytes(b)
  after={p.relative_to(dest).as_posix():sha(p.read_bytes()) for p in dest.rglob('*') if p.is_file()}
  changed=sorted(p for p in set(before)|set(after) if before.get(p)!=after.get(p))
  rec={'arm':arm,'source_sha':plan[arm],'build_receipt_sha256':sha(build_path.read_bytes()),'binary_sha256':{n:a['binary_sha256'] for n,a in build['artifacts'].items()},
    'key':e['key'],'profile':e['profile'],'output':str(dest),'suite_sha256':e['suite_sha256'],'query_sha256':e['query_sha256'],
    'validate_command':validate,'validate_exit':v.returncode,'command':command,'run_exit_code':proc.returncode,'replay_command':replay_command,'replay_exit_code':replay.returncode,
    'started_utc':started,'finished_utc':datetime.now(timezone.utc).isoformat(),'before_replay_sha256':before,'after_replay_sha256':after,'changed_on_replay':changed}
  commands.append(rec);(D/'commands.json').write_text(json.dumps(commands,indent=2)+'\n')
  print(json.dumps({k:rec[k] for k in ['arm','key','run_exit_code','replay_exit_code','changed_on_replay']}),flush=True)
  # Failure is retained, not retried. The original runner itself refuses queries until prepare/readiness succeeded.
 for n,h in plan['input_file_sha256'].items():assert sha((root/'inputs'/n).read_bytes())==h
 for n,a in build['artifacts'].items():assert sha(Path(a['copied_binary']).read_bytes())==a['binary_sha256']
