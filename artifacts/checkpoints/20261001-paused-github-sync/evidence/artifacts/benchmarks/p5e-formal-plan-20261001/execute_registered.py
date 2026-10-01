#!/usr/bin/env python3
"""One immutable registered sequence; stage exits are observations, not G5 verdicts."""
import argparse,hashlib,json,os,pathlib,subprocess,time
p=argparse.ArgumentParser();p.add_argument('--approval',type=pathlib.Path,required=True);a=p.parse_args()
root=pathlib.Path.cwd();profile_path=root/'artifacts/benchmarks/p5e-g5-profile/final-v1/profile.json';expected='0d7caef9979dcb95a86ed7ae32c75d51fb9bb42f00c56f45753d45940319c8c7'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
assert sha(profile_path)==expected
profile=json.load(open(profile_path));approval=json.load(open(a.approval))
assert approval.get('profile_sha256')==expected and approval.get('decision')=='approved_to_execute','require explicit independent matching approval receipt'
plan_path=root/'artifacts/benchmarks/p5e-formal-plan-20261001/FINAL-RUN-PLAN-v2.json';assert sha(plan_path)=='300bdd4d3428dbfb5c8febe9b2773a1772a1a737c212e18102c17d07575f7426';plan=json.load(open(plan_path));assert plan['sequence']==profile['execution_sequence']
manifest=json.load(open(root/'artifacts/benchmarks/p5e-harness-20261001/final-v5/FINAL-HARNESS-INPUT-MANIFEST.json'))
def identities():
 for entry in manifest['harness_files']+manifest['factorial_preparation']+[manifest[k] for k in ['candidate_producer','candidate_binary','baseline_producer','baseline_binary']]:
  f=root/entry['path'];assert f.stat().st_size==entry['bytes'] and sha(f)==entry['sha256'],str(f)
 for entry in profile['locked_inputs']:
  f=root/entry['path'];assert sha(f)==entry['sha256'],str(f)
 for entry in plan['binaries'].values():assert sha(root/entry['path'])==entry['sha256'],entry['path']
identities();out=root/'artifacts/benchmarks/p5e-formal-runs-20261001-v1';assert not out.exists();out.mkdir();logs=out/'stage-logs';logs.mkdir();env=os.environ.copy();env.update(plan['process_env']);receipts=[]
def validate_cells():
 base=out/'factorial-build';cells=json.load(open(base/'plan.json'));reference=json.load(open(base/'reference-source.json'));checks=[]
 for cell in cells['variants']:
  source=base/cell['source_root'];receipt=json.load(open(base/cell['build_receipt']));assert receipt['exit_code']==0
  actual={p.relative_to(source).as_posix():sha(p) for p in source.rglob('*') if p.is_file()};assert actual==receipt['source_files'] and set(actual)==set(reference)
  expected={k:(base/'reference'/k).read_bytes() for k in reference}
  for control in cells['controls']:
   content=expected[control['path']].decode();assert content.count(control['on_text'])==1
   if control['id'] not in cell['enabled']:expected[control['path']]=content.replace(control['on_text'],control['off_text'],1).encode()
  assert all(actual[k]==hashlib.sha256(v).hexdigest() for k,v in expected.items()),cell['id']
  assert sha(base/cell['binary'])==receipt['binary_sha256']
  options=receipt['build_options'];assert options['profile']=='release' and options['features']=='default' and options['jobs']==2 and options['SDKROOT']==plan['process_env']['SDKROOT'] and options['RUSTFLAGS']==plan['process_env']['RUSTFLAGS']
  assert '--offline' in options['command'] and '--locked' in options['command'] and '+stable' in options['command']
  checks.append({'cell':cell['id'],'source_files':len(actual),'binary_sha256':receipt['binary_sha256'],'only_registered_control_patches':True})
 (out/'EIGHT-CELL-PREMEASUREMENT-VALIDATION.json').write_text(json.dumps({'status':'exact_registered_control_only_cell_closures_verified','checks':checks,'profile_sha256':expected_profile_sha},indent=2)+'\n')
expected_profile_sha=expected
for step in plan['sequence']:
 identities();log=logs/f"{step['sequence']:02d}-{step['stage']}.log";start=time.time();print('START',step['sequence'],step['stage'],flush=True)
 with log.open('w') as stream:rc=subprocess.call(step['argv'],cwd=root,env=env,stdout=stream,stderr=subprocess.STDOUT)
 receipt={'step':step,'started_epoch':start,'finished_epoch':time.time(),'exit_code':rc,'log_sha256':sha(log),'profile_sha256':expected,'approval_sha256':sha(a.approval),'scope':'raw stage observation,not G5 acceptance'};receipts.append(receipt);(logs/f"{step['sequence']:02d}-{step['stage']}.json").write_text(json.dumps(receipt,indent=2)+'\n');print('TERMINAL',step['sequence'],step['stage'],rc,flush=True)
 # Quality red code1 deliberately retained. Build/protocol/foundation anomalies halt.
 if rc not in [0,1] or (step['scope']=='build_only_no_measurements' and rc!=0):
  (out/'EXECUTION-STOPPED.json').write_text(json.dumps({'status':'blocked_foundation_or_build','stage':step,'exit_code':rc,'receipts':receipts},indent=2)+'\n');raise SystemExit(rc or 2)
 if step['stage']=='eight_cell_build':validate_cells()
 identities()
(out/'STAGE-RECEIPTS.json').write_text(json.dumps({'status':'registered_sequence_observed_pending_raw_replay_and_independent_acceptance','receipts':receipts,'profile_sha256':expected,'approval_sha256':sha(a.approval),'scope':'no G5/task completion from CLI exits'},indent=2)+'\n')
