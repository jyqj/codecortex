#!/usr/bin/env python3
"""Fixed-source independent product replay; immutable, sequential feature builds."""
import hashlib,json,os,re,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
REPO=HERE.parents[2]
BASE='c8c20b5b7d416372ee06ed5e248663c48912064e'
SOURCE='153ced0bf83a844f83211e41e2841d9a265786a7'
FROZEN='c8ac032b7e84989bceedd81bdd197150688d6953'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
write=lambda p,v:p.write_text(json.dumps(v,indent=2,sort_keys=True)+'\n')
for path,source in [('crates/cc-server/tests/p7_v18_parameter_contract.rs',FROZEN),('crates/cc-server/tests/p7_query_deadline_public.rs',SOURCE)]:
 assert (REPO/path).read_bytes()==subprocess.check_output(['git','show',source+':'+path],cwd=REPO), 'Frozen source mismatch'
root=HERE/'matrix'
root.mkdir()
env=dict(os.environ,CARGO_HOME='/workspace/.cargo',RUSTUP_HOME='/workspace/.rustup',PATH='/workspace/.cargo/bin:'+os.environ['PATH'],CARGO_BUILD_JOBS='5',CARGO_INCREMENTAL='0')
results=[]
for profile,feature,count in [('default',None,8),('semantic','semantic',9),('semantic-http','semantic-http',12)]:
 for n in range(1,6):
  suites=[('p7_v18_parameter_contract',count)]
  if feature=='semantic-http': suites+=[('p7_query_deadline_public',2)]
  for suite,total in suites:
   run=root/profile/f'{n:02}'/suite
   run.mkdir(parents=True)
   raw=run/'raw'
   command=['cargo','test','-p','cc-server','--test',suite,'--locked','--offline']
   if feature:command+=['--features',feature]
   command+=['--','--nocapture']
   with (run/'test.log').open('wb') as out:
    result=subprocess.run(command,cwd=REPO,env=dict(env,P7_V18_EVIDENCE_DIR=str(raw),P7_PUBLIC_EVIDENCE_DIR=str(raw)),stdout=out,stderr=subprocess.STDOUT)
   log=(run/'test.log').read_text()
   if result.returncode or f'{total} passed; 0 failed; 0 ignored' not in log:raise RuntimeError(f'{profile}/{n}/{suite}: failure retained')
   records=[json.loads(p.read_text()) for p in sorted(raw.glob('*.json'))]
   if suite=='p7_query_deadline_public':
    assert len(records)==6
    for record in records:
     closed=[e for e in record['http'] if e['kind']=='peer_closed']
     assert record['only_loopback'] and not record['real_credentials']
     if record['test'].startswith('deadline-'):
      assert record['query_calls']==2 and len(closed)==1 and closed[0]['elapsed_ms']<1500
     else:
      assert record['query_calls']==3 and len(closed)==2 and max(e['elapsed_ms'] for e in closed)<2500
   elif feature=='semantic-http':
    probes=[r['data'] for r in records if r.get('kind')=='query_network_probe']
    assert any(p['query_http_calls']==2 for p in probes), 'Search/context must actually encode separate markers'
   runner=re.search(r'Running tests/'+suite+r'\.rs \(([^)]+)\)',log).group(1)
   receipt={'baseline_sha':BASE,'source_sha':SOURCE,'frozen_v18_source_sha':FROZEN,'profile':profile,'suite':suite,'repetition':n,'command':command,'exit_code':0,'passed':total,'failed':0,'ignored':0,'product_binary_sha256':sha(REPO/'target/debug/codecortex'),'test_binary_sha256':sha(REPO/runner),'source_file_sha256':sha(REPO/('crates/cc-server/tests/'+suite+'.rs')),'raw_record_count':len(records),'artifact_sha256':{str(p.relative_to(run)):sha(p) for p in sorted(run.rglob('*')) if p.is_file()}}
   write(run/'receipt.json',receipt)
   results.append(receipt)
 print(profile,'5 complete repetitions passed',flush=True)
write(root/'summary.json',{'baseline_sha':BASE,'source_sha':SOURCE,'frozen_v18_source_sha':FROZEN,'test_runs':len(results),'test_executions':sum(r['passed'] for r in results),'failed':0,'ignored':0,'runs':results,'scope':'synthetic public product stdio; complete gates plus real HTTP header/body deadlines and post-cancel fallback','immediate_physical_http_abort':'not claimed: blocking I/O retains its clamped deadline','real_provider_calls':0,'D1_D2':'unchanged','formal_P7_014':'main owner full regression and independent formal review remain separate'})
