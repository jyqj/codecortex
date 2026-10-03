"""Build in an exclusive temporary target; guard only the owned Cargo process group."""
import hashlib,json,os,pathlib,signal,subprocess,time
REPO=pathlib.Path('/workspace/codecortex')
OUT=pathlib.Path('/tmp/p7-empty-test-population-release-build')
TARGET=pathlib.Path('/tmp/p7-resource-release-target')
OUT.mkdir();assert TARGET.exists();assert json.loads(pathlib.Path('/tmp/p7-resource-release-build/receipt.json').read_text())['CARGO_TARGET_DIR']==str(TARGET)
source=subprocess.check_output(['git','rev-parse','HEAD'],cwd=REPO,text=True).strip()
env={**os.environ,'CARGO_HOME':'/workspace/.cargo','RUSTUP_HOME':'/workspace/.rustup','PATH':'/workspace/.cargo/bin:'+os.environ['PATH'],'CARGO_BUILD_JOBS':'1','CARGO_INCREMENTAL':'0','CARGO_TARGET_DIR':str(TARGET)}
command=['cargo','build','--release','--locked','--offline','-p','cc-server','--bin','codecortex','--no-default-features','--features','semantic-http','--message-format=json-render-diagnostics']
max_target=2684354560;reserve=1073741824;max_seconds=1800
begin=time.monotonic();guard_stop=None
with (OUT/'cargo.jsonl').open('wb') as stdout,(OUT/'cargo.stderr.log').open('wb') as stderr,(OUT/'guard.jsonl').open('w') as log:
 child=subprocess.Popen(command,cwd=REPO,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
 while child.poll() is None:
  stat={k:int(v) for k,v in (l.split() for l in pathlib.Path('/sys/fs/cgroup/memory.stat').read_text().splitlines())}
  limit=int(pathlib.Path('/sys/fs/cgroup/memory.max').read_text());usage=0;seen=set()
  for parent,dirs,files in os.walk(TARGET):
   for name in files:
    try:
     s=(pathlib.Path(parent)/name).stat()
     if s.st_ino not in seen:usage+=s.st_blocks*512;seen.add(s.st_ino)
    except FileNotFoundError:pass
  available=os.statvfs(OUT);free=available.f_bavail*available.f_frsize
  nonreclaim=stat.get('anon',0)+stat.get('shmem',0)+stat.get('kernel',0)
  row={'elapsed_seconds':time.monotonic()-begin,'cargo_pid':child.pid,'target_allocated_bytes':usage,'tmp_available_bytes':free,'cgroup_memory_current':int(pathlib.Path('/sys/fs/cgroup/memory.current').read_text()),'anon_shmem_kernel_bytes':nonreclaim,'memory_limit':limit};log.write(json.dumps(row)+'\n');log.flush()
  if usage>max_target:guard_stop='exclusive_target_allocation_budget_2.5GiB'
  elif free<reserve:guard_stop='tmp_1GiB_reserve'
  elif nonreclaim>limit*.8:guard_stop='nonreclaimable_memory_80percent_limit'
  elif row['elapsed_seconds']>max_seconds:guard_stop='1800s_build_resource_budget'
  if guard_stop:
   assert os.getpgid(child.pid)==child.pid
   os.killpg(child.pid,signal.SIGTERM)
   try:child.wait(timeout=15)
   except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
   break
  time.sleep(2)
 exit_code=child.wait()
rows=[]
for line in (OUT/'cargo.jsonl').read_text().splitlines():
 try:row=json.loads(line)
 except json.JSONDecodeError:continue
 if row.get('reason')=='compiler-artifact' and row.get('target',{}).get('name')=='codecortex' and row.get('executable'):rows.append(row)
receipt={'source_sha':source,'command':command,'CARGO_TARGET_DIR':str(TARGET),'build_exit_code':exit_code,'guard_stop':guard_stop,'elapsed_seconds':time.monotonic()-begin,'resource_budgets':{'target_allocated_bytes':max_target,'tmp_reserve_bytes':reserve,'nonreclaimable_memory_fraction':.8,'build_seconds':max_seconds},'scope':'isolated owned group, no OS/network permission changes; no other process signals'}
if exit_code==0:
 assert len(rows)==1 and sorted(rows[0]['features'])==['semantic','semantic-http']
 binary=pathlib.Path(rows[0]['executable']);os.link(binary,OUT/'codecortex')
 receipt.update(binary=str(OUT/'codecortex'),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),compiler_artifact=rows[0],profile='release',saved_hardlink=True)
(OUT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({k:receipt.get(k) for k in ('source_sha','build_exit_code','guard_stop','elapsed_seconds','binary_sha256','binary')}))
