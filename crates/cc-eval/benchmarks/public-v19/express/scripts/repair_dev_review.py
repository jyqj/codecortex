#!/usr/bin/env python3
"""Source-driven repairs of eight explicit PR71 public-dev decisions.
Does not read historical/holdout bodies or run ranking. Writes versioned files.
"""
import copy,hashlib,json
from pathlib import Path
P=Path(__file__).resolve().parents[1]
BASE='0850acc4fdacde2609c56cf733b7776e5027ceff'; REVIEW='24affebcaab1caa9acd08841233867c89931dbf3'; SHA='7ef98448f8b38099ab1ded55e458538ad47a51e7'
def sha(b):return hashlib.sha256(b).hexdigest()
def dump(path,obj):path.write_text(json.dumps(obj,ensure_ascii=False,indent=2)+'\n')
def line(q):return (json.dumps(q,ensure_ascii=False,separators=(',',':'))+'\n').encode()
def ev(path,lo,hi,symbol):
 b=(P/'source'/path).read_bytes();ls=b.splitlines(keepends=True);assert 1<=lo<=hi<=len(ls);start=sum(map(len,ls[:lo-1]));end=sum(map(len,ls[:hi]))
 return {'source_sha':SHA,'path':path,'symbol':symbol,'line_start':lo,'line_end':hi,'byte_start':start,'byte_end':end,'sha256':sha(b[start:end]),'text':b[start:end].decode()}
def add_evidence(q,e):
 a=q['annotations']['v19']; es=a['author_provenance']['source_evidence'];i=len(es);es.append(e);gid='facet-'+str(i)
 q['answers'].append({'id':gid,'primary':False,'grade':2,'alternatives':[{'path':e['path'],'symbol':{'name':e['symbol'],'qname':None,'kind':None},'span':{'start':e['byte_start'],'end':e['byte_end']}}]})
 a['facets'].append({'id':'required-'+str(i),'group_id':gid,'required':True});return i
def internal(q,i):
 e=q['annotations']['v19']['author_provenance']['source_evidence'][i];return {'kind':'internal','evidence_index':i,'path':e['path'],'symbol':e['symbol']}
def edge(q,src,dst,relation,kind,callsite,extra=()):
 support=sorted(set([src]+([dst['evidence_index']] if 'evidence_index' in dst else [])+list(extra)))
 return {'relation':relation,'edge_kind':kind,'from':internal(q,src),'to':dst,'supporting_evidence':support,'callsite_evidence':callsite}
def external(module,symbol,binding,target):return {'kind':'external_boundary','module':module,'symbol':symbol,'target_expression':target,'implementation_admitted':False,'identity_status':'source import/reference only; excluded implementation not inspected','binding_evidence':[binding]}
A='lib/application.js';E='lib/express.js';Q='lib/request.js';R='lib/response.js'
reviews=[]
for name in ['dev-068-review.json','dev-002-delta-review.json']:reviews+=json.loads((P/'review/independent-pr71'/name).read_text())['rows']
issues={r['family_id_sha256']:r for r in reviews if r['decision']=='needschange'};assert len(issues)==8
oldnative=(P/'intake/public-dev-102/queries.native.dev.jsonl').read_bytes();oldcompat=(P/'intake/public-dev-102/queries.compat.dev.jsonl').read_bytes()
newrows={}; native_lines=[]; mappings=[]
for raw in oldnative.splitlines(keepends=True):
 q=json.loads(raw); f=q['query_family']; issue=issues.get(sha(f.encode()))
 if not issue:native_lines.append(raw);newrows[q['id']]=q;continue
 assert sha(raw)==issue['row_sha256'],f
 original=copy.deepcopy(q);a=q['annotations']['v19'];p=a['author_provenance'];es=p['source_evidence'];reason=''
 if f=='v19.express.f0063':
  es[1]=ev(A,20,20,'methods');alt=q['answers'][1]['alternatives'][0];alt['span']={'start':es[1]['byte_start'],'end':es[1]['byte_end']}
  reason='Correct methods import citation from compileETag line21 to actual methods binding line20; same answer obligation.'
 elif f=='v19.express.f0065':
  edges=[edge(q,0,internal(q,1),'callable app invokes app.handle','call_internal',ev(E,37,39,'createApplication')),
   edge(q,1,external('router','router.handle',ev(A,26,26,'Router'),'this.router.handle(req, res, done)'),'handle dispatches to external router.handle','call_external',ev(A,177,177,'handle'))]
  a['graph_constraints']=edges;p['chain_edges']=copy.deepcopy(edges);reason='External router.handle is a excluded dependency boundary, never an internal handle self-edge.'
 elif f=='v19.express.f0068':
  edges=copy.deepcopy(p['chain_edges']);target={'kind':'dynamic_callback','symbol':'configured query parser fn','binding_expression':"this.app.get('query parser fn')",'target_expression':'queryparse(querystring)','resolution':'configuration-dependent; supplied custom functions are not necessarily admitted source','implementation_admitted':'not_proven_for_all_configurations','binding_evidence':[ev(Q,230,243,'query')]}
  edges[2]=edge(q,2,target,'query invokes dynamically configured parser','call_dynamic',ev(Q,240,242,'query'))
  a['graph_constraints']=edges;p['chain_edges']=copy.deepcopy(edges);reason='Represent queryparse as a dynamic configured target, not the query getter itself; no claim of an included parser implementation for arbitrary configuration.'
 elif f=='v19.express.f0069':
  get_i=add_evidence(q,ev(R,701,703,'get'));status_i=add_evidence(q,ev(R,65,77,'status'))
  edges=[edge(q,0,internal(q,1),'send reads req.fresh','getter_read',ev(R,195,196,'send')),
   edge(q,1,internal(q,get_i),'fresh calls res.get for ETag and Last-Modified','call_internal',ev(Q,479,482,'fresh')),
   edge(q,0,internal(q,status_i),'send calls status(304) when request is fresh','call_internal',ev(R,195,196,'send'))]
  a['graph_constraints']=edges;p['chain_edges']=copy.deepcopy(edges);reason='Bind header read to actual res.get declaration and status change to res.status, with required-secondary exact source spans.'
 elif f=='v19.express.f0077':
  edges=copy.deepcopy(p['chain_edges']);edges[2]=edge(q,2,external('router','router.route',ev(A,26,26,'Router'),'this.router.route(path)'),'app.route calls external router.route','call_external',ev(A,256,258,'route'))
  a['graph_constraints']=edges;p['chain_edges']=copy.deepcopy(edges);a['global_family']='v19.express.f0063'
  reason='Represent router.route as excluded external target; conservatively propose same-task component with f0063 because both include Node methods -> lowercased list -> generated app verb binding. No task removed or ID/split changed.'
 elif f=='v19.express.f0079':
  edges=[edge(q,0,internal(q,1),'app.set calls compileTrust','call_internal',ev(A,370,371,'set')),
   edge(q,0,internal(q,2),'app.set stores trust proxy fn; ip reads and passes the configured value','data_flow',ev(Q,340,343,'ip'),extra=(1,)),
   edge(q,2,external('proxy-addr','proxyaddr',ev(Q,23,23,'proxyaddr'),'proxyaddr(this, trust)'),'ip calls external proxyaddr with the trust argument','call_external',ev(Q,340,343,'ip'))]
  a['graph_constraints']=edges;p['chain_edges']=copy.deepcopy(edges);reason='External proxyaddr target is distinct from ip; trust is a stored/read argument data dependency, not a direct ip -> app.set call.'
 elif f=='v19.express.f0080':
  get_i=add_evidence(q,ev(A,471,482,'methods.forEach'));set_i=add_evidence(q,ev(A,351,355,'set'))
  get_target=internal(q,get_i);get_target.update(kind='dynamic_internal_member',symbol='app.get',declared_source_symbol='methods.forEach',member='get',definition_expression='app[method]',resolution_condition="method === 'get' && arguments.length === 1",resolved_target='generated app.get(setting) wrapper')
  edges=[edge(q,0,internal(q,1),'callable app invokes handle with next argument','call_internal',ev(E,37,39,'createApplication')),
   edge(q,1,external('finalhandler','finalhandler',ev(A,16,16,'finalhandler'),'finalhandler(req, res, options)'),'handle constructs external finalhandler if callback is absent','call_external',ev(A,152,157,'handle')),
   edge(q,1,internal(q,2),'handle registers bound logerror as onerror callback','callback_registration',ev(A,154,157,'handle')),
   edge(q,2,get_target,'logerror calls generated app.get(env)','call_internal_dynamic_member',ev(A,615,618,'logerror')),
   edge(q,get_i,internal(q,set_i),'single-argument app.get delegates to app.set for settings lookup','call_internal',ev(A,471,476,'methods.forEach'))]
  a['graph_constraints']=edges;p['chain_edges']=copy.deepcopy(edges);reason='Resolve logerror env read to generated app.get then app.set, not handle; distinguish finalhandler external call from logerror callback registration.'
 elif f=='v19.express.f0102':
  q['answers'][0].update(primary=False,grade=2);q['answers'][1].update(primary=True,grade=3)
  a['primary_designation']='facet-1 transfer error routing branch is primary grade3; facet-0 req.next/callback setup is required secondary grade2'
  reason='Make the actually requested transfer-completion behavioral branch primary; binding/setup remains required secondary. Query, rationale, spans and facts unchanged.'
 else:raise AssertionError(f)
 p['author_revision']={'version':'dev-repair-v1','independent_review_commit':REVIEW,'original_review_error_codes':issue['error_codes'],'supersedes_native_row_sha256':sha(raw),'status':'pending_same_independent_reviewer_recheck','reason':reason,'ranking_observed':False}
 a['review_status']='pending'
 new=line(q);native_lines.append(new);newrows[q['id']]=q
 mappings.append({'family':f,'id':q['id'],'review_commit':REVIEW,'reviewer_id':'independent-source-review/cloud-express-dev-review','original_error_codes':issue['error_codes'],'old_native_row_sha256':sha(raw),'new_native_row_sha256':sha(new),'old_component':original['annotations']['v19']['global_family'],'new_proposed_component':a['global_family'],'split':q['split'],'query_semantics_changed':False,'scope':'public_dev_only','reason':reason,'independent_recheck_status':'pending'})
assert len(mappings)==8
newcompat=[]
for raw in oldcompat.splitlines(keepends=True):
 c=json.loads(raw);q=newrows[c['id']]
 if sha(c['query_family'].encode()) not in issues:newcompat.append(raw);continue
 replacement=copy.deepcopy(q);replacement['expected_files']=list(dict.fromkeys(alt['path'] for g in sorted(q['answers'],key=lambda g:not g['primary']) for alt in g['alternatives']));replacement['answers']=[];replacement['annotations']['v19']['facets']=[];replacement['annotations']['v19']['graph_constraints']=[]
 assert replacement['expected_files']==c['expected_files'];new=line(replacement);newcompat.append(new)
 m=next(m for m in mappings if m['id']==q['id']);m.update(old_compat_row_sha256=sha(raw),new_compat_row_sha256=sha(new),compat_expected_paths_unchanged=True)
relations=json.loads((P/'relations.json').read_text());relations['status']='candidate_local_associations_pending_same_independent_reviewer_and_global_adjudication';relations['components'].append({'global_family':'v19.express.f0063','members':['v19.express.f0063','v19.express.f0077']});dump(P/'relations.dev-repair-v1.json',relations)
out=P/'intake/dev-repair-v1';out.mkdir(exist_ok=True)
for profile,data in [('native',b''.join(native_lines)),('compat',b''.join(newcompat))]:
 (out/f'queries.{profile}.dev.jsonl').write_bytes(data);suite=json.loads((P/'intake/public-dev-102'/f'suite.{profile}.dev.json').read_text());suite['name']='V19 Express public dev repair-v1 pending independent recheck';suite['queries_digest']='REQUIRES_EXPLICIT_VERSIONED_AUTHOR_FREEZE';dump(out/f'suite.{profile}.dev.json',suite)
dump(P/'review/dev-repair-v1-row-mapping.json',{'base_author_commit':BASE,'independent_review_commit':REVIEW,'version':'dev-repair-v1','changed_native_rows':8,'unchanged_native_rows':62,'native_rows':70,'compat_rows':59,'scope':'public_dev_only; no historical/holdout bodies read','author_self_accepted':False,'rank_observed':False,'old_files_unchanged':True,'mappings':mappings,'old_native_file_sha256':sha(oldnative),'new_native_file_sha256':sha(b''.join(native_lines)),'old_compat_file_sha256':sha(oldcompat),'new_compat_file_sha256':sha(b''.join(newcompat))})
dump(P/'review/dev-repair-v1-component-proposal.json',{'status':'pending_independent_global_adjudication','review_commit':REVIEW,'pair':['v19.express.f0063','v19.express.f0077'],'proposed_canonical':'v19.express.f0063','reason':'Source obligation overlap from Node method list through the generated app verb binding. The longer chain retains extra route delegation evidence but does not establish a separate independent component. Conservative clustering pending designated adjudication.','old_components':['v19.express.f0063','v19.express.f0077'],'old_split':['dev','dev'],'new_candidate_split':'dev','cross_split_move':False,'old_relations_sha256':sha((P/'relations.json').read_bytes()),'new_relations_sha256':sha((P/'relations.dev-repair-v1.json').read_bytes()),'proposed_total_components':99,'proposed_visible_dev_components':67,'quarantined_holdout_components_unchanged':32,'old_components_not_deleted':True,'accepted':False})
print(json.dumps({'changed_native_rows':8,'unchanged_native_rows':62,'native_dev':70,'compat_dev':59,'proposed_dev_components':67,'independent_recheck':'pending'}))
