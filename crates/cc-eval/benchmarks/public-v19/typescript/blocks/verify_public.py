#!/usr/bin/env python3
"""Read-only saved public dev/source verification; counts/hash diagnostics only."""
import hashlib,importlib.util,json,pathlib,subprocess,sys,tarfile
R=pathlib.Path(__file__).resolve().parents[1];sys.path.insert(0,str(R/'.validation-deps'))
import blake3
sp=importlib.util.spec_from_file_location('prepare',R/'blocks/prepare_block.py');m=importlib.util.module_from_spec(sp);sp.loader.exec_module(m)
selected=sys.argv[1:] or [d.name for d in sorted((R/'blocks').iterdir()) if (d/'corpus-receipt.json').exists()]
allrows=[];commitments=[]
with tarfile.open('/tmp/typescript-v19.tar.gz') as t:
 for block in selected:
  D=R/'blocks'/block;receipt=json.loads((D/'corpus-receipt.json').read_text());src=json.loads((D/'source-manifest.json').read_text());assert src['upstream_sha']==m.SHA
  assert m.sha((D/'source-manifest.json').read_bytes())==receipt['source_manifest_sha256']
  for f in src['files']:
   b=(R/'source'/f['path']).read_bytes();assert b==t.extractfile('TypeScript-'+m.SHA+'/'+f['path']).read();assert m.sha(b)==f['sha256'] and len(b)==f['bytes'];assert blake3.blake3(b).hexdigest()==f['blake3']
  for l in src['licenses']:assert m.sha((R/l['path']).read_bytes())==l['sha256']
  rows=[json.loads(s) for s in (D/'queries.native.dev.jsonl').read_text().splitlines()];m.validate(rows);assert all(q['split']=='dev' for q in rows);allrows+=rows
  for profile in ['native','compat']:
   suite=json.loads((D/f'suite.{profile}.candidate.json').read_text());qp=D/suite['queries'];assert blake3.blake3(qp.read_bytes()).hexdigest()==suite['queries_digest'];inventory=[]
   for p in suite['source']['files']:
    b=(R/'source'/p).read_bytes();inventory.append({'path':p,'bytes':len(b),'digest':blake3.blake3(b).hexdigest()})
   assert blake3.blake3(json.dumps(inventory,separators=(',',':')).encode()).hexdigest()==suite['source']['digest']
   proc=subprocess.run([str(R/'.build/debug/cc-eval'),'validate','--suite',str(D/f'suite.{profile}.candidate.json')],capture_output=True);assert proc.returncode==0,(block,profile,m.sha(proc.stderr))
  assert receipt['accepted_families']==0 and receipt['confirmatory_holdout_eligible_families']==0
  assert len(rows)==receipt['public_dev_families'];assert m.sha((D/'queries.native.dev.jsonl').read_bytes())==next(p['file_sha256'] for p in receipt['native_partitions'] if p['partition']=='dev')
  commitments.append({'block':block,'corpus_receipt_sha256':m.sha((D/'corpus-receipt.json').read_bytes()),'candidate_families':receipt['candidate_families'],'public_dev':receipt['public_dev_families'],'holdout_custody_blocked':receipt['holdout_custody_blocked_families']})
assert len(allrows)==len({q['query_family'] for q in allrows});assert len({q['query'].casefold().strip() for q in allrows})==len(allrows)
print(json.dumps({'status':'public_source_schema_span_native_compat_locks_verified_not_review','blocks':commitments,'public_dev_families':len(allrows),'accepted':0,'holdout_access_boundary_verified':False}))
