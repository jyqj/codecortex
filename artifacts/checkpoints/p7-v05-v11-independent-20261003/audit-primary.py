#!/usr/bin/env python3
"""Review retained primary JSON without reusing production scope predicates."""
import copy,hashlib,json
from pathlib import Path
HERE=Path(__file__).resolve().parent
REPO=HERE.parents[2]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
write=lambda p,v:p.write_text(json.dumps(v,indent=2,sort_keys=True)+'\n')
V05=REPO/'artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003'
V11=REPO/'artifacts/benchmarks/p7-v11-715ab33-ff968c63-generation-v1-20261002'
docs={'scope/needle.py':'def needle():\n    return 731\n','scope/sub/needle.py':'def needle():\n    return 732\n','scope/needle.rs':'pub fn needle() -> u32 { 733 }\n','outside/needle.py':'def needle():\n    return 999\n'}
gold={'needle v18_live_scope path:scope/ lang:python':{'scope/needle.py','scope/sub/needle.py'},'needle v18_live_scope path:scope/ lang:rust':{'scope/needle.rs'},'needle v18_live_scope path:scope/ path:scope/sub/ lang:python':{'scope/sub/needle.py'},'needle v18_live_scope path:scope/ path:outside/':set(),'needle v18_live_scope lang:python lang:rust':set(),'needle v18_live_scope path:missing/ lang:python':set()}
checks={};logs=[]
for root in [V05,V11]:
 for row in json.loads((root/'commands.json').read_text()):
  for name,digest in row['raw_files'].items():
   p=root/'raw'/f"{row['round']:02}"/name
   assert sha(p)==digest
   checks[str(p.relative_to(REPO))]=digest
  log=Path(row['local_log']);logs.append({'path':str(log),'exists_in_review_workspace':log.is_file(),'sha_verified':sha(log)==row['log_sha256'] if log.is_file() else None})
for name,digest in json.loads((V05/'manifest.json').read_text())['raw_file_sha256'].items():
 p=V05/'raw'/name
 assert sha(p)==digest
 checks[str(p.relative_to(REPO))]=digest
nonempty=empty=0;omissions=[];witness=None
for p in sorted((V05/'raw').glob('**/*dsl*.json')):
 record=json.loads(p.read_text())
 for call in record['calls']:
  if call.get('kind')!='call' or call['tool'] not in ['search','context']:continue
  args=call['args'];query=args.get('query',args.get('task'))
  expected=gold[query];value=call['result'];hits=value['machine_pack']['hits'];actual={h['file_path'] for h in hits}
  assert actual<=expected
  assert all(h['text'] in docs[h['file_path']] for h in hits)
  assert all(s['file_path'] in expected for s in value['spans'])
  if expected:
   nonempty+=1;assert hits
   if actual!=expected:omissions.append({'file':str(p.relative_to(REPO)),'tool':call['tool'],'strategy':args['retrieval_strategy'],'missing':sorted(expected-actual)})
   if len(expected)==2 and len(actual)==2 and args['retrieval_strategy']=='local' and witness is None:witness=(p,call,expected)
  else:empty+=1;assert not hits and not value['spans']
assert nonempty+empty==180
p,call,expected=witness
original=call['result']
def primary_subset_predicate(value):
 hits=value['machine_pack']['hits']
 return bool(hits) and all(h['file_path'] in expected and h['text'] in docs[h['file_path']] for h in hits) and all(s['file_path'] in expected for s in value['spans'])
omitted=copy.deepcopy(original);kept=omitted['machine_pack']['hits'][0]['file_path'];omitted['machine_pack']['hits']=[h for h in omitted['machine_pack']['hits'] if h['file_path']==kept];omitted['spans']=[s for s in omitted['spans'] if s['file_path']==kept]
empty_text=copy.deepcopy(original)
for h in empty_text['machine_pack']['hits']:h['text']=''
assert primary_subset_predicate(omitted) and primary_subset_predicate(empty_text)
assert {h['file_path'] for h in omitted['machine_pack']['hits']}!=expected
assert all(not h['text'] for h in empty_text['machine_pack']['hits'])
write(HERE/'counterfactual-proof-gap.json',{'witness_primary_raw':str(p.relative_to(REPO)),'witness_sha256':sha(p),'literal_gold':sorted(expected),'original':original,'omitted_legal_path':omitted,'empty_rendered_text':empty_text,'primary_subset_predicate_accepts_both':True,'independent_exact_domain_and_nonempty_byte_oracle_rejects_both':True,'synthetic_payload_mutation_only':True,'real_product_bug_claim':False})
conflicts=[]
for p in sorted((V11/'raw').glob('**/*generation*.json')):
 r=json.loads(p.read_text());e=next(c for c in r['calls'] if c['kind']=='generation_conflict');assert e['before']!=e['after'];assert e['provider_attempts']==1 and e['reader_pool']==1;assert e['wire']['code']==-32603 and e['wire']['data']['retryable'] is True
 conflicts.append({'file':str(p.relative_to(REPO)),'before':e['before'],'after':e['after'],'wire':e['wire'],'query_calls':r['query_calls']})
assert len(conflicts)==10
bm=[]
for p in sorted((V05/'raw').glob('**/bm25.json')):
 r=json.loads(p.read_text());assert r['strong_contribution']>r['weak_contribution']>0;bm.append(r)
assert len(bm)==5
write(HERE/'primary-receipt-audit.json',{'PR46':'4ecfb02b41bdde08db38f595b817f3a84a8c73f4','PR49':'1a6f60264933a61ba08a5150ebab1b7a13164a38','verified_unique_primary_raw_files':len(checks),'verified_sha256':checks,'dsl_observations':180,'nonempty_cases':nonempty,'empty_cases':empty,'observed_missing_legal_paths':omissions,'actual_generation_conflicts':conflicts,'bm25_observations':bm,'primary_external_log_access':logs,'counterfactuals':'proof gaps only; original observed product outputs are not mutated','complete_V05':False,'complete_V11':False})
print({'primary_raw_files':len(checks),'dsl_cases':180,'generation_conflicts':len(conflicts),'BM25_samples':len(bm),'observed_literal_omissions':len(omissions)})
