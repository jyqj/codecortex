#!/usr/bin/env python3
"""Read current public dev only; publish opaque per-row checks/hashes, never gold."""
import ast,hashlib,json,pathlib,subprocess
HERE=pathlib.Path(__file__).resolve().parent
ROOT=HERE.parents[5]
AUTHOR=ROOT/'crates/cc-eval/benchmarks/public-v19/requests'
BASE='e49f9ada5826b4206d2128f3bfb8d31603ff42fa'
UPSTREAM='611c6162cbc4ac2020a2f91c7cfa4f3abf9bbb60'
PROTOCOL='03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6'
sha=lambda data:hashlib.sha256(data).hexdigest()
canonical=lambda value:json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
read_paths={}
def read(path):
 p=AUTHOR/path;data=p.read_bytes()
 assert data==subprocess.check_output(['git','show',BASE+':'+str(p.relative_to(ROOT))],cwd=ROOT),path
 read_paths[path]=sha(data)
 return data

native=[json.loads(line) for line in read('queries.native.dev.jsonl').splitlines()]
compat=[json.loads(line) for line in read('queries.compat.dev.jsonl').splitlines()]
gold=json.loads(read('gold/dev.json'))
assert len(native)==len(gold)==91 and len(compat)==83
assert all(r['split']=='dev' for r in native+compat) and all(r['split']=='dev' for r in gold)
gold_by_id={g['family_id']:g for g in gold};compat_by_id={q['id']:q for q in compat}
selected=[q for q in native if q['query_family']=='v19.requests.f0022']+native[75:];assert len(selected)==17;selected_ids={r['id'] for r in selected}
manifest=json.loads(read('source-manifest.json'))
assert manifest['source_sha']==UPSTREAM
license_review=json.loads(read('license/inclusion-review.json'))
assert len(license_review['adapted_third_party_helpers'])==3
assert license_review['status']=='public_BSD_lineage_and_full_notices_verified_pending_independent_acceptance'
independent_license=json.loads((HERE/'delta-license-review.json').read_bytes());assert independent_license['author_sha']==BASE and independent_license['independent_license_evidence']=='accept_with_full_BSD_notice_retention'
blocked=[]
for helper in license_review['adapted_third_party_helpers']:
 data=read('source/'+helper['path']);segment=data[helper['start_byte']:helper['end_byte']]
 assert sha(segment)==helper['sha256'] and not helper['gold_admission'] and not helper['index_byte_exclusion_supported']
 blocked.append((helper['path'],helper['start_byte'],helper['end_byte']))
for entry in manifest['admitted']:
 path=entry['path'];folder='license' if path in ['LICENSE','NOTICE'] else 'source';data=read(folder+'/'+path)
 assert len(data)==entry['bytes'] and sha(data)==entry['sha256']
assert sha(read('license/LICENSE'))=='09e8a9bcec8067104652c168685ab0931e7868f9c8284b66f5ae6edae5f1130b'
protocol_receipt=json.loads(read('provenance/protocol/receipt.json'))
assert protocol_receipt['commit']==PROTOCOL
for path,digest in protocol_receipt['files'].items():assert sha(read('provenance/protocol/'+path))==digest
relations=json.loads(read('relations.json'))
components={member:c['global_family'] for c in relations['components'] for member in c['members']}
assert len(components)==116 and len(relations['components'])==102

modules={};symbols={}
def module(path):
 if path not in modules:
  raw=read('source/'+path);tree=ast.parse(raw,filename=path);modules[path]=(raw,tree)
  def walk(nodes,parents=()):
   for node in nodes:
    if isinstance(node,(ast.FunctionDef,ast.AsyncFunctionDef,ast.ClassDef)):
     qualified='.'.join((*parents,node.name));symbols.setdefault((path,qualified),[]).append(node)
     if isinstance(node,ast.ClassDef):walk(node.body,(*parents,node.name))
    elif isinstance(node,(ast.If,ast.Try,ast.With)):
     walk(node.body,parents)
     walk(getattr(node,'orelse',[]),parents)
     walk(getattr(node,'finalbody',[]),parents)
     for handler in getattr(node,'handlers',[]):walk(handler.body,parents)
  walk(tree.body)
 return modules[path]

def offsets(raw,node):
 lines=raw.splitlines(keepends=True)
 return sum(map(len,lines[:node.lineno-1]))+node.col_offset,sum(map(len,lines[:node.end_lineno-1]))+node.end_col_offset

rows=[]
for q in selected:
 g=gold_by_id[q['query_family']];assert g['source_sha']==UPSTREAM and g['global_family']==q['annotations']['v19']['global_family']==components[q['query_family']]
 assert q['annotations']['v19']['source_sha']==UPSTREAM and q['annotations']['v19']['review_status']=='pending'
 grouped=q['answers'];evidence=g['evidence'];evidence_keys=set();files=set()
 for e in evidence:
  raw,tree=module(e['path']);files.add(e['path'])
  assert sha(raw)==e['file_sha256'] and sha(raw[e['start_byte']:e['end_byte']])==e['span_sha256'] and e['source_sha']==UPSTREAM
  candidates=symbols[(e['path'],e['symbol'])]
  candidates=[n for n in candidates if not any(isinstance(dec,ast.Name) and dec.id=='overload' for dec in getattr(n,'decorator_list',[]))]
  exact=[n for n in candidates if offsets(raw,n)==(e['start_byte'],e['end_byte'])]
  assert len(exact)==1 and (exact[0].lineno,exact[0].end_lineno)==(e['start_line'],e['end_line'])
  assert not any(path==e['path'] and max(start,e['start_byte'])<min(end,e['end_byte']) for path,start,end in blocked)
  evidence_keys.add((e['path'],e['start_byte'],e['end_byte'],e['symbol'].split('.')[-1]))
 if q['no_answer']:
  assert q['answers']==q['expected_files']==[] and q['id'] not in compat_by_id
  assert g['scope'] and g['absence']
 else:
  assert q['expected_files']==[] and grouped and sum(bool(a['primary']) for a in grouped)==1
  actual_keys=set();expected_files=[]
  for a in grouped:
   assert a['grade']==(3 if a['primary'] else 2) and len(a['alternatives'])==1
   for alternative in a['alternatives']:
    span=alternative['span'];identity=alternative['symbol'];actual_keys.add((alternative['path'],span['start'],span['end'],identity['name']))
    assert identity['qname']=='requests.'+pathlib.Path(alternative['path']).stem+'.'+next(e['symbol'] for e in evidence if e['path']==alternative['path'] and e['start_byte']==span['start'])
    ev=next(e for e in evidence if e['path']==alternative['path'] and e['start_byte']==span['start'])
    target=next(n for n in symbols[(ev['path'],ev['symbol'])] if offsets(module(ev['path'])[0],n)==(span['start'],span['end']))
    assert identity['kind']==('class' if isinstance(target,ast.ClassDef) else 'function')
    if alternative['path'] not in expected_files:expected_files.append(alternative['path'])
  assert actual_keys==evidence_keys
  c=compat_by_id[q['id']];assert c['query']==q['query'] and c['expected_files']==expected_files and not c['no_answer'] and not c['answers']
  assert c['query_family']==q['query_family'] and c['annotations']['v19']['global_family']==g['global_family']
  assert [f['group_id'] for f in q['annotations']['v19']['facets']]==[a['id'] for a in grouped]
  assert all(f['required'] for f in q['annotations']['v19']['facets'])
 for edge in g['edges']:
  raw,tree=module(edge['path']);s,t=edge['start_byte'],edge['end_byte'];assert raw[s:t].decode()==edge['expression']
  exact=[n for n in ast.walk(tree) if isinstance(n,ast.Call) and offsets(raw,n)[0]==s and (offsets(raw,n)[1]==t or (t<offsets(raw,n)[1] and edge['expression']==ast.unparse(n.func)+'('))]
  if not exact:
   wrappers=[n for n in ast.walk(tree) if isinstance(n,ast.Assign) and offsets(raw,n)==(s,t) and isinstance(n.value,ast.Call)]
   exact=[n.value for n in wrappers]
  if not exact:
   wrappers=[n for n in ast.walk(tree) if isinstance(n,ast.BoolOp) and isinstance(n.op,ast.Or) and offsets(raw,n)==(s,t) and isinstance(n.values[0],ast.Call) and all(not any(isinstance(x,ast.Call) for x in ast.walk(v)) for v in n.values[1:])]
   exact=[n.values[0] for n in wrappers]
  assert len(exact)==1 and exact[0].lineno==edge['line'] and edge['source_sha']==UPSTREAM
  caller_module,caller_symbol=edge['caller'].split(':',1);callee_module,callee_symbol=edge['callee'].split(':',1)
  caller=next(e for e in evidence if pathlib.Path(e['path']).stem==caller_module and e['symbol']==caller_symbol)
  callee=next(e for e in evidence if pathlib.Path(e['path']).stem==callee_module and e['symbol']==callee_symbol)
  assert caller['start_byte']<=s<t<=caller['end_byte']
  call=exact[0];leaf=callee_symbol.split('.')[-1]
  if isinstance(call.func,ast.Name) and call.func.id==leaf:
   if caller_module!=callee_module:
    imports=[n for n in tree.body if isinstance(n,ast.ImportFrom) and n.module==callee_module]
    assert any(any(a.name==leaf and a.asname is None for a in n.names) for n in imports)
  elif isinstance(call.func,ast.Attribute):
   assert call.func.attr==leaf
   # New challenge-replay edges follow the declared Response parameter and
   # its PreparedRequest attribute. Verify annotations/imports in the actual
   # source; certify declared base-class flow, not arbitrary subclass dispatch.
   klass=callee_symbol.rsplit('.',1)[0]
   caller_node=next(n for n in symbols[(caller['path'],caller['symbol'])] if offsets(raw,n)==(caller['start_byte'],caller['end_byte']))
   receiver=call.func.value
   def imported_class(name,target_module):
    return any(isinstance(n,ast.ImportFrom) and n.module==target_module and any(a.name==name and a.asname is None for a in n.names) for n in ast.walk(tree))
   if isinstance(receiver,ast.Name):
    assert any(a.arg==receiver.id and isinstance(a.annotation,ast.Name) and a.annotation.id==klass for a in caller_node.args.args)
    assert imported_class(klass,callee_module)
   elif isinstance(receiver,ast.Attribute) and isinstance(receiver.value,ast.Name):
    argument=next(a for a in caller_node.args.args if a.arg==receiver.value.id)
    assert isinstance(argument.annotation,ast.Name)
    parent_class=argument.annotation.id;assert imported_class(parent_class,callee_module)
    parent_node=next(n for n in symbols[(callee['path'],parent_class)] if isinstance(n,ast.ClassDef))
    assert any(isinstance(n,ast.AnnAssign) and isinstance(n.target,ast.Name) and n.target.id==receiver.attr and isinstance(n.annotation,ast.Name) and n.annotation.id==klass for n in parent_node.body)
   else:raise AssertionError('unreviewed declared receiver binding')
  elif isinstance(call.func,ast.Name) and leaf=='__call__':
   klass=callee_symbol.rsplit('.',1)[0];receiver=call.func.id
   caller_node=next(n for n in symbols[(caller['path'],caller['symbol'])] if offsets(raw,n)==(caller['start_byte'],caller['end_byte']))
   constructors=[n for n in ast.walk(caller_node) if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id==receiver for t in n.targets) and isinstance(n.value,ast.Call) and isinstance(n.value.func,ast.Name) and n.value.func.id==klass and n.lineno<edge['line']]
   assert constructors, 'concrete callable branch must be constructor-bound before dispatch'
   # Only the query's explicit constructor branch is certified, not arbitrary
   # dynamic callables or monkey-patched modules.
  else:
   raise AssertionError('unreviewed static edge binding')
 expected_edges=[{'direction':'caller_to_callee','caller':edge['caller'],'callee':edge['callee'],'kind':edge['kind'],'evidence':{'path':edge['path'],'span':{'start':edge['start_byte'],'end':edge['end_byte']}}} for edge in g['edges']]
 assert q['annotations']['v19']['graph_constraints']==expected_edges
 reason_codes=['SOURCE_FACTS_MANUALLY_REVIEWED','AST_SPANS_AND_HASHES_VERIFIED','NATIVE_COMPAT_FACET_CONTRACT_VERIFIED','NO_RANKING_OBSERVED']
 decision='accept'
 if q['query_family']=='v19.requests.f0022':reason_codes.append('PRIOR_SCOPE_DISPUTE_RESOLVED_FOR_REVISED_INPUT_DOMAIN')
 if g['edges']:reason_codes.append('DECLARED_BASE_CLASS_EDGE_BINDING_VERIFIED')
 rows.append({'query_id':q['id'],'family_id':q['query_family'],'global_component':g['global_family'],'decision':decision,'decision_scope':'current_public_dev_source_content_only','reason_codes':reason_codes,'question_sha256':sha(canonical(q)),'gold_record_sha256':sha(canonical(g)),'evidence_records_checked':len(evidence),'chain_edges_checked':len(g['edges']),'required_facets_checked':len(q['annotations']['v19']['facets']),'bounded_no_answer':q['no_answer'],'suite_admission':'pending_global_component_and_integrator_admission','reviewer_id':'requests-independent-dev-reviewer/root','author_id':q['annotations']['v19']['author_id']})

# Carry forward only independently frozen public-dev decisions after exact hash
# comparisons, without opening any author-held prior gold/specification file.
prior=[]
for receipt in ['dev20-review.json','block02-review.json','block03-review.json','block04-review.json']:
 prior.extend(json.loads((HERE/receipt).read_bytes())['rows'])
assert len(prior)==75 and [r['query_id'] for r in prior]==[q['id'] for q in native[:75]]
old_by_id={r['query_id']:r for r in prior};unchanged=0
for q in native[:75]:
 old=old_by_id[q['id']];g=gold_by_id[q['query_family']]
 assert components[q['query_family']]==old['global_component']
 if q['query_family']=='v19.requests.f0022':
  assert sha(canonical(q))!=old['question_sha256'] and sha(canonical(g))!=old['gold_record_sha256'] and old['decision']=='needs_change'
 else:
  assert sha(canonical(q))==old['question_sha256'] and sha(canonical(g))==old['gold_record_sha256'] and old['decision']=='accept';unchanged+=1
assert unchanged==74
# Source manifest and every indexed byte remain bound to the old official read.
previous=json.loads((HERE/'block04-review.json').read_bytes())['read_allowlist_sha256']
for path,digest in read_paths.copy().items():
 if path.startswith('source/') or path in ['license/LICENSE','license/NOTICE']:
  assert previous[path]==digest
# Independently model only the revised input domain. Never execute/import corpus.
q=next(q for q in selected if q['query_family']=='v19.requests.f0022');e=gold_by_id[q['query_family']]['evidence'][0]
raw,tree=module(e['path']);node=next(n for n in symbols[(e['path'],e['symbol'])] if offsets(raw,n)==(e['start_byte'],e['end_byte']))
condition=next(n.test for n in node.body if isinstance(n,ast.If))
def interpret(n,value):
 if isinstance(n,ast.Constant):return n.value
 if isinstance(n,ast.Name):return {'str':str,'bytes':bytes,'isinstance':isinstance}.get(n.id,value)
 if isinstance(n,ast.Tuple):return tuple(interpret(x,value) for x in n.elts)
 if isinstance(n,ast.UnaryOp) and isinstance(n.op,ast.USub):return -interpret(n.operand,value)
 if isinstance(n,ast.Subscript):return interpret(n.value,value)[interpret(n.slice,value)]
 if isinstance(n,ast.Call):
  assert isinstance(n.func,ast.Name) and n.func.id=='isinstance' and len(n.args)==2 and not n.keywords
  return isinstance(interpret(n.args[0],value),interpret(n.args[1],value))
 if isinstance(n,ast.BoolOp):
  assert isinstance(n.op,ast.And);return all(interpret(x,value) for x in n.values)
 if isinstance(n,ast.Compare):
  assert len(n.ops)==len(n.comparators)==1 and isinstance(n.ops[0],ast.NotEq)
  return interpret(n.left,value)!=interpret(n.comparators[0],value)
 raise AssertionError('unexpected source predicate shape')
cases=['boundary','<boundary','boundary>','<boundary>','路径/file']
actual=[bool(interpret(condition,v)) for v in cases];expected=[v[0]!='<' and v[-1]!='>' for v in cases];assert actual==expected
assert 'non-empty Python str' in q['query'] and 'not bytes' in q['query']
assert bool(interpret(condition,b'<boundary>')) is True
witness={'in_scope_synthetic_cases':len(cases),'in_scope_mismatches':0,'excluded_type_counterexample_preserved':True,'predicate_ast_sha256':sha(ast.dump(condition,include_attributes=False).encode()),'results_sha256':sha(canonical(actual)),'source_executed':False}
# Compare all new tasks to all old public dev tasks and to each other. Published
# overlap receipts contain opaque IDs only. A shared function/class alone is
# not a same-task proof; all overlapping task focuses were manually adjudicated.
overlaps=[]
new=native[75:]
for i,q in enumerate(new):
 for other in native[:75]+new[:i]:
  left=gold_by_id[q['query_family']]['evidence'];right=gold_by_id[other['query_family']]['evidence']
  same=any(a['path']==b['path'] and max(a['start_byte'],b['start_byte'])<min(a['end_byte'],b['end_byte']) for a in left for b in right)
  if same:overlaps.append({'left_id':q['id'],'right_id':other['id'],'shared_evidence_region':True,'decision':'distinct_task_focus_at_public_dev_boundary','reason_code':'DIFFERENT_TRIGGER_STATE_BRANCH_OR_LIFECYCLE_TASK','global_independence':'pending'})
public_components={components[q['query_family']] for q in native};assert len(public_components)==82
result={'reviewed_author_sha':BASE,'upstream_sha':UPSTREAM,'protocol_commit':PROTOCOL,'rows':rows,'counts':{'native_public_dev':91,'compat_public_dev':83,'delta_rows_reviewed':17,'new_dev_rows_reviewed':16,'revised_rows_reviewed':1,'content_accept':17,'content_reject':0,'content_needs_change':0,'prior_public_dev_rows_verified_unchanged':unchanged,'current_total_content_accept':91,'public_dev_proposed_components':82,'all_candidate_ids_metadata_only':116,'all_proposed_components_metadata_only':102,'holdout_reviewed':0,'holdout_custody_blocked':25,'new_holdout_serials_reserved_without_bodies':4,'source_suite_admitted_components':0,'remaining_independent_helper_license_evidence_blockers':0},'f0022_language_boundary_witness':witness,'component_overlap_adjudications':overlaps,'proven_new_same_task_merges':[],'global_component_adjudication':'pending_designated_global_reviewer_including_unread_holdout_and_cross_repository','independent_license_evidence':'accept_with_full_BSD_notice_retention','source_suite_admission':'not_written_pending_integrator','ranking_runs':0,'gold_changes':0,'holdout_bodies_read':False,'legacy_gold_read':False,'author_scripts_executed_or_imported':False,'not_a_protocol_complete20_family_block':True,'public_output':'opaque IDs counts hashes decision codes; no query/gold excerpts','read_allowlist_sha256':read_paths}
(HERE/'delta-dev-review.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'baseline':BASE,'counts':result['counts'],'overlap_pairs_reviewed':len(overlaps)}))
# Relation metadata is public opaque component membership, never gold bodies.
old_rel_path=AUTHOR/'relations.json';old_rel=json.loads(subprocess.check_output(['git','show','67f89aff2392ced113327cfe8f671cd153c7e0c8:'+str(old_rel_path.relative_to(ROOT))],cwd=ROOT))
old_components={m:c['global_family'] for c in old_rel['components'] for m in c['members']};assert len(old_components)==100
assert all(components[k]==v for k,v in old_components.items())
assert len(set(components)-set(old_components))==16 and {q['query_family'] for q in new}==set(components)-set(old_components)
commitment_path=str((AUTHOR/'review/holdout-commitments.json').relative_to(ROOT))
old_commitment=subprocess.check_output(['git','rev-parse','67f89aff2392ced113327cfe8f671cd153c7e0c8:'+commitment_path],cwd=ROOT).decode().strip()
new_commitment=subprocess.check_output(['git','rev-parse',BASE+':'+commitment_path],cwd=ROOT).decode().strip();assert old_commitment==new_commitment
result['opaque_relation_metadata_verification']={'old_candidate_component_memberships_preserved':100,'new_dev_memberships':16,'old_holdout_commitment_git_blob_unchanged':True,'holdout_commitment_git_blob':new_commitment,'holdout_bodies_opened':0}
(HERE/'delta-dev-review.json').write_text(json.dumps(result,indent=2)+'\n')
