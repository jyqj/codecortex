from pathlib import Path
import sys,json,subprocess,hashlib
root=Path('/workspace/review-owner-evidence'); arm=sys.argv[1]
rows=[json.loads(s) for s in (root/f'{arm}-artifacts.jsonl').read_text().splitlines() if s.startswith('{')]
libs={}
for row in rows:
 if row.get('reason')=='compiler-artifact' and row['target']['name'] in ['cc_eval','cc_server','serde_json'] and row['profile'].get('debuginfo')==2:
  paths=[x for x in row['filenames'] if x.endswith('.rlib')]
  if paths: libs[row['target']['name']]=paths[0]
assert set(libs)=={'cc_eval','cc_server','serde_json'}
cmd=['/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc','--edition','2021',str(root/'independent_driver.rs'),'-L','dependency='+str(Path(libs['cc_server']).parent),'-o',str(root/f'driver-{arm}')]
for k,v in libs.items(): cmd.extend(['--extern',k+'='+v])
r=subprocess.run(cmd,text=True,capture_output=True)
(root/f'{arm}-link.log').write_text(r.stdout+r.stderr)
receipt={'cmd':cmd,'exit':r.returncode,'libraries':{k:{'path':v,'sha256':hashlib.sha256(Path(v).read_bytes()).hexdigest()} for k,v in libs.items()}}
if r.returncode==0: receipt['binary_sha256']=hashlib.sha256((root/f'driver-{arm}').read_bytes()).hexdigest()
(root/f'{arm}-link.json').write_text(json.dumps(receipt,indent=2))
print(r.stdout+r.stderr); sys.exit(r.returncode)
