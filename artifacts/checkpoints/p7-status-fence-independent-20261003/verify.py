#!/usr/bin/env python3
"""Immutable finite repair replay; exact candidate source plus unchanged product."""
import hashlib,json,os,re,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
REPO=HERE.parents[2]
BASE='3dceedf3dcee851b3b2e4d4938bba11d76c63326'
SOURCE='7def2c401e0d302484d62bc6bbd0ac3dd6be1cd4'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
write=lambda p,v:p.write_text(json.dumps(v,indent=2,sort_keys=True)+'\n')
module=REPO/'crates/cc-server/src/capability_status.rs'
prefix=module.read_text().split('\n#[cfg(test)]\nmod tests')[0]
assert module.read_bytes()==subprocess.check_output(['git','show',BASE+':crates/cc-server/src/capability_status.rs'],cwd=REPO)
assert (REPO/'crates/cc-server/tests/support/p7_status_review/candidate.rs').read_text()==prefix+'\n#[path = "checks.rs"]\nmod independent;\n'
original=REPO/'crates/cc-server/tests/p7_v11_generation_public.rs'
assert original.read_bytes()==subprocess.check_output(['git','show','1a6f602:crates/cc-server/tests/p7_v11_generation_public.rs'],cwd=REPO)
root=HERE/'matrix';root.mkdir()
env=dict(os.environ,CARGO_HOME='/workspace/.cargo',RUSTUP_HOME='/workspace/.rustup',PATH='/workspace/.cargo/bin:'+os.environ['PATH'],CARGO_BUILD_JOBS='5',CARGO_INCREMENTAL='0')
results=[]
for profile,feature,suites in [('semantic','semantic',[('p7_status_snapshot_independent_review',4,10)]),('semantic-http','semantic-http',[('p7_status_snapshot_independent_review',4,10),('p7_v11_generation_public',2,20)])]:
 for suite,count,repeats in suites:
  for n in range(1,repeats+1):
   run=root/profile/suite/f'{n:02}';run.mkdir(parents=True);raw=run/'raw'
   command=['cargo','test','-p','cc-server','--test',suite,'--features',feature,'--locked','--offline','--','--nocapture']
   with (run/'test.log').open('wb') as out:
    result=subprocess.run(command,cwd=REPO,env=dict(env,P7_STATUS_REVIEW_EVIDENCE_DIR=str(raw),P7_V11_EVIDENCE_DIR=str(raw)),stdout=out,stderr=subprocess.STDOUT)
   log=(run/'test.log').read_text();m=re.search(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
   assert m, 'infrastructure failure retained'
   passed,failed,ignored=map(int,m.groups())
   runner=re.search(r'Running tests/'+suite+r'\.rs \(([^)]+)\)',log).group(1)
   records=[json.loads(p.read_text()) for p in sorted(raw.glob('*.json'))]
   if result.returncode==0 and suite=='p7_status_snapshot_independent_review':
    assert len(records)==4
    crossed=next(r for r in records if r.get('root_attempts')==2);assert crossed['status']['retrieval']['generation']==crossed['after']==crossed['coverage']['generation']
    churn=next(r for r in records if r.get('root_attempts')==3);assert churn['provider_calls']==4 and churn['status']['retrieval']['generation'] is None and churn['status']['retrieval']['error']['retryable'] is True
    old=next(r for r in records if r.get('old_ready_generation_inconsistent'));assert old['status']['retrieval']['generation']==old['before'] and old['before']!=old['after']
   receipt={'baseline_sha':BASE,'source_sha':SOURCE,'profile':profile,'suite':suite,'repetition':n,'command':command,'exit_code':result.returncode,'passed':passed,'failed':failed,'ignored':ignored,'product_binary_sha256':sha(REPO/'target/debug/codecortex'),'test_binary_sha256':sha(REPO/runner),'status_module_sha256':sha(module),'test_source_sha256':sha(REPO/('crates/cc-server/tests/'+suite+'.rs')),'raw_record_count':len(records),'artifact_sha256':{str(p.relative_to(run)):sha(p) for p in sorted(run.rglob('*')) if p.is_file()}}
   write(run/'receipt.json',receipt);results.append(receipt)
   if result.returncode or passed!=count or failed or ignored:raise RuntimeError(f'{profile}/{suite}/{n}: failure retained')
  print(profile,suite,repeats,'bounded runs passed',flush=True)
write(root/'summary.json',{'baseline_sha':BASE,'source_sha':SOURCE,'test_runs':len(results),'test_executions':sum(r['passed'] for r in results),'failed':0,'ignored':0,'runs':results,'original_PR52_failure':'retained as historical result, not overwritten','levels':{'fixture':'L2 exact-source private observer; real library worker/DB','original':'L3 actual product stdio loopback'},'full_V11':False,'full_V05':False,'formal_P7_014':'separate main owner acceptance','D1_D2':'unchanged'})
