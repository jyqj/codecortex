#!/usr/bin/env python3
"""Read-only integrity verification of saved gold, never gold regeneration."""
import hashlib,json,pathlib,sys,tarfile
R=pathlib.Path(__file__).resolve().parent
sys.path.insert(0,str(R/'.validation-deps'))
import blake3,jsonschema
sha=lambda b:hashlib.sha256(b).hexdigest()
prov=json.loads((R/'provenance.json').read_text()); suite=json.loads((R/'suite.candidate.json').read_text())
rows=[json.loads(s) for s in (R/suite['queries']).read_text().splitlines()]
schema=json.loads((R.parents[4]/'crates/cc-eval/benchmarks/schema/query.schema.json').read_text())
assert prov['license_sha256']=='a7d00bfd54525bc694b6e32f64c7ebcf5e6b7ae3657be5cc12767bce74654a47'
assert sha((R/'license/LICENSE.txt').read_bytes())==prov['license_sha256']
for f in prov['license_artifacts']:assert sha((R/f['path']).read_bytes())==f['sha256']
assert blake3.blake3((R/suite['queries']).read_bytes()).hexdigest()==suite['queries_digest']
files=[]
for f in prov['files']:
 b=(R/'source'/f['path']).read_bytes();assert sha(b)==f['sha256']
 assert len(b)==f['bytes'];assert blake3.blake3(b).hexdigest()==f['digest']
 files.append({k:f[k] for k in ['path','bytes','digest']})
assert blake3.blake3(json.dumps(files,separators=(',',':')).encode()).hexdigest()==suite['source']['digest']
assert len({q['query_family'] for q in rows})==len(rows)==20
clusters={}
for q in rows:
 jsonschema.validate(q,schema);a=q['annotations'];assert a['source_sha']==prov['upstream_sha']
 assert sha(q['query_family'].encode())==a['family_hash_sha256']
 h=sha((a['split_salt']+':'+a['split_cluster']).encode());assert h==a['split_hash_sha256']
 assert q['split']==('holdout' if int(h[:8],16)%4==0 else 'dev')
 assert clusters.setdefault(a['split_cluster'],q['split'])==q['split']
 if q['no_answer']:
  e=a['absence_evidence']; assert q['answers']==q['expected_files']==[]
  assert e['scope_files']==[q['path_prefix']]
  b=(R/'source'/q['path_prefix']).read_bytes();assert sha(b)==e['file_sha256']
  assert e['checked_bytes']==[0,len(b)] and e['checked_lines']==[1,len(b.splitlines())]
  assert all(t.encode().lower() not in b.lower() for t in e['absent_tokens'])
 else:
  for g,e in zip(q['answers'],a['gold_evidence'],strict=True):
   alt=g['alternatives'][0]; b=(R/'source'/e['path']).read_bytes();span=alt['span']
   assert alt['path']==e['path'] and alt['symbol']['name']==e['symbol']
   assert span=={'start':e['start_byte'],'end':e['end_byte']}
   chunk=b[span['start']:span['end']];assert sha(chunk)==e['span_sha256'];chunk.decode('utf-8')
   assert b[:span['start']].count(b'\n')+1==e['start_line']
   assert b[:span['end']].count(b'\n')==e['end_line']
  for edge in a['chain_edges']:
   e=a['gold_evidence'][edge['from_facet']-1]; target=a['gold_evidence'][edge['to_facet']-1]
   b=(R/'source'/edge['path']).read_bytes()
   assert e['start_byte']<=edge['start_byte']<edge['end_byte']<=e['end_byte']
   assert b[edge['start_byte']:edge['end_byte']]==target['symbol'].encode()+b'('
if len(sys.argv)>1:
 with tarfile.open(sys.argv[1]) as t:
  prefix='TypeScript-'+prov['upstream_sha']+'/'
  for f in prov['files']:
   assert t.extractfile(prefix+f['path']).read()==(R/'source'/f['path']).read_bytes()
  for p in ['LICENSE.txt','NOTICE.txt']:
   assert t.extractfile(prefix+p).read()==(R/'license'/p).read_bytes()
print('PASS: 20 candidate families; schema, source locks, saved spans, chain edges, scoped negative evidence and related-family splits'+('; upstream archive bytes' if len(sys.argv)>1 else ''))
