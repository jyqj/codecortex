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
rows=[json.loads(l) for l in (base/'queries.native.dev.jsonl').read_text().splitlines()]
assert len({q['query_family'] for q in rows})==len(rows)
gold={g['family']:g for g in json.loads((base/'gold/evidence.json').read_text())}; assert set(gold)=={q['query_family'] for q in rows}
spans=edges=noanswers=0
prefix=b'codecortex-public-v19-split-v1\n'
for q in rows:
 a=q['annotations']['v19']; assert q['split']=='dev'; assert int.from_bytes(hashlib.sha256(prefix+a['global_family'].encode()).digest()[:8],'big')>=2**62; assert a['source_sha']==sha; assert a['review_status']=='pending'
 assert gold[q['query_family']]==dict(family=q['query_family'],**a)
 ev=a['gold_evidence']; assert ev
 for i,e in enumerate(ev):
  b=(base/'source'/e['path']).read_bytes(); start,end=e['span']['start'],e['span']['end']; assert 0<=start<end<=len(b); b[start:end].decode('utf8'); assert e['source_sha']==sha; assert hashlib.sha256(b[start:end]).hexdigest()==e['sha256']; assert e['start_line']==b[:start].count(b'\n')+1; assert e['end_line']==b[:end].count(b'\n')+1; assert e['symbol'].split('.')[-1].encode() in b[start:end].splitlines()[0]; spans+=1
  if not q['no_answer']:
   alt=q['answers'][i]['alternatives'][0]; assert alt['path']==e['path'] and alt['span']==e['span']; assert alt['symbol']['name']==e['symbol'].split('.')[-1]; assert q['answers'][i]['primary']==(i==0); assert q['answers'][i]['grade']==(3 if i==0 else 2)
 for edge in a['graph_constraints']:
  assert edge['from_group'] in {g['id'] for g in q['answers']} and edge['to_group'] in {g['id'] for g in q['answers']}; assert all(e in ev for e in edge['evidence']); edges+=1
 if q['no_answer']:
  noanswers+=1; assert not q['answers'] and not q['expected_files']; review=a['absence_review']; assert review['scope']==sorted({e['path'] for e in ev})
  for p,d in review['scope_sha256'].items(): assert hashlib.sha256((base/'source'/p).read_bytes()).hexdigest()==d
receipt=json.loads((base/'corpus-receipt.json').read_text()); assert receipt['published_native_dev_rows']==len(rows); assert receipt['eligible_confirmatory_holdout_families']==0; assert receipt['custody_status']=='holdout_custody_blocked'
assert hashlib.sha256((base/'queries.native.dev.jsonl').read_bytes()).hexdigest()==receipt['query_file_sha256']['native_dev']
compat=[json.loads(l) for l in (base/'queries.compat.dev.jsonl').read_text().splitlines()]; assert len(compat)==len(rows)-noanswers
result=dict(upstream_sha=sha,source_files=len(files),source_byte_identity=True,license_sha256=hashlib.sha256(license_bytes).hexdigest(),published_dev_families=len(rows),published_compat_dev=len(compat),spans=spans,chain_edges=edges,bounded_noanswers=noanswers,draft_family_count=100,custody_blocked_quarantine=receipt['quarantine_custody_blocked_families'],independent_review=False,retrieval_run=False)
print(json.dumps(result,indent=2))
