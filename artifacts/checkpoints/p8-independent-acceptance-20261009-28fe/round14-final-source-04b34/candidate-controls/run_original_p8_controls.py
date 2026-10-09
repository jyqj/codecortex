#!/usr/bin/env python3
"""Run the existing complete P8 Python suite on the frozen composite product."""
from pathlib import Path
import datetime,hashlib,json,os,re,shutil,subprocess,tempfile,time
OUT=Path(__file__).resolve().parent
ROOT=OUT.parent.parent/'integration-validation/round14-final-source/source'
ENV=os.environ|{'PYTHONDONTWRITEBYTECODE':'1','GIT_OPTIONAL_LOCKS':'0'}
def git(*args):return subprocess.check_output(['git',*args],cwd=ROOT,env=ENV,text=True).strip()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def inputs():return {str(p.relative_to(ROOT)):sha(p) for p in sorted((ROOT/'scripts').rglob('*.py'))}
head=git('rev-parse','HEAD');assert head=='4d971d143cd49ae3c1901816c0b7cee0a922977e'
assert not git('status','--porcelain','--untracked-files=all')
r={'schema':'actual-composite-original-P8-python-controls-v1','cwd':str(ROOT),'actual_HEAD_before':head,'actual_tree_before':git('rev-parse','HEAD^{tree}'),'inputs_before':inputs(),'argv':['python3','-B','-m','unittest','discover','-s','scripts/tests','-p','test_p8*.py','-v'],'started_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'running','scope':'Original Python controls only; no Cargo compilation, native measurements or original TODO completion.'}
OUT.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='codecortex-28fe-round14-controls-',dir='/dev/shm') as temporary:
    ENV['TMPDIR']=temporary
    r['temporary_directory']=temporary
    (OUT/'all-p8-running.json').write_text(json.dumps(r,indent=2)+'\n')
    started=time.monotonic()
    with (OUT/'all-p8.log').open('xb') as log:
        done=subprocess.run(r['argv'],cwd=ROOT,env=ENV,stdout=log,stderr=subprocess.STDOUT,timeout=600)
    r.update(exit_code=done.returncode,elapsed_seconds=time.monotonic()-started,finished_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),actual_HEAD_after=git('rev-parse','HEAD'),actual_tree_after=git('rev-parse','HEAD^{tree}'),status_porcelain_after=git('status','--porcelain','--untracked-files=all'),inputs_after=inputs())
    raw=(OUT/'all-p8.log').read_bytes();counts=re.findall(rb'^Ran ([0-9]+) tests? in ',raw,re.M)
    r.update(log_bytes=len(raw),log_sha256=sha(OUT/'all-p8.log'),tests_run=int(counts[-1]) if counts else None,standalone_OK=bool(re.search(rb'^OK$',raw,re.M)),inputs_unchanged=r['inputs_before']==r['inputs_after'],HEAD_unchanged=r['actual_HEAD_before']==r['actual_HEAD_after'],tree_unchanged=r['actual_tree_before']==r['actual_tree_after'])
r['temporary_directory_removed']=not Path(r['temporary_directory']).exists()
r['status']='passed' if r['exit_code']==0 and r['standalone_OK'] and r['inputs_unchanged'] and r['HEAD_unchanged'] and r['tree_unchanged'] and not r['status_porcelain_after'] else 'failed_or_input_changed'
(OUT/'all-p8-receipt.json').write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({k:r[k] for k in ('status','exit_code','tests_run','elapsed_seconds','log_sha256','inputs_unchanged','HEAD_unchanged','tree_unchanged')}),flush=True)
raise SystemExit(0 if r['status']=='passed' else 2)
