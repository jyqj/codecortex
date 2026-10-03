#!/usr/bin/env python3
"""Complete the originally planned bounded matrix after a primary failure.
Never overwrite or omit prior failures; summary stays non-green when any fails.
Also run the independent ready-snapshot supplement, separately source-bound.
"""
import hashlib,json,os,re,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
REPO=HERE.parents[2]
BASE='1a6f60264933a61ba08a5150ebab1b7a13164a38'
SOURCE='5e31dee516c037bb39c7624cbff441bd20759711'
SUPPLEMENT='379caca96a213970d394e35ebe1bcf0c5338d88d'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
write=lambda p,v:p.write_text(json.dumps(v,indent=2,sort_keys=True)+'\n')
env=dict(os.environ,CARGO_HOME='/workspace/.cargo',RUSTUP_HOME='/workspace/.rustup',PATH='/workspace/.cargo/bin:'+os.environ['PATH'],CARGO_BUILD_JOBS='5',CARGO_INCREMENTAL='0')
results=[]
# Existing default runs are already complete. Complete HTTP before doing any
# feature change so failure binary hashes can be checked against prior profile.
for profile,features in [('semantic-http','semantic-http'),('default',None)]:
 for n in range(1,6):
  suites=[('p7_v05_v11_independent_review',6)]
  if features:suites+=[('p7_v11_generation_public',2),('p7_v05_scope_public',2)]
  for suite,count in suites:
   run=HERE/'matrix'/profile/f'{n:02}'/suite
   command=['cargo','test','-p','cc-server','--test',suite,'--locked','--offline']
   if features:command+=['--features',features]
   command+=['--','--nocapture']
   path=REPO/('crates/cc-server/tests/'+suite+'.rs')
   assert path.read_bytes()==subprocess.check_output(['git','show',SOURCE+':'+str(path.relative_to(REPO))],cwd=REPO)
   if not (run/'test.log').exists():
    run.mkdir(parents=True)
    with (run/'test.log').open('wb') as out:
     result=subprocess.run(command,cwd=REPO,env=dict(env,P7_SCOPE_REVIEW_EVIDENCE_DIR=str(run/'raw'),P7_V11_EVIDENCE_DIR=str(run/'raw'),P7_V05_EVIDENCE_DIR=str(run/'raw')),stdout=out,stderr=subprocess.STDOUT)
   if (run/'receipt.json').exists():
    results.append(json.loads((run/'receipt.json').read_text()));continue
   log=(run/'test.log').read_text();m=re.search(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
   assert m, 'retain infrastructure error without turning into a pass'
   passed,failed,ignored=map(int,m.groups());assert passed+failed==count and ignored==0
   runner=re.search(r'Running tests/'+suite+r'\.rs \(([^)]+)\)',log).group(1)
   if failed:
    prior=json.loads((HERE/'matrix/semantic-http/03'/suite/'receipt.json').read_text())
    assert prior['product_binary_sha256']==sha(REPO/'target/debug/codecortex') and prior['test_binary_sha256']==sha(REPO/runner), 'failure build binding differs from unchanged immediately prior profile'
   receipt={'baseline_sha':BASE,'source_sha':SOURCE,'profile':profile,'suite':suite,'repetition':n,'command':command,'exit_code':101 if failed else 0,'passed':passed,'failed':failed,'ignored':ignored,'product_binary_sha256':sha(REPO/'target/debug/codecortex'),'test_binary_sha256':sha(REPO/runner),'source_file_sha256':sha(path),'raw_record_count':len(list((run/'raw').glob('*.json'))),'artifact_sha256':{str(p.relative_to(run)):sha(p) for p in sorted(run.rglob('*')) if p.is_file()},'retained_primary_failure':bool(failed)}
   write(run/'receipt.json',receipt);results.append(receipt)
write(HERE/'matrix/summary.json',{'baseline_sha':BASE,'source_sha':SOURCE,'test_runs':len(results),'test_executions':sum(r['passed']+r['failed'] for r in results),'passed':sum(r['passed'] for r in results),'failed':sum(r['failed'] for r in results),'ignored':sum(r['ignored'] for r in results),'runs':results,'full_V05':False,'full_V11':False,'status':'retained original primary failure; never greened by selective rerun'})
print('original planned matrix',sum(r['passed'] for r in results),'passed',sum(r['failed'] for r in results),'failed',flush=True)
extra=HERE/'ready-snapshot-supplement';extra.mkdir()
supp=[]
for n in range(1,6):
 run=extra/f'{n:02}';run.mkdir();raw=run/'raw'
 suite='p7_v11_ready_epoch_independent_review'
 command=['cargo','test','-p','cc-server','--test',suite,'--features','semantic-http','--locked','--offline','--','--nocapture']
 with (run/'test.log').open('wb') as out:
  result=subprocess.run(command,cwd=REPO,env=dict(env,P7_V11_READY_REVIEW_EVIDENCE_DIR=str(raw)),stdout=out,stderr=subprocess.STDOUT)
 log=(run/'test.log').read_text();assert result.returncode==0 and '1 passed; 0 failed; 0 ignored' in log, 'supplement failure retained'
 records=[json.loads(p.read_text()) for p in sorted(raw.glob('*.json'))];assert len(records)==2
 runner=re.search(r'Running tests/'+suite+r'\.rs \(([^)]+)\)',log).group(1)
 receipt={'baseline_sha':BASE,'source_sha':SUPPLEMENT,'profile':'semantic-http','repetition':n,'command':command,'exit_code':0,'passed':1,'failed':0,'ignored':0,'product_binary_sha256':sha(REPO/'target/debug/codecortex'),'test_binary_sha256':sha(REPO/runner),'source_file_sha256':sha(REPO/('crates/cc-server/tests/'+suite+'.rs')),'raw_record_count':2,'artifact_sha256':{str(p.relative_to(run)):sha(p) for p in sorted(run.rglob('*')) if p.is_file()}}
 write(run/'receipt.json',receipt);supp.append(receipt)
write(extra/'summary.json',{'baseline_sha':BASE,'source_sha':SUPPLEMENT,'passed':5,'failed':0,'ignored':0,'runs':supp,'original_primary_failure_retained':True,'quiescence_claim':False})
print('independent ready-snapshot supplement 5 passed',flush=True)
