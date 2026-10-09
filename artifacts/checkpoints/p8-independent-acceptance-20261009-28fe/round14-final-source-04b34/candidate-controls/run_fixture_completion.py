#!/usr/bin/env python3
"""Complete only the original controls blocked by three missing tracked fixtures."""
from pathlib import Path
import datetime,hashlib,json,os,re,subprocess,tempfile,time
OUT=Path(__file__).resolve().parent
ROOT=OUT.parent.parent/'integration-validation/round14-final-source/source'
ENV=os.environ|{'PYTHONDONTWRITEBYTECODE':'1','GIT_OPTIONAL_LOCKS':'0'}
def git(*args):return subprocess.check_output(['git',*args],cwd=ROOT,env=ENV,text=True).strip()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def inputs():return {str(p.relative_to(ROOT)):sha(p) for p in sorted((ROOT/'scripts').rglob('*.py'))}
assert git('rev-parse','HEAD')=='4d971d143cd49ae3c1901816c0b7cee0a922977e'
assert not git('status','--porcelain','--untracked-files=all')
original=json.loads((OUT/'all-p8-receipt.json').read_text());assert original['exit_code']==1
materialization=OUT.parent.parent/'integration-validation/round14-final-source/original-fixture-materialization-receipt.json'
r={'schema':'fixed-original-fixture-blocked-tests-completion-v1','actual_HEAD':git('rev-parse','HEAD'),'tree':git('rev-parse','HEAD^{tree}'),'source_inputs_before':inputs(),'original_failed_suite_receipt_sha256':sha(OUT/'all-p8-receipt.json'),'materialization_receipt_sha256':sha(materialization),'started_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commands':[],'scope':'Only original test_p8_external_candidate and test_p8_release_review; no new controls/native build or measurement.'}
(OUT/'fixture-completion-running.json').write_text(json.dumps(r,indent=2)+'\n')
with tempfile.TemporaryDirectory(prefix='codecortex-28fe-round14-fixture-controls-',dir='/dev/shm') as temporary:
    ENV['TMPDIR']=temporary;r['temporary_directory']=temporary
    for index,pattern in enumerate(('test_p8_external_candidate.py','test_p8_release_review.py')):
        argv=['python3','-B','-m','unittest','discover','-s','scripts/tests','-p',pattern,'-v']
        log=OUT/f'fixture-completion-{index}.log';begun=time.monotonic()
        with log.open('xb') as out:done=subprocess.run(argv,cwd=ROOT,env=ENV,stdout=out,stderr=subprocess.STDOUT,timeout=120)
        raw=log.read_bytes();counts=re.findall(rb'^Ran ([0-9]+) tests? in ',raw,re.M)
        r['commands'].append({'argv':argv,'exit_code':done.returncode,'elapsed_seconds':time.monotonic()-begun,'log':log.name,'log_bytes':len(raw),'log_sha256':sha(log),'tests_run':int(counts[-1]) if counts else None,'standalone_OK':bool(re.search(rb'^OK$',raw,re.M))})
r.update(source_inputs_after=inputs(),actual_HEAD_after=git('rev-parse','HEAD'),tree_after=git('rev-parse','HEAD^{tree}'),status_porcelain_after=git('status','--porcelain','--untracked-files=all'),finished_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),temporary_directory_removed=not Path(r['temporary_directory']).exists())
r['inputs_unchanged']=r['source_inputs_before']==r['source_inputs_after']==original['inputs_after']
r['passed']=r['inputs_unchanged'] and r['actual_HEAD']==r['actual_HEAD_after'] and r['tree']==r['tree_after'] and not r['status_porcelain_after'] and all(x['exit_code']==0 and x['standalone_OK'] for x in r['commands'])
(OUT/'fixture-completion-receipt.json').write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'passed':r['passed'],'commands':r['commands'],'inputs_unchanged':r['inputs_unchanged']}),flush=True)
raise SystemExit(0 if r['passed'] else 2)
