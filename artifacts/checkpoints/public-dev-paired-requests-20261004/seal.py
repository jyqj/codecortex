#!/usr/bin/env python3
"""Aggregate only retained observations; never rerun retrieval or alter scorer output."""
import collections
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile

D=Path(__file__).resolve().parent
def sha(b): return hashlib.sha256(b).hexdigest()
def write(p,x): p.write_text(json.dumps(x,indent=2,sort_keys=True)+'\n')
def main():
    runs=json.loads((D/'runs.json').read_text()); assert len(runs)==4
    aggregate=[]; files={}; archive=[]; cases=[]; schedules={}
    for r in runs:
        root=Path('/workspace/paired-requests-'+r['side']+'-runs')/r['mode']
        rows=[json.loads(l) for l in (root/'normalized.jsonl').read_text().splitlines()]
        costs=[json.loads(l) for l in (root/'costs.jsonl').read_text().splitlines()]
        scores=[json.loads(l) for l in (root/'scores.jsonl').read_text().splitlines()]
        assert len(scores)==len(rows)==len(costs)
        assert r['gate']['status']=='gate_failed' and r['missing']==0 and r['replay_byte_identical']
        queries=[json.loads(l) for l in (root/'queries.jsonl').read_text().splitlines()]
        expected={(q['id'],i) for q in queries for i in range(3)}
        actual=[(v['case_id'],v['repetition']) for v in rows]
        assert len(actual)==len(set(actual)) and set(actual)==expected
        assert actual==[(v['case_id'],v['repetition']) for v in costs]
        schedules[r['side'],r['mode']]=actual
        numeric=collections.Counter(); observed=collections.Counter()
        def costwalk(obj,path=''):
            if isinstance(obj,dict):
                for key,value in obj.items(): costwalk(value,path+'/'+key)
            elif isinstance(obj,list):
                for i,value in enumerate(obj): costwalk(value,path+'/'+str(i))
            elif isinstance(obj,(int,float)) and not isinstance(obj,bool):
                numeric[path]+=obj; observed[path]+=1
        for c in costs: costwalk(c['work'])
        metrics={k:v for k,v in r['metrics'].items() if k!='cases'}
        score_means={key:{'mean':sum(values)/len(values) if values else None,'observed':len(values),'null':len(scores)-len(values)}
            for key in scores[0] for values in [[s[key] for s in scores if s[key] is not None]]}
        item={k:r[k] for k in ['side','mode','source_commit','scheduled','executed','missing','statuses','run_exit','replay_exit','replay_byte_identical','infrastructure_failure','source_digest','query_sha256','suite_sha256']}
        item.update(metrics=metrics,all_recorded_score_means=score_means,gate_status=r['gate']['status'],error_rows=sum(n for s,n in r['statuses'].items() if s not in ['partial','success','no_match']),
                    prepare_stage='success',index_stage='ready',parser_errors=r['prepare']['result']['parse_errors'],
                    cost_numeric_totals=dict(numeric),cost_observation_counts=dict(observed))
        aggregate.append(item)
        diagnostics=collections.Counter()
        for i,(row,cost) in enumerate(zip(rows,costs)):
            payload=json.loads((root/'raw'/f'{i:06d}.json').read_text())
            evidence=payload.get('evidence_summary',{})
            diagnostics['packing_partial_rows']+=evidence.get('packing',{}).get('partial') is True
            diagnostics['source_freshness_partial_rows']+=evidence.get('source_freshness',{}).get('partial') is True
            for h in row.get('hits',[]):
                diagnostics['retained_normalized_hits']+=1
                diagnostics['retained_qname_nonnull_hits']+=h.get('qname') is not None
                diagnostics['retained_method_hits']+=h.get('kind')=='method'
            cases.append({'side':r['side'],'mode':r['mode'],'case_id':row['case_id'],'repetition':row['repetition'],
                          'status':row['status'],'raw_file':f"{r['side']}/{r['mode']}/raw/{i:06d}.json",
                          'diagnostic':row.get('diagnostic'),'retrieval_cost':cost,'scores':scores[i]})
        item['observed_evidence_diagnostics']=dict(diagnostics)
        for f in sorted(root.rglob('*')):
            if f.is_file():
                name=f"{r['side']}/{r['mode']}/"+str(f.relative_to(root)); raw=f.read_bytes()
                assert sha(raw)==r['raw_sha256'][str(f.relative_to(root))]
                files[name]={'sha256':sha(raw),'bytes':len(raw)}; archive.append((name,raw))
    for mode in ('native','compat'): assert schedules['base',mode]==schedules['candidate',mode]
    for f in sorted((D/'retained-licenses').rglob('*')):
        if f.is_file():
            name=str(f.relative_to(D)); raw=f.read_bytes(); files[name]={'sha256':sha(raw),'bytes':len(raw)}; archive.append((name,raw))
    for f in ['original-schedule.json','intake.json','notice-manifest.json','binding-base.json','binding-candidate.json','paired-input-lock.json']:
        raw=(D/f).read_bytes(); files[f]={'sha256':sha(raw),'bytes':len(raw)}; archive.append((f,raw))
    buf=io.BytesIO()
    with gzip.GzipFile(fileobj=buf,mode='wb',mtime=0,filename='') as z:
        with tarfile.open(fileobj=z,mode='w') as t:
            for name,raw in sorted(archive):
                info=tarfile.TarInfo(name); info.size=len(raw); info.mode=0o644; t.addfile(info,io.BytesIO(raw))
    raw=buf.getvalue(); (D/'paired-requests-raw.tar.gz').write_bytes(raw)
    write(D/'raw-artifact-manifest.json',{'schema_version':1,'scope':'Requests paired public DEV complete original outputs with licenses, no full source/binary/DB/holdout','archive_sha256':sha(raw),'files':files})
    with tarfile.open(D/'paired-requests-raw.tar.gz') as t:
        assert set(t.getnames())==set(files)
        for name,pin in files.items():
            b=t.extractfile(name).read(); assert sha(b)==pin['sha256'] and len(b)==pin['bytes']
    (D/'case-stage-cost-score.jsonl').write_text(''.join(json.dumps(c,sort_keys=True)+'\n' for c in cases))
    delta={}
    for mode in ('native','compat'):
        a,b=[r for r in aggregate if r['mode']==mode]
        delta[mode]={k:b['metrics'][k]-a['metrics'][k] for k in ['mean_ndcg10','mean_top1']}
    write(D/'aggregate.json',{'stage':'executed_and_replayed_quality_failed','scope':'public_DEV_Requests_v2_native_and_original_compat','runs':aggregate,'paired_metric_deltas':delta,'performance_claim':False,'certification_claim':False,'clean_holdout':0,'new_independent_samples':0,'archive_reverified':True,'paired_schedule_identical':True})
    print(json.dumps({'runs':[{k:r[k] for k in ['side','mode','scheduled','executed','missing','statuses','gate_status','metrics']} for r in aggregate],'delta':delta},sort_keys=True))
if __name__=='__main__': main()
