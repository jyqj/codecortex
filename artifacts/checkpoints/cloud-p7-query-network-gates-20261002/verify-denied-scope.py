#!/usr/bin/env python3
"""Independent supplement: denied local and empty scope, five runs/profile."""
import hashlib,json,os,re,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
REPO=HERE.parents[2]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
source=subprocess.check_output(['git','rev-parse','6f3df4e'],cwd=REPO,text=True).strip()
root=HERE/'denied-scope-matrix'
root.mkdir()
env=dict(os.environ,CARGO_HOME='/workspace/.cargo',RUSTUP_HOME='/workspace/.rustup',PATH='/workspace/.cargo/bin:'+os.environ['PATH'],CARGO_BUILD_JOBS='5',CARGO_INCREMENTAL='0')
results=[]
for profile,features in [('default',None),('semantic','semantic'),('semantic-http','semantic-http')]:
 for n in range(1,6):
  run=root/profile/f'{n:02}'
  run.mkdir(parents=True)
  raw=run/'raw'
  command=['cargo','test','-p','cc-server','--test','p7_v18_parameter_contract','query_network_contract::denied_authority_local_and_empty_scope_make_no_query_calls','--locked','--offline']
  if features: command+=['--features',features]
  command+=['--','--nocapture']
  with (run/'test.log').open('wb') as out:
   result=subprocess.run(command,cwd=REPO,env=dict(env,P7_V18_EVIDENCE_DIR=str(raw)),stdout=out,stderr=subprocess.STDOUT)
  log=(run/'test.log').read_text()
  if result.returncode or '1 passed; 0 failed; 0 ignored' not in log: raise RuntimeError(f'{profile}/{n}: retained failure')
  records=[json.loads(p.read_text()) for p in sorted(raw.glob('*.json'))]
  probes=[r['data'] for r in records if r['kind']=='query_network_probe']
  assert len(probes)==1 and probes[0]['query_http_calls']==0 and probes[0]['only_loopback'] and not probes[0]['real_credentials']
  runner=re.search(r'Running tests/p7_v18_parameter_contract\.rs \(([^)]+)\)',log).group(1)
  receipt={'source_sha':source,'baseline_sha':'00e8c6d667198b1c0a1eb2b4a8fd69ca3fd71c4b','profile':profile,'repetition':n,'command':command,'exit_code':0,'passed':1,'failed':0,'ignored':0,'query_http_calls':0,'document_http_calls':probes[0]['document_http_calls'],'product_binary_sha256':sha(REPO/'target/debug/codecortex'),'test_binary_sha256':sha(REPO/runner),'source_file_sha256':sha(REPO/'crates/cc-server/tests/p7_v18_parameter_contract.rs'),'raw_record_count':len(records),'artifact_sha256':{str(p.relative_to(run)):sha(p) for p in sorted(run.rglob('*')) if p.is_file()}}
  (run/'receipt.json').write_text(json.dumps(receipt,indent=2,sort_keys=True)+'\n')
  results.append(receipt)
 print(profile,'5 denied-scope runs passed',flush=True)
(root/'summary.json').write_text(json.dumps({'source_sha':source,'test_executions':15,'failed':0,'ignored':0,'query_http_calls':0,'runs':results,'live_encoder_local_scope_acceptance':'pending real wiring; denied-authority supplement only','D1_D2':'unchanged'},indent=2,sort_keys=True)+'\n')
