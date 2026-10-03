#!/usr/bin/env python3
"""Immutable finite independent replay; primary files are never modified."""
import hashlib,json,os,re,subprocess
from pathlib import Path
HERE=Path(__file__).resolve().parent
REPO=HERE.parents[2]
BASE='1a6f60264933a61ba08a5150ebab1b7a13164a38'
PR46='4ecfb02b41bdde08db38f595b817f3a84a8c73f4'
SOURCE='5e31dee516c037bb39c7624cbff441bd20759711'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
write=lambda p,v:p.write_text(json.dumps(v,indent=2,sort_keys=True)+'\n')
for path,ref in [('crates/cc-server/tests/p7_v11_generation_public.rs',PR46),('crates/cc-server/tests/p7_v05_scope_public.rs',BASE),('crates/cc-server/tests/p7_v05_v11_independent_review.rs',SOURCE)]:
 assert (REPO/path).read_bytes()==subprocess.check_output(['git','show',ref+':'+path],cwd=REPO), 'exact-source mismatch'
root=HERE/'matrix'
root.mkdir()
env=dict(os.environ,CARGO_HOME='/workspace/.cargo',RUSTUP_HOME='/workspace/.rustup',PATH='/workspace/.cargo/bin:'+os.environ['PATH'],CARGO_BUILD_JOBS='5',CARGO_INCREMENTAL='0')
results=[]
for profile,features in [('default',None),('semantic-http','semantic-http')]:
 for n in range(1,6):
  suites=[('p7_v05_v11_independent_review',6)]
  if features:suites+=[('p7_v11_generation_public',2),('p7_v05_scope_public',2)]
  for suite,count in suites:
   run=root/profile/f'{n:02}'/suite
   run.mkdir(parents=True)
   raw=run/'raw'
   command=['cargo','test','-p','cc-server','--test',suite,'--locked','--offline']
   if features:command+=['--features',features]
   command+=['--','--nocapture']
   with (run/'test.log').open('wb') as out:
    result=subprocess.run(command,cwd=REPO,env=dict(env,P7_SCOPE_REVIEW_EVIDENCE_DIR=str(raw),P7_V11_EVIDENCE_DIR=str(raw),P7_V05_EVIDENCE_DIR=str(raw)),stdout=out,stderr=subprocess.STDOUT)
   log=(run/'test.log').read_text()
   if result.returncode or f'{count} passed; 0 failed; 0 ignored' not in log:raise RuntimeError(f'{profile}/{n}/{suite}: failure retained')
   records=[json.loads(p.read_text()) for p in sorted(raw.glob('*.json'))]
   if suite=='p7_v05_v11_independent_review':
    assert len(records)==6
    stages=next(r for r in records if isinstance(r,dict) and r.get('attempts')==2)
    assert '731' in stages['observations'][0]['first'] and '947' in stages['observations'][0]['last']
    nested=next(r for r in records if isinstance(r,dict) and r.get('inner_attempts')==9)
    assert nested['outer_attempts']==3
   runner=re.search(r'Running tests/'+suite+r'\.rs \(([^)]+)\)',log).group(1)
   receipt={'baseline_sha':BASE,'PR46_sha':PR46,'source_sha':SOURCE,'profile':profile,'suite':suite,'repetition':n,'command':command,'exit_code':0,'passed':count,'failed':0,'ignored':0,'product_binary_sha256':sha(REPO/'target/debug/codecortex'),'test_binary_sha256':sha(REPO/runner),'source_file_sha256':sha(REPO/('crates/cc-server/tests/'+suite+'.rs')),'raw_record_count':len(records),'artifact_sha256':{str(p.relative_to(run)):sha(p) for p in sorted(run.rglob('*')) if p.is_file()}}
   write(run/'receipt.json',receipt)
   results.append(receipt)
 print(profile,'5 bounded repetitions passed',flush=True)
write(root/'summary.json',{'baseline_sha':BASE,'PR46_sha':PR46,'source_sha':SOURCE,'test_runs':len(results),'test_executions':sum(r['passed'] for r in results),'failed':0,'ignored':0,'runs':results,'full_V05':False,'full_V11':False,'D1_D2':'unchanged','scope':'subset only; no whole-public-stage hook or complete cache/resource certification'})
