#!/usr/bin/env python3
"""One preregistered sequence, stable owned disk progress, actual waits only."""
import argparse,hashlib,json,os,pathlib,subprocess,time
p=argparse.ArgumentParser();p.add_argument('--profile',type=pathlib.Path,required=True);p.add_argument('--approval',type=pathlib.Path,required=True);a=p.parse_args();root=pathlib.Path.cwd();profile=json.load(open(a.profile));approval=json.load(open(a.approval));sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();expected=sha(a.profile);assert approval['decision']=='approved_to_execute' and approval['profile_sha256']==expected
plan_path=root/'artifacts/benchmarks/p5e-formal-plan-20261001-v2/FINAL-RUN-PLAN.json';plan=json.load(open(plan_path));assert sha(plan_path)=='8f9f5b6800324089aab1d50f4da00fcd8f70773f35638774030e9aec9797f177';assert profile['execution_sequence']==plan['sequence'];manifest=json.load(open(root/'artifacts/benchmarks/p5e-harness-20261001/final-v6/FINAL-HARNESS-INPUT-MANIFEST.json'))
def identities():
 for item in manifest['sourcefiles']+manifest['producer_receipts']+manifest['short_control_actual_evidence']+[manifest['original51_source_lock']]:
  f=root/item['path'];assert f.stat().st_size==item['bytes'] and sha(f)==item['sha256'],item['path']
 for item in plan['binaries'].values():assert sha(root/item['path'])==item['sha256'],item['path']
 for item in profile['locked_inputs']:assert sha(root/item['path'])==item['sha256'],item['path']
identities();out=root/'artifacts/benchmarks/p5e-formal-runs-20261001-v2';assert not out.exists();out.mkdir();logs=out/'stage-logs';logs.mkdir();env=os.environ.copy();env.update(plan['process_env']);receipts=[];progress=out/'progress.jsonl'
def cell_verify():
 b=out/'factorial-build';m=json.load(open(b/'plan.json'));ref=json.load(open(b/'reference-source.json'));targets=set();checks=[]
 for cell in m['variants']:
  r=json.load(open(b/cell['build_receipt']));assert r['exit_code']==0;t=r['build_options']['CARGO_TARGET_DIR'];assert t not in targets;targets.add(t);source=b/cell['source_root'];actual={p.relative_to(source).as_posix():sha(p) for p in source.rglob('*') if p.is_file()};assert actual==r['source_files'] and set(actual)==set(ref)
  expectedbytes={k:(b/'reference'/k).read_bytes() for k in ref}
  for c in m['controls']:
   text=expectedbytes[c['path']].decode();assert text.count(c['on_text'])==1
   if c['id'] not in cell['enabled']:expectedbytes[c['path']]=text.replace(c['on_text'],c['off_text'],1).encode()
  assert all(actual[k]==hashlib.sha256(v).hexdigest() for k,v in expectedbytes.items());assert sha(b/cell['binary'])==r['binary_sha256'];checks.append({'cell':cell['id'],'target':t,'binary_sha256':r['binary_sha256'],'only_registered_control_patch':True})
 (out/'CELL-SOURCE-POSTBUILD-CHECK.json').write_text(json.dumps({'status':'independenttargetsourceonlyregisteredpatch+binaryreceiptverified_runtimecontrolsnextmandatory','checks':checks},indent=2)+'\n')
for step in plan['sequence']:
 identities();log=logs/f"{step['sequence']:02d}-{step['stage']}.log";start=time.time()
 with progress.open('a') as f:f.write(json.dumps({'event':'start','epoch':start,'step':step})+'\n')
 with log.open('w') as stream:
  child=subprocess.Popen(step['argv'],cwd=root,env=env,stdout=stream,stderr=subprocess.STDOUT);actual_rc=child.wait()
 receipt={'step':step,'started_epoch':start,'finished_epoch':time.time(),'actual_return_code':actual_rc,'child_pid':child.pid,'log_sha256':sha(log),'profile_sha256':expected,'approval_sha256':sha(a.approval),'cwd':str(root),'env':plan['process_env'],'meaning':'raw observation,not G5 verdict'};receipts.append(receipt);(logs/f"{step['sequence']:02d}-{step['stage']}.json").write_text(json.dumps(receipt,indent=2)+'\n')
 with progress.open('a') as f:f.write(json.dumps({'event':'terminal','actual_rc':actual_rc,'step':step['stage'],'epoch':time.time()})+'\n')
 if actual_rc not in [0,1] or (step['scope'] in ['build_only_no_measurements','mandatory_premeasurement_binary_behavior_binding'] and actual_rc!=0):
  (out/'EXECUTION-STOPPED.json').write_text(json.dumps({'status':'blocked_build_controlbinding_provenance_or_foundation','receipt':receipt},indent=2)+'\n');raise SystemExit(actual_rc or 2)
 if step['stage']=='eight_cell_build':cell_verify()
 identities()
(out/'STAGE-RECEIPTS.json').write_text(json.dumps({'status':'registered31stages_observed_pending_rawreplay_and_independentqualityacceptance','receipts':receipts,'profile_sha256':expected,'approval_sha256':sha(a.approval)},indent=2)+'\n')
