"""Verify byte-identical upstream files, spans and author annotations; never searches."""
import pathlib,json,hashlib,subprocess,sys
base=pathlib.Path(__file__).resolve().parents[1]
sha='43fe48e8a0f44af783116cdb010725e6bb50255f'
upstream=pathlib.Path(sys.argv[1])
assert subprocess.check_output(['git','-C',str(upstream),'rev-parse','HEAD'],text=True).strip()==sha
assert subprocess.check_output(['git','-C',str(upstream),'status','--porcelain'],text=True)==''
inv=json.loads((base/'provenance/inventory.json').read_text()); files={x['path']:x for x in inv if x['admitted']}
assert set(files)=={p.relative_to(base/'source').as_posix() for p in (base/'source').rglob('*') if p.is_file()}
for path,r in files.items():
 b=(base/'source'/path).read_bytes(); assert b==subprocess.check_output(['git','-C',str(upstream),'show',sha+':'+path]); assert hashlib.sha256(b).hexdigest()==r['sha256']; assert b'MIT style' in b[:300] and b'Code generated' not in b
license_bytes=(base/'license/LICENSE').read_bytes(); assert license_bytes==(upstream/'LICENSE').read_bytes(); assert hashlib.sha256(license_bytes).hexdigest()=='b104efb2c7700691650f27034e8541c5ae0ed9af54d884287f19a7467ca2fe7f'
rows=[json.loads(l) for s in ['dev','holdout'] for l in (base/'questions'/f'{s}.jsonl').read_text().splitlines()]
assert len({q['query_family'] for q in rows})==len(rows)
gold={g['family']:g for g in json.loads((base/'gold/evidence.json').read_text())}; assert set(gold)=={q['query_family'] for q in rows}
spans=edges=noanswers=0
for q in rows:
 a=q['annotations']; h=hashlib.sha256(q['query_family'].encode()).hexdigest(); assert a['family_sha256']==h; assert q['split']==('holdout' if int(h,16)%4==0 else 'dev'); assert a['source_sha']==sha; assert a['status']=='candidate_not_independently_reviewed'
 assert gold[q['query_family']]==dict(family=q['query_family'],**a)
 ev=a['gold_evidence']; assert ev
 for i,e in enumerate(ev):
  b=(base/'source'/e['path']).read_bytes(); start,end=e['span']['start'],e['span']['end']; assert 0<=start<end<=len(b); b[start:end].decode('utf8'); assert e['source_sha']==sha; assert hashlib.sha256(b[start:end]).hexdigest()==e['sha256']; assert e['start_line']==b[:start].count(b'\n')+1; assert e['end_line']==b[:end].count(b'\n')+1; assert e['symbol'].split('.')[-1].encode() in b[start:end].splitlines()[0]; spans+=1
  if not q['no_answer']:
   alt=q['answers'][i]['alternatives'][0]; assert alt['path']==e['path'] and alt['span']==e['span']; assert alt['symbol']['name']==e['symbol'].split('.')[-1]
 for edge in a['chain_edges']:
  assert 1<=edge['from_facet']<=len(ev) and 1<=edge['to_facet']<=len(ev); edges+=1
 if q['no_answer']:
  noanswers+=1; assert not q['answers'] and not q['expected_files']; review=a['absence_review']; assert review['scope']==sorted({e['path'] for e in ev})
  for p,d in review['scope_sha256'].items(): assert hashlib.sha256((base/'source'/p).read_bytes()).hexdigest()==d
result=dict(upstream_sha=sha,source_files=len(files),source_byte_identity=True,license_sha256=hashlib.sha256(license_bytes).hexdigest(),families=len(rows),spans=spans,chain_edges=edges,bounded_noanswers=noanswers,independent_review=False,retrieval_run=False)
print(json.dumps(result,indent=2))
