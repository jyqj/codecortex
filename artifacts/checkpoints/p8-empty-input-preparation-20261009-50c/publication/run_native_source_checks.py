import datetime,hashlib,json,os,subprocess,sys,time
from pathlib import Path
root=Path('/workspace/scratch/50c364fd60b1/codecortex')
out=Path('/workspace/scratch/50c364fd60b1/validation/native-source-checks')
out.mkdir(exist_ok=False)
expected='50d9b3bc7d1ecca1b6b1c3822a36c12b546b0529'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==expected
record={'head':expected,'product_source':'b4fef72211e5967f4fba729d25d0ca2958094fd5','commands':[],'started_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Original unchanged native source-integrity suite and v15 entry, per-command 3600s execution bound; a timeout is not a pass.'}
commands=[('native-suite',[sys.executable,'-B','-m','unittest','discover','-s','tests/source_integrity','-v']),('v15-cli',[sys.executable,'-B','scripts/verify_reviewed_source_v15.py','--source-version','p8-completion-source-20261009-v15'])]
for name,command in commands:
 started=time.monotonic();path=out/(name+'.log')
 with path.open('xb') as log:
  result=subprocess.run(['timeout','--signal=TERM','--kill-after=15s','3600s',*command],cwd=root,env={**os.environ,'PYTHONDONTWRITEBYTECODE':'1'},stdout=log,stderr=subprocess.STDOUT)
 row={'name':name,'command':command,'exit_code':result.returncode,'elapsed_seconds':time.monotonic()-started,'log':path.name,'log_sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
 record['commands'].append(row)
 (out/'receipt.json').write_text(json.dumps(record,indent=2,sort_keys=True)+'\n')
 print(json.dumps(row),flush=True)
record['head_after']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
record['completed_at_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
record['all_passed']=all(x['exit_code']==0 for x in record['commands']) and record['head_after']==expected
(out/'receipt.json').write_text(json.dumps(record,indent=2,sort_keys=True)+'\n')
sys.exit(0 if record['all_passed'] else 1)
