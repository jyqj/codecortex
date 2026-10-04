"""Check meaningful end-to-end claims, including controls; no gold tuning."""
import json,hashlib
from pathlib import Path
out=Path(__file__).parent
load=lambda arm,variant:json.loads((out/('micro-'+arm+variant)/'results.json').read_text())
summary={}
for suffix in ['', '-inversion','-receiver']:
 a,b=load('baseline',suffix),load('candidate',suffix)
 for api in ['engine','mcp']:
  for query in a:
   x,y=a[query][api],b[query][api]
   assert all(h['evidence_valid'] for h in x['normalized']+y['normalized'])
   if query in ['Feeder','SeedLabel']:
    assert x['scores']['top1']==0 and y['scores']['top1']==1
   if query=='Pulse':assert x['scores']['top1']==y['scores']['top1']==1
  if suffix=='-receiver':
   q='Which Cedar API changes the ready state with Pulse?';x,y=a[q][api],b[q][api]
   assert x['scores']['top1']==1 and y['scores']['top1']==0
   assert x['scores']['mrr10']==1 and y['scores']['mrr10']==.5
   assert x['normalized'][0]['symbol_name']=='Pulse' and y['normalized'][0]['symbol_name']=='Cedar' and y['normalized'][1]['symbol_name']=='Pulse'
   old=x['raw']['machine_pack']['hits'][0];new=y['raw']['machine_pack']['hits'][1];winner=y['raw']['machine_pack']['hits'][0]
   assert old['rerank_score']==new['rerank_score']
   assert dict(winner['score_trace'])['boost:symbol-exact']==.18
   oldtype=next(h for h in x['raw']['machine_pack']['hits'] if h['chunk_id']==winner['chunk_id'])
   assert oldtype['symbol_name'] is None and winner['symbol_name']=='Cedar'
   assert abs(winner['rerank_score']-oldtype['rerank_score']-.18)<1e-12
   assert oldtype['text']==winner['text'] and oldtype['metadata']['source_evidence']==winner['metadata']['source_evidence']
   summary[api]={'top1':[1,0],'mrr10':[1,.5],'target_score':new['rerank_score'],'new_type_score':winner['rerank_score'],'old_type_score':oldtype['rerank_score'],'type_new_boost':.18,'unchanged_target_span':x['normalized'][0]['span']==y['normalized'][1]['span'],'correct_method_qname_irrelevant_when_unconstrained':True}
summary['checks']='3 paired own fixture variants; engine and MCP; interface/alias correction; literal method control; receiver rank inversion; every returned source verified'
(out/'micro-summary.json').write_text(json.dumps(summary,indent=2)+'\n');print('micro checks pass')
