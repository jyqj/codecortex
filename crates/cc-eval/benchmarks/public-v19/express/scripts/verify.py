#!/usr/bin/env python3
"""Read-only span/provenance/contract validation; never runs a search backend."""
import argparse,hashlib,json,subprocess
from pathlib import Path
P=Path(__file__).resolve().parents[1]
def sha(b):return hashlib.sha256(b).hexdigest()
a=argparse.ArgumentParser(); a.add_argument('--block',default='block-020'); a.add_argument('--evaluator',required=True); a.add_argument('--upstream',type=Path); o=a.parse_args()
lock=json.loads((P/'provenance/source-lock.json').read_text()); commit=lock['upstream_lock']['source_sha']
assert sha((P/'license/LICENSE').read_bytes())==lock['license_sha256']
if o.upstream:
    assert subprocess.check_output(['git','-C',str(o.upstream),'rev-parse','HEAD'],text=True).strip()==commit
    assert not subprocess.check_output(['git','-C',str(o.upstream),'status','--porcelain'])
for f in lock['files']:
    b=(P/'source'/f['path']).read_bytes(); assert len(b)==f['bytes'] and sha(b)==f['sha256']
    assert b'MIT Licensed' in b[:250] and b'\0' not in b
    blob=hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest(); assert blob==f['git_blob']
    if o.upstream:assert b==subprocess.check_output(['git','-C',str(o.upstream),'show',commit+':'+f['path']])
block=P/'blocks'/o.block; rows=[json.loads(x) for x in (block/'questions.jsonl').read_text().splitlines()]; inv=json.loads((block/'inventory.json').read_text()); clusters={}; ids=set()
for r in rows:
    assert r['id'] not in ids; ids.add(r['id']); an=r['annotations']; assert an['status']=='candidate_pending_independent_review' and an['reviewer'] is None and not an['retrieval_inspected']
    assert an['source_sha']==commit and an['family_sha256']==sha(r['query_family'].encode())
    ch=sha(an['related_family_cluster'].encode()); assert ch==an['split_hash_sha256']; assert r['split']==('holdout' if int(ch[:16],16)%4==0 else 'dev')
    assert clusters.setdefault(an['related_family_cluster'],r['split'])==r['split']
    for i,e in enumerate(an['source_evidence']):
        b=(P/'source'/e['path']).read_bytes(); ls=b.splitlines(keepends=True)
        start=sum(map(len,ls[:e['line_start']-1])); end=sum(map(len,ls[:e['line_end']]))
        assert start==e['byte_start'] and end==e['byte_end'] and start<end
        assert sha(b[start:end])==e['sha256'] and b[start:end].decode()==e['text'] and e['source_sha']==commit
        if not r['no_answer']:
            alt=r['answers'][i]['alternatives'][0]; assert alt['path']==e['path'] and alt['span']=={'start':start,'end':end} and alt['symbol']['name']==e['symbol']
    for edge in an['chain_edges']:
        assert edge['relation'] and all(0<=i<len(an['source_evidence']) for i in edge['supporting_evidence'])
        for side in ['from','to']:
            endpoint=edge[side]; e=an['source_evidence'][endpoint['evidence_index']]; assert endpoint['path']==e['path'] and endpoint['symbol']==e['symbol']
    if r['category']=='crossfile-chain':assert len({e['path'] for e in an['source_evidence']})>=2
    if r['no_answer']:
        assert not r['answers'] and not r['expected_files']; proof=an['absence_proof']; files=proof['scope_files']
        assert files==sorted(f['path'] for f in lock['files'])
        assert proof['scope_digest_sha256']==sha(json.dumps([(f,sha((P/'source'/f).read_bytes())) for f in files],separators=(',',':')).encode())
        for check in proof['checks']:
            hits=[{'path':f,'line':i,'text':line} for f in files for i,line in enumerate((P/'source'/f).read_text().splitlines(),1) if check['literal'] in line]; assert hits==check['hits']
assert inv['candidate_families']==len(rows) and inv['questions_sha256']==sha((block/'questions.jsonl').read_bytes())
assert inv['family_set_sha256']==sha('\n'.join(sorted(r['query_family'] for r in rows)).encode())
suite=block/'suite.json'; result=subprocess.run([o.evaluator,'validate','--suite',str(suite)],capture_output=True,text=True)
receipt={'status':'candidate_integrity_validation_only','block':o.block,'families':len(rows),'independently_reviewed':0,'source_sha':commit,'license_sha256':lock['license_sha256'],'queries_sha256':inv['questions_sha256'],'family_set_sha256':inv['family_set_sha256'],'suite_sha256':sha(suite.read_bytes()),'evaluator_sha256':sha(Path(o.evaluator).read_bytes()),'evaluator_command':['cc-eval','validate','--suite',str(suite.relative_to(P))],'evaluator_exit':result.returncode,'evaluator_stdout':result.stdout,'evaluator_stderr':result.stderr,'verified_git_blob_ids':True,'upstream_checkout_bytes_verified':bool(o.upstream),'span_checks':'exact line / half-open UTF8 byte / source digest / literal text','ranking_or_provider_used':False,'limits':['Gold semantic correctness and family independence need independent reviewer C.','Chain edges and absence rationale are annotations; native chain scoring not implemented.','All primary groups do not imply conjunctive facet scoring.','Local split is hash based, not a global freeze.']}
(P/'review'/f'{o.block}-validation.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:receipt[k] for k in ['block','families','evaluator_exit','source_sha','license_sha256']},ensure_ascii=False)); raise SystemExit(result.returncode)
