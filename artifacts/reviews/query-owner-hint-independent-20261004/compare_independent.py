from pathlib import Path
import json,math,hashlib
root=Path('/workspace/review-owner-evidence'); summary={'arms':['base','candidate'],'variants':['base','receiver','inversion'],'query_api_pairs':0,'common_hits':0,'membership_differences':[],'changed_bonuses':[],'observations':[],'top_k_1':[]}
for variant in summary['variants']:
 b=json.loads((root/f'results/base-{variant}/results.json').read_text()); c=json.loads((root/f'results/candidate-{variant}/results.json').read_text())
 assert b['source_text']==c['source_text']
 for q,entry in b['queries'].items():
  for api,before in entry.items():
   after=c['queries'][q][api]; summary['query_api_pairs']+=1
   bh=before['raw']['machine_pack']['hits']; ch=after['raw']['machine_pack']['hits']
   bm={h['chunk_id']:h for h in bh}; cm={h['chunk_id']:h for h in ch}
   if bm.keys()!=cm.keys(): summary['membership_differences'].append([variant,q,api,list(bm),list(cm)])
   bn={(h['path'],h['start_line'],h['end_line'],h['symbol_name'],h['kind']):h for h in before['normalized']}
   cn={(h['path'],h['start_line'],h['end_line'],h['symbol_name'],h['kind']):h for h in after['normalized']}
   assert bn==cn, ('source identity normalization drift',variant,q,api)
   for cid in bm.keys() & cm.keys():
    x,y=bm[cid],cm[cid]; summary['common_hits']+=1
    for field in ['symbol_name','symbol_kind','text','start_line','end_line','file_path','breadcrumb','fused_score','lexical_score','grep_score','graph_score']:
     assert x[field]==y[field],(variant,q,api,cid,field)
    assert [t for t in x['score_trace'] if t[0]!='boost:symbol-exact']==[t for t in y['score_trace'] if t[0]!='boost:symbol-exact']
    for h in [x,y]: assert math.isclose(sum(v for _,v in h['score_trace']),h['rerank_score'],abs_tol=1e-9)
    if x['score_trace']!=y['score_trace']:
     assert x['symbol_name']=='Beacon' and x['symbol_kind']=='class' and 'exact-target' not in x['reasons']
     assert [v for k,v in x['score_trace'] if k=='boost:symbol-exact']==[.18]
     assert not [v for k,v in y['score_trace'] if k=='boost:symbol-exact']
     assert math.isclose(x['rerank_score']-.18,y['rerank_score'],abs_tol=1e-9)
     assert [r for r in x['reasons'] if r!='symbol-exact']==y['reasons']
     summary['changed_bonuses'].append({'variant':variant,'query':q,'api':api,'chunk_id':cid})
    else: assert x['rerank_score']==y['rerank_score'] and x['reasons']==y['reasons']
   summary['observations'].append({'variant':variant,'query':q,'api':api,'before_top':bh[0]['symbol_name'] if bh else None,'after_top':ch[0]['symbol_name'] if ch else None,'before_scores':before.get('scores'),'after_scores':after.get('scores')})
 small=lambda obj: [(h['chunk_id'],h['symbol_name']) for h in obj['top_k_1']['machine_pack']['hits']]
 summary['top_k_1'].append({'variant':variant,'base':small(b),'candidate':small(c)})
summary['input_hashes']={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in root.glob('results/*/results.json') if p.parent.name.startswith(('base-', 'candidate-'))}
(root/'comparison.json').write_text(json.dumps(summary,indent=2))
print({k:v for k,v in summary.items() if k not in ['observations','changed_bonuses','input_hashes']})
print('removed bonuses',len(summary['changed_bonuses']))
for x in summary['observations']:
 if x['query'].startswith('Which'): print(x['variant'],x['api'],x['query'],x['before_top'],'->',x['after_top'],x['before_scores'],x['after_scores'])
