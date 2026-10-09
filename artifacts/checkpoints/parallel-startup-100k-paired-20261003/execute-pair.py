#!/usr/bin/env python3
import hashlib,json,os,pathlib,subprocess,time
O=pathlib.Path(__file__).resolve().parent
def read(p):return json.loads(p.read_text())
def write(p,v):p.write_text(json.dumps(v,indent=2)+'\n')
p=read(O/'preregistration.json');receipts={k:read(O/k/'build-receipt.json') for k in p['order']}
for label,b in receipts.items():
 assert b['build_exit_code']==0 and b['guard_stop'] is None
 assert hashlib.sha256(pathlib.Path(b['compiler_artifact']['executable']).read_bytes()).hexdigest()==b['binary_sha256']
assert receipts['baseline']['rustc']==receipts['candidate']['rustc']
assert receipts['baseline']['cargo']==receipts['candidate']['cargo']
assert read(O/'baseline/source-files.json')['scripts/p7_release_resource_preparation.py']==read(O/'candidate/source-files.json')['scripts/p7_release_resource_preparation.py']==read(O/'source-identity.json')['candidate']['generator_sha256']
# Exact generator identity is independently checked in source-identity.json and compare.py.
write(O/'pair-builds-ready.json',{'time_ns':time.monotonic_ns(),'utc':subprocess.check_output(['date','-u','+%FT%TZ'],text=True).strip(),'preregistration_sha256':hashlib.sha256((O/'preregistration.json').read_bytes()).hexdigest(),'builds':{k:{'source_sha':b['source_sha'],'source_tree':b['source_tree'],'binary_sha256':b['binary_sha256'],'build_wall_seconds':b['wall_seconds'],'receipt_sha256':hashlib.sha256((O/k/'build-receipt.json').read_bytes()).hexdigest()} for k,b in receipts.items()}})
results=[]
for label in p['order']:
 live=O/label/'live';assert not live.exists(),'one run only; existing evidence refuses rerun'
 observed=[]
 for task in pathlib.Path('/proc').iterdir():
  if task.name.isdigit():
   try:
    comm=(task/'comm').read_text().strip()
    if comm in ['cargo','rustc','cc','gcc','clang','ld','lld']:observed.append({'pid':task.name,'comm':comm})
   except OSError:pass
 assert not observed,observed
 write(O/label/'run-preflight.json',{'time_ns':time.monotonic_ns(),'parallel_build_processes':observed,'cpu_max':pathlib.Path('/sys/fs/cgroup/cpu.max').read_text(),'memory_max':pathlib.Path('/sys/fs/cgroup/memory.max').read_text(),'affinity':sorted(os.sched_getaffinity(0)),'fixed_http_port':p['fixed_http_port']})
 print('START',label,flush=True);start=time.monotonic_ns()
 with (O/label/'run.log').open('w') as log:
  result=subprocess.run(['python3',str(O/label/'run.py')],env={**os.environ,'P7_RESOURCE_PORT':str(p['fixed_http_port']),'PYTHONDONTWRITEBYTECODE':'1'},stdout=log,stderr=subprocess.STDOUT)
 row={'variant':label,'start_ns':start,'end_ns':time.monotonic_ns(),'exit_code':result.returncode,'summary_exists':(O/label/'summary.json').exists()};results.append(row);write(O/'pair-execution.json',results)
 print('END',label,json.dumps(row),flush=True)
 # Product timeout retained and candidate still runs once; a harness failure is retained too.
print('PAIR COMPLETED; no additional run',flush=True)
