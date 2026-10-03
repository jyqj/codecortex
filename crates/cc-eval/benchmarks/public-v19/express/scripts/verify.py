#!/usr/bin/env python3
"""Public-dev exact source/span checks. No ranking, holdout output or lock refresh."""
import argparse,hashlib,json,subprocess
from pathlib import Path
P=Path(__file__).resolve().parents[1]
def sha(b):return hashlib.sha256(b).hexdigest()
a=argparse.ArgumentParser();a.add_argument('--block',choices=['first-020','full-100'],default='full-100');a.add_argument('--evaluator',required=True);a.add_argument('--upstream',type=Path);o=a.parse_args()
lock=json.loads((P/'provenance/source-lock.json').read_text());commit=lock['upstream_lock']['source_sha'];assert sha((P/'license/LICENSE').read_bytes())==lock['license_sha256']
if o.upstream:
 assert subprocess.check_output(['git','-C',str(o.upstream),'rev-parse','HEAD'],text=True).strip()==commit
 assert not subprocess.check_output(['git','-C',str(o.upstream),'status','--porcelain'])
 assert not subprocess.check_output(['git','-C',str(o.upstream),'submodule','status','--recursive'])
for f in lock['files']:
 b=(P/'source'/f['path']).read_bytes();assert len(b)==f['bytes'] and sha(b)==f['sha256'];assert b'MIT Licensed' in b[:250] and b'\0' not in b
 assert hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()==f['git_blob']
 if o.upstream:assert b==subprocess.check_output(['git','-C',str(o.upstream),'show',commit+':'+f['path']])
intake=P/'intake'/o.block;rows=[json.loads(x) for x in (intake/'queries.native.dev.jsonl').read_text().splitlines()]
span_count=0
for q in rows:
 assert q['split']=='dev';an=q['annotations']['v19'];author=an['author_provenance'];assert an['review_status']=='pending' and an['reviewer_id'] is None
 expected='holdout' if int.from_bytes(hashlib.sha256(b'codecortex-public-v19-split-v1\n'+an['global_family'].encode()).digest()[:8],'big')<2**62 else 'dev';assert expected=='dev'
 assert an['source_sha']==commit
 for i,e in enumerate(author['source_evidence']):
  b=(P/'source'/e['path']).read_bytes();ls=b.splitlines(keepends=True);start=sum(map(len,ls[:e['line_start']-1]));end=sum(map(len,ls[:e['line_end']]))
  assert start==e['byte_start'] and end==e['byte_end'] and start<end
  assert sha(b[start:end])==e['sha256'] and b[start:end].decode()==e['text'] and e['source_sha']==commit
  if not q['no_answer']:
   alt=q['answers'][i]['alternatives'][0];assert alt['path']==e['path'] and alt['span']=={'start':start,'end':end} and alt['symbol']['name']==e['symbol']
  span_count+=1
 for edge in author['chain_edges']:
  for side in ['from','to']:
   endpoint=edge[side];e=author['source_evidence'][endpoint['evidence_index']];assert endpoint['path']==e['path'] and endpoint['symbol']==e['symbol']
 if an['original_category']=='crossfile-chain':assert len({e['path'] for e in author['source_evidence']})>=2
 if q['no_answer']:
  assert not q['answers'] and not q['expected_files'];proof=author['absence_proof'];files=proof['scope_files'];assert files==sorted(f['path'] for f in lock['files'])
  assert proof['scope_digest_sha256']==sha(json.dumps([(f,sha((P/'source'/f).read_bytes())) for f in files],separators=(',',':')).encode())
  for check in proof['checks']:
   hits=[{'path':f,'line':i,'text':line} for f in files for i,line in enumerate((P/'source'/f).read_text().splitlines(),1) if check['literal'] in line];assert hits==check['hits']
checks=[]
for profile in ['native','compat']:
 suite=intake/f'suite.{profile}.dev.json';r=subprocess.run([o.evaluator,'validate','--suite',str(suite)],capture_output=True)
 checks.append({'profile':profile,'suite_sha256':sha(suite.read_bytes()),'validate_exit':r.returncode,'diagnostics_sha256':sha(r.stdout+r.stderr)});assert r.returncode==0
receipt={'status':'public_dev_integrity_passed_not_independent_gold_review','block':o.block,'public_native_families':len(rows),'source_sha':commit,'license_sha256':lock['license_sha256'],'span_count':span_count,'source_files':7,'upstream_bytes_checked':bool(o.upstream),'snapshot_git_cleanliness_claimed':False,'independently_reviewed':0,'holdout_custody_status':'holdout_custody_blocked','evaluator_sha256':sha(Path(o.evaluator).read_bytes()),'checks':checks,'ranking_observed':False}
(P/'review'/f'{o.block}-source-span-validation.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({'status':receipt['status'],'public_native_families':len(rows),'span_count':span_count,'receipt_sha256':sha((P/'review'/f'{o.block}-source-span-validation.json').read_bytes())}))
