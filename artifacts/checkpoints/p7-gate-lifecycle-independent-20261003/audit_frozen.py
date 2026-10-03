#!/usr/bin/env python3
"""Bind retained raw receipts to their recorded hashes, never relabel prior runs."""
import hashlib,json,pathlib,subprocess
HERE=pathlib.Path(__file__).resolve().parent
ROOT=HERE.parents[2]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
blocks={
 'V05_public_BM25_and_hydrate':('2ae064d30d15ab1348320b0af3f694a010a7e3fe','artifacts/checkpoints/p7-v05-v11-independent-20261003'),
 'V18_public_parameters_deadline_cancel':('027fc86d20950cbdc4b6e59aabebe39a4c6e6d70','artifacts/checkpoints/p7-query-public-independent-20261002')}
rows=[]
for name,(commit,path) in blocks.items():
 receipts=list((ROOT/path/'matrix').rglob('receipt.json'))
 assert receipts
 files=0;raw=0;failures=0
 for receipt in receipts:
  r=json.loads(receipt.read_text())
  assert receipt.read_bytes()==subprocess.check_output(['git','show',commit+':'+str(receipt.relative_to(ROOT))],cwd=ROOT)
  for local,digest in r['artifact_sha256'].items():
   p=receipt.parent/local
   assert p.is_file() and sha(p)==digest,(name,p)
   files+=1;raw+=int('/raw/' in str(p))
  failures+=r['failed']
 rows.append({'id':name,'frozen_commit':commit,'receipt_count':len(receipts),'hashed_artifacts_checked':files,'raw_records_checked':raw,'retained_failures':failures,'fresh_product_run':False})
for name,commit,path in [
 ('V16_oracle','2edf2009af1dbc91443a9f030e96103fc8fd056f','artifacts/benchmarks/p7-v16-independent-review-20261002/receipt.json'),
 ('V16_read_bound','ed978e69b63ae932119eaa9530ac1ba929e7e0f3','artifacts/benchmarks/p7-cache-independent-review-20261003/receipt.json')]:
 p=ROOT/path;r=json.loads(p.read_text())
 assert p.read_bytes()==subprocess.check_output(['git','show',commit+':'+path],cwd=ROOT)
 fingerprints=r.get('sha256',r.get('fingerprints'))
 checked=[]
 for source,digest in fingerprints.items():
  checked.append({'path':source,'retained_sha256':digest,'current_equal':sha(ROOT/source)==digest})
 rows.append({'id':name,'frozen_commit':commit,'receipt_sha256':sha(p),'source_fingerprint_comparison':checked,'fresh_product_run':False,'raw_assertion_audit':'v16-frozen-audit.json' if name=='V16_oracle' else 'existing fixed negative/resource receipts; no current RSS recertification'})
legacy=ROOT/'artifacts/checkpoints/cloud-p7-recovery-20261002/stdio-retry-receipt.json'
r=json.loads(legacy.read_text())
rows.append({'id':'legacy_lifecycle_summary','source_checkpoint':r['source_checkpoint'],'source_tree':r['source_tree'],'receipt_sha256':sha(legacy),'historical_only':True,'raw_logs_retained_in_checkout':False,'reason':'summary lists temporary raw logs and no complete stdio/HTTP payloads; new lifecycle supplement supplies fresh proof'})
(HERE/'frozen-evidence-audit.json').write_text(json.dumps(rows,indent=2)+'\n')
print(json.dumps({'blocks':len(rows),'retained_failures':sum(r.get('retained_failures',0) for r in rows),'prior_runs_not_relabelled':True}))
