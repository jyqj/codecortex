import os,sys,pathlib,json,hashlib,subprocess,datetime,time,signal,ast,re
BASE=pathlib.Path(__file__).resolve().parents[3]
ROOT=BASE/'candidate-macos-integration'
OUT=pathlib.Path(__file__).resolve().parent
EXPECTED_HEAD='ae77486dab0141173fcb2c4c99dd85d34e888157'
EXPECTED_TREE='86233d9f79c6be50145922d1b05d5739d88acc4e'
EXPECTED_PARENT='dd1d00a3bf18d9025fc48222ffe0d21896c26da5'
EXPECTED_P='b413f7882d8bf65f93a6ff3d07e0c17706d95d69'
PY='/Library/Developer/CommandLineTools/usr/bin/python3'
def stamp():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(x):return hashlib.sha256(x).hexdigest()
def write(name,v):(OUT/name).write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def git(*args):return subprocess.check_output(['git',*args],cwd=ROOT)
def meta(path):
 b=path.read_bytes();return {'bytes':len(b),'sha256':sha(b)}
reg=json.loads((ROOT/'scripts/reviewed-source-registry-v15.json').read_text())
def snapshot():
 files=set(reg['complete_inputs'])|set(reg['validation_inputs'])|{
 'scripts/verify_reviewed_source_v15.py','scripts/reviewed-source-registry-v15.json',
 '.github/workflows/ci.yml','artifacts/checkpoints/p8-macos-integration-bfcc-20261009/independent-source-review.json'}
 result={}
 for p in sorted(files):result[p]=meta(ROOT/p)
 return {'head':git('rev-parse','HEAD').decode().strip(),'tree':git('rev-parse','HEAD^{tree}').decode().strip(),'parents':git('show','-s','--format=%P','HEAD').decode().strip().split(),'branch':git('branch','--show-current').decode().strip(),'status_porcelain':git('status','--porcelain=v1','--untracked-files=all').decode(),'tracked_paths_modes_blobs_sha256':sha(git('ls-files','-s','-z')),'input_count':len(result),'inputs':result}
start=snapshot()
write('initial-source.json',start)
assert start['head']==EXPECTED_HEAD and start['tree']==EXPECTED_TREE and start['parents']==[EXPECTED_PARENT]
assert start['branch']=='task/p8-macos-integration-bfcc-20261009'
assert start['status_porcelain']==''
assert len(reg['complete_inputs'])==1089 and len(reg['validation_inputs'])==138
assert meta(ROOT/'scripts/reviewed-source-registry-v15.json')['sha256']=='b3185920fa5c4ce5d1eedb8b49a1ffce7e3ffc01829e7e0f88153b7756699462'
assert meta(ROOT/'scripts/verify_reviewed_source_v15.py')['sha256'].startswith('c044dbc0')
assert not os.environ.get('PYTHONOPTIMIZE') and not os.environ.get('PYTHONPATH') and sys.flags.optimize==0
pins={}
for p in (ROOT/'scripts').glob('*.py'):
 if not (p.name.startswith('verify_') or p.name.endswith('_historical_context.py')):continue
 tree=ast.parse(p.read_text())
 for n in tree.body:
  if isinstance(n,ast.Assign):
   for v in ast.walk(n.value):
    if isinstance(v,ast.Constant) and isinstance(v.value,str) and re.fullmatch('[0-9a-f]{40}',v.value):
     pins.setdefault(v.value,[]).append(str(p.relative_to(ROOT)))
refs=sorted(pins)
read=subprocess.run(['git','cat-file','--batch-check=%(objectname) %(objecttype)'],cwd=ROOT,input=''.join(x+'^{commit}\n' for x in refs),capture_output=True,text=True)
write('historical-object-preconditions.json',{'pins':pins,'exit_code':read.returncode,'stdout':read.stdout,'stderr':read.stderr,'scope':'Static named constant/literal commit pins in verify scripts and historical adapters; complete inherited history is additionally checked by original CLI.'})
assert read.returncode==0 and len(read.stdout.splitlines())==len(refs) and all(x.endswith(' commit') for x in read.stdout.splitlines())
defs=[]
for p in sorted((ROOT/'tests/source_integrity').glob('test_*.py')):
 t=ast.parse(p.read_text())
 defs.append({'path':str(p.relative_to(ROOT)),'test_definition_count':sum(isinstance(n,(ast.FunctionDef,ast.AsyncFunctionDef)) and n.name.startswith('test_') for n in ast.walk(t))})
assert sum(x['test_definition_count'] for x in defs)==181
env=dict(os.environ);env['PYTHONDONTWRITEBYTECODE']='1'
commands=[
 ('source-integrity',[PY,'-m','unittest','discover','-s','tests/source_integrity','-v']),
 ('original-v15',[PY,'scripts/verify_reviewed_source_v15.py','--source-version','p8-completion-source-20261009-v15'])]
write('execution-plan.json',{'created_at':stamp(),'actual_head':EXPECTED_HEAD,'actual_tree':EXPECTED_TREE,'parent':EXPECTED_PARENT,'product':EXPECTED_P,'python_executable':sys.executable,'python_version':sys.version,'optimize':sys.flags.optimize,'commands':[{'name':n,'argv':a} for n,a in commands],'cwd':str(ROOT),'child_env_overrides':{'PYTHONDONTWRITEBYTECODE':'1'},'PYTHONOPTIMIZE':None,'PYTHONPATH':None,'per_child_outer_timeout_seconds':3600,'runner_outer_timeout_seconds':7500,'internal_validation_logic_or_budgets_modified':False,'source_integrity_static_definitions':defs,'remaining_original_todos':29,'todo_credit':0})
completed=[]
for name,argv in commands:
 before=snapshot()
 assert before==start
 write(name+'.source-before.json',before)
 rec={'name':name,'command':argv,'cwd':str(ROOT),'started_at':stamp(),'status':'starting','exit_code':None,'actual_head':EXPECTED_HEAD,'outer_timeout_seconds':3600,'source_before_file':name+'.source-before.json','child_env_overrides':{'PYTHONDONTWRITEBYTECODE':'1'}}
 write(name+'.json',rec)
 timed_out=False
 with (OUT/(name+'.stdout')).open('wb') as stdout,(OUT/(name+'.stderr')).open('wb') as stderr:
  proc=subprocess.Popen(argv,cwd=ROOT,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
  rec.update(pid=proc.pid,status='running');write(name+'.json',rec)
  print(json.dumps({'started':name,'pid':proc.pid,'time':rec['started_at']}),flush=True)
  try:code=proc.wait(timeout=3600)
  except subprocess.TimeoutExpired:
   timed_out=True;os.killpg(proc.pid,signal.SIGTERM)
   try:code=proc.wait(timeout=10)
   except subprocess.TimeoutExpired:os.killpg(proc.pid,signal.SIGKILL);code=proc.wait()
 after=snapshot();write(name+'.source-after.json',after)
 rec.update(status='completed',exit_code=code,timed_out=timed_out,finished_at=stamp(),source_after_file=name+'.source-after.json',source_before_after_equal=after==before,stdout=meta(OUT/(name+'.stdout')),stderr=meta(OUT/(name+'.stderr')))
 write(name+'.json',rec);completed.append(rec)
 print(json.dumps({'completed':name,'exit_code':code,'timed_out':timed_out,'source_unchanged':after==before,'time':rec['finished_at']}),flush=True)
 if code!=0 or timed_out or after!=before:
  write('final.json',{'status':'not_passed','completed':completed,'stopped_after':name,'source_unchanged':after==before,'actual_head':EXPECTED_HEAD,'todo_credit':0})
  raise SystemExit(code if 0<code<256 else 1)
stderr=(OUT/'source-integrity.stderr').read_text()
assert re.search(r'Ran 181 tests in [0-9.]+s\s+OK\s*$',stderr),stderr[-1000:]
write('final.json',{'status':'passed','actual_head':EXPECTED_HEAD,'actual_tree':EXPECTED_TREE,'source_unchanged':snapshot()==start,'completed':completed,'source_integrity_tests':181,'scope':'Original source-integrity tests and original v15 source admission only; no workload/performance/release/TODO credit.','todo_credit':0,'remaining_original_todos':29})
print(json.dumps({'final':'passed','head':EXPECTED_HEAD,'child_exit_codes':[x['exit_code'] for x in completed],'source_integrity_tests':181}),flush=True)
