#!/usr/bin/env python3
"""Two source-derived public-dev obligations, reserved serials; no retrieval."""
import copy,hashlib,json
from pathlib import Path
P=Path(__file__).resolve().parents[1]
SHA='7ef98448f8b38099ab1ded55e458538ad47a51e7'
BASE='a739431b6595bb44e79130843e27e2cde2d34eb1'
def sha(b):return hashlib.sha256(b).hexdigest()
def dump(path,obj):path.write_text(json.dumps(obj,ensure_ascii=False,indent=2)+'\n')
def jsonl(rows):return ''.join(json.dumps(r,ensure_ascii=False,separators=(',',':'))+'\n' for r in rows).encode()
def evidence(path,lo,hi,symbol):
 b=(P/'source'/path).read_bytes(); ls=b.splitlines(keepends=True);start=sum(map(len,ls[:lo-1]));end=sum(map(len,ls[:hi]));assert 1<=lo<=hi<=len(ls)
 return {'source_sha':SHA,'path':path,'symbol':symbol,'line_start':lo,'line_end':hi,'byte_start':start,'byte_end':end,'sha256':sha(b[start:end]),'text':b[start:end].decode()}
reservation=json.loads((P/'provenance/component-reservation-101-102.json').read_text());assert reservation['families']==['v19.express.f0101','v19.express.f0102']
# Tasks fixed by source reading before the reserved IDs' split was computed.
specs=[
 {'family':'v19.express.f0101','category':'api_usage','original_category':'exact/API','query':'How does app.use distinguish a nested middleware-array first argument from an explicit path and flatten the callback arguments before registration?',
  'answer':'It starts at path / with offset 0. If the first argument is not a function, it repeatedly inspects the first element of each nonempty array; only a non-function leaf causes offset 1 and path to be the original first argument. It slices arguments from that offset and uses Array.prototype.flat with Infinity before registering each middleware.',
  'ev':[('lib/application.js',190,210,'use'),('lib/application.js',33,34,'flatten')],
  'independence':'Nonempty input shape disambiguation plus recursive array normalization; prior use questions concern empty-callback rejection, child mount inheritance, or prototype restoration, not this normalization obligation.',
  'related_prior_candidates':['v19.express.f0011','v19.express.f0082'],'mutation_note':'changing empty-callback rejection or child mount inheritance does not define the normalization contract'},
 {'family':'v19.express.f0102','category':'error_handling','original_category':'config/error','query':'In res.sendFile transfer completion, how do an explicit callback, EISDIR, ECONNABORTED and write errors affect forwarding to req.next?',
  'answer':'A provided callback receives err first and returns, overriding the default routing. Otherwise EISDIR calls next() without an error. Other errors reach next(err) only if code is not ECONNABORTED and syscall is not write; those two cases are suppressed by this default handler. A successful transfer without a callback does not call next here.',
  'ev':[('lib/response.js',375,380,'sendFile'),('lib/response.js',407,416,'sendFile')],
  'independence':'Asynchronous transfer error routing and explicit callback precedence; prior sendFile questions concern synchronous path validation, boolean etag configuration, or the private sendfile completion-state helper.',
  'related_prior_candidates':['v19.express.f0017','v19.express.f0061','v19.express.f0078'],'mutation_note':'changing the synchronous absolute-path guard or etag switch does not define the transfer error forwarding policy'}
]
native=[]
for s in specs:
 f=s['family'];digest=hashlib.sha256(b'codecortex-public-v19-split-v1\n'+f.encode()).digest();split='holdout' if int.from_bytes(digest[:8],'big')<2**62 else 'dev'
 assert split=='dev','Reserved family is not public dev: stop without persisting any question body'
 ev=[evidence(*e) for e in s['ev']]
 groups=[{'id':'facet-'+str(i),'primary':i==0,'grade':3 if i==0 else 2,'alternatives':[{'path':e['path'],'symbol':{'name':e['symbol'],'qname':None,'kind':None},'span':{'start':e['byte_start'],'end':e['byte_end']}}]} for i,e in enumerate(ev)]
 provenance={'status':'candidate_pending_independent_review','author_role':'B/express','source_sha':SHA,'family_sha256':sha(f.encode()),'answer_rationale':s['answer'],'source_evidence':ev,'chain_edges':[],'reviewer':None,'retrieval_inspected':False,'distinct_obligation':s['independence'],'related_but_not_equivalent_candidate_ids':s['related_prior_candidates'],'mutation_reasoning_not_executed':s['mutation_note']}
 native.append({'id':f+'.en01','category':s['category'],'difficulty':2,'language':'JavaScript','split':split,'query_family':f,'query':s['query'],'path_prefix':None,'no_answer':False,'expected_files':[],'answers':groups,'annotations':{'v19':{'protocol_version':1,'repo_id':'express','source_sha':SHA,'global_family':f,'intent':s['query'],'query_language':'en','hard_scope':{'path_prefix':None},'facets':[{'id':'required-'+str(i),'group_id':g['id'],'required':True} for i,g in enumerate(groups)],'graph_constraints':[],'mutation_profile':'none','review_status':'pending','author_id':'B/express','reviewer_id':None,'original_category':s['original_category'],'primary_designation':'input shape / transfer routing obligation primary grade3; source binding evidence required secondary grade2','author_provenance':provenance}}})
compat=[]
for q in native:
 c=copy.deepcopy(q);c['expected_files']=list(dict.fromkeys(a['path'] for g in c['answers'] for a in g['alternatives']));c['answers']=[];c['annotations']['v19']['facets']=[];compat.append(c)
for label in ['supplement-002','public-dev-102']:
 out=P/'intake'/label;out.mkdir(exist_ok=True)
 for profile,rows in [('native',native),('compat',compat)]:
  previous=(P/'intake/full-100'/f'queries.{profile}.dev.jsonl').read_bytes() if label=='public-dev-102' else b''
  raw=previous+jsonl(rows);(out/f'queries.{profile}.dev.jsonl').write_bytes(raw)
  suite=json.loads((P/'intake/full-100'/f'suite.{profile}.dev.json').read_text());suite['name']=f'V19 Express {label} pending public dev';suite['queries_digest']='REQUIRES_EXPLICIT_AUTHOR_FREEZE';dump(out/f'suite.{profile}.dev.json',suite)
  assert raw.startswith(previous)
receipt={'status':'two_additional_public_dev_components_pending_independent_review','base_commit':BASE,'reservation_commit':'d82607e','reserved_family_ids':reservation['families'],'source_sha':SHA,'new_family_count':2,'new_component_count':2,'new_native_dev':2,'new_compat_dev':2,'new_holdout':0,'total_draft_families':102,'total_candidate_components':100,'total_native_public_dev':70,'total_compat_public_dev':59,'unchanged_quarantined_would_be_holdout':32,'accepted':0,'independently_reviewed':0,'relations_sha256':sha((P/'relations.json').read_bytes()),'old_gold_modified':False,'split_resampled':False,'rank_observed':False,'native_supplement_sha256':sha(jsonl(native)),'compat_supplement_sha256':sha(jsonl(compat)),'family_set_sha256':sha('\n'.join(sorted([f'v19.express.f{i:04d}' for i in range(1,103)])).encode()),'independence_status':'author source-obligation proposal; independent reviewer may merge/quarantine'}
dump(P/'corpus-receipt-supplement.json',receipt)
print(json.dumps({k:receipt[k] for k in ['new_component_count','total_candidate_components','total_native_public_dev','total_compat_public_dev','accepted']}))
