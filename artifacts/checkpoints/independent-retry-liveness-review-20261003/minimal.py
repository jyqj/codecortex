#!/usr/bin/env python3
"""Replay only the 12-document test, rejecting any mismatched saved binary."""
import pathlib,json,hashlib,subprocess,os,sys
p=pathlib.Path(__file__).resolve().parent
label=sys.argv[1] if len(sys.argv)>1 else 'fixed'
j=json.loads((p/f'{label}-identity.json').read_text())
assert len(j['binaries'])==1
name,digest=next(iter(j['binaries'].items()));binary=p/name
assert hashlib.sha256(binary.read_bytes()).hexdigest()==digest
assert hashlib.sha256((p/'oracle.rs').read_bytes()).hexdigest()==j['oracle_sha256']
env=os.environ.copy();env.update(j['environment'])
cmd=[str(binary),'--exact','semantic_runtime::fresh_retry_oracle::fresh_minimal_12_document_reproduction','--nocapture','--test-threads=1']
with (p/f'{label}-minimal.log').open('w') as f:result=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT)
(p/f'{label}-minimal-receipt.json').write_text(json.dumps({'source':j['source'],'binary':name,'binary_sha256':digest,'oracle_sha256':j['oracle_sha256'],'command':cmd,'exit_code':result.returncode},indent=2)+'\n')
raise SystemExit(result.returncode)
