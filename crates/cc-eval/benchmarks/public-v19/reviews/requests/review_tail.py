#!/usr/bin/env python3
"""Read current public dev only; publish opaque per-row checks/hashes, never gold."""
import argparse,ast,hashlib,json,pathlib,subprocess
parser=argparse.ArgumentParser();parser.add_argument("--block",type=int,choices=[3,4],required=True);args=parser.parse_args()
START,END={2:(20,40),3:(40,60),4:(60,75)}[args.block]
HERE=pathlib.Path(__file__).resolve().parent
ROOT=HERE.parents[5]
AUTHOR=ROOT/'crates/cc-eval/benchmarks/public-v19/requests'
BASE='67f89aff2392ced113327cfe8f671cd153c7e0c8'
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
assert len(native)==len(gold)==75 and len(compat)==67
assert all(r['split']=='dev' for r in native+compat) and all(r['split']=='dev' for r in gold)
gold_by_id={g['family_id']:g for g in gold};compat_by_id={q['id']:q for q in compat}
selected=native[START:END];selected_ids={r['id'] for r in selected}
manifest=json.loads(read('source-manifest.json'))
assert manifest['source_sha']==UPSTREAM
license_review=json.loads(read('license/inclusion-review.json'))
assert len(license_review['adapted_third_party_helpers'])==3
assert license_review['status']=='author_inventory_not_independent_license_review'
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
assert len(components)==100 and len(relations['components'])==86

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
   assert call.func.attr==leaf and isinstance(call.func.value,ast.Name)
   receiver=call.func.value.id;klass=callee_symbol.rsplit('.',1)[0]
   caller_node=next(n for n in symbols[(caller['path'],caller['symbol'])] if offsets(raw,n)==(caller['start_byte'],caller['end_byte']))
   constructors=[]
   for n in ast.walk(caller_node):
    if getattr(n,'lineno',edge['line'])>edge['line']:continue
    if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id==receiver for t in n.targets):constructors.append(n.value)
    if isinstance(n,ast.With):
     constructors.extend(item.context_expr for item in n.items if isinstance(item.optional_vars,ast.Name) and item.optional_vars.id==receiver)
   constructors=[c for c in constructors if isinstance(c,ast.Call) and ((isinstance(c.func,ast.Name) and c.func.id==klass) or (isinstance(c.func,ast.Attribute) and c.func.attr==klass))]
   assert constructors, 'receiver must be constructor-bound within actual caller, not name coincidence'
   # Branch conditions and relevant imports are also independently source-read.
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
 if q['query_family']=='v19.requests.f0074':
  reason_codes.append('LINKED_COMPONENT_HAS_PRIOR_SCOPE_CLARIFICATION_PENDING')
 rows.append({'query_id':q['id'],'family_id':q['query_family'],'global_component':g['global_family'],'decision':decision,'decision_scope':'current_public_dev_source_content_only','reason_codes':reason_codes,'question_sha256':sha(canonical(q)),'gold_record_sha256':sha(canonical(g)),'evidence_records_checked':len(evidence),'chain_edges_checked':len(g['edges']),'required_facets_checked':len(q['annotations']['v19']['facets']),'bounded_no_answer':q['no_answer'],'suite_admission':'blocked_adapted_helper_license_and_global_review','reviewer_id':'requests-independent-dev-reviewer/root','author_id':q['annotations']['v19']['author_id']})

# Structural absence checks accompany independent manual semantic source review.
# All target names/literals remain in the already-public dev input, not this report.
for q in selected:
 if not q['no_answer']:continue
 g=gold_by_id[q['query_family']];absence=g['absence'];raw,tree=module(g['evidence'][0]['path'])
 if absence['type']=='assignment_not_member':
  name=absence['name']
  declarations=[n for n in tree.body if isinstance(n,(ast.Assign,ast.AnnAssign)) and ((isinstance(n,ast.AnnAssign) and isinstance(n.target,ast.Name) and n.target.id==name) or (isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id==name for t in n.targets)))]
  assert len(declarations)==1 and absence['missing'] not in ast.literal_eval(declarations[0].value)
 elif absence['type']=='required_literal':
  e=g['evidence'][0];node=next(n for n in symbols[(e['path'],e['symbol'])] if offsets(raw,n)==(e['start_byte'],e['end_byte']))
  segment=raw[e['start_byte']:e['end_byte']].decode()
  assert absence['literal'] in segment and absence['also'] in segment
  assert any(isinstance(n,ast.Raise) for n in ast.walk(node)), 'bounded unimplemented extension body reviewed'
  # Literal presence alone is insufficient: the reviewer separately inspected
  # the full branch and all assignments, with only scoped content acceptance.
 elif absence['type']=='missing_class':
  assert absence['name'] not in {n.name for n in ast.walk(tree) if isinstance(n,ast.ClassDef)}
  assert not any(isinstance(n,ast.AsyncFunctionDef) for n in ast.walk(tree))
 elif absence['type']=='no_calls':
  e=g['evidence'][0];node=next(n for n in symbols[(e['path'],e['symbol'])] if offsets(raw,n)==(e['start_byte'],e['end_byte']))
  segment=raw[e['start_byte']:e['end_byte']].decode();assert absence['anchor'] in segment
  actual={ast.unparse(n.func) for n in ast.walk(node) if isinstance(n,ast.Call)}
  assert actual.isdisjoint(absence['missing'])
  # Absence of listed call strings alone is not semantic proof; actual getter
  # or extension body and all branch effects were independently read.
 else:
  raise AssertionError('unreviewed absence form')

result={'reviewed_author_sha':BASE,'upstream_sha':UPSTREAM,'protocol_commit':PROTOCOL,'reviewer_id':'requests-independent-dev-reviewer/root','rows':rows,'counts':{'native_public_dev':75,'compat_public_dev':67,'block_rows_reviewed':len(rows),'block_answerable':sum(not r['bounded_no_answer'] for r in rows),'block_no_answer':sum(r['bounded_no_answer'] for r in rows),'content_accept':sum(r['decision']=='accept' for r in rows),'content_reject':sum(r['decision']=='reject' for r in rows),'content_needs_change':sum(r['decision']=='needs_change' for r in rows),'block_proposed_components':len({r['global_component'] for r in rows}),'content_accepted_unique_proposed_components':len({r['global_component'] for r in rows if r['decision']=='accept'}),'accepted_suite_components':0,'other_rows_not_reviewed_in_this_receipt':75-len(rows),'holdout_reviewed':0,'holdout_custody_blocked':25,'historically_public_holdout_metadata_count':2,'adapted_helper_license_blockers':3},'source_license_admission':'blocked_not_accepted','global_component_adjudication':'pending_designated_global_reviewer','ranking_runs':0,'gold_changes':0,'holdout_bodies_read':False,'legacy_gold_read':False,'author_scripts_executed_or_imported':False,'block_number':args.block,'native_row_range_half_open':[START,END],'not_a_protocol_complete20_family_block':True,'public_output':'opaque IDs counts hashes decision codes; no query/gold excerpts','read_allowlist_sha256':read_paths}
(HERE/f'block{args.block:02}-review.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'baseline':BASE,'counts':result['counts'],'source_license_admission':result['source_license_admission']}))
