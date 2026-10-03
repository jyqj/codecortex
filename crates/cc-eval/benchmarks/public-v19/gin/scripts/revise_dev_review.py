"""Versioned repair of four public-dev findings; never reads blocked holdout bodies."""
import pathlib,subprocess,json,hashlib,copy
BASE=pathlib.Path(__file__).resolve().parents[1]
BEFORE='77d8707110afcb9935d29117ddf6162df4cef277'
REVIEW='0ecfc52a14e423a1d37b81240667d4c1511076cb'
REVIEW_PATH='crates/cc-eval/benchmarks/public-v19/reviews/gin/dev-067-review.json'
NAMESPACE='crates/cc-eval/benchmarks/public-v19/gin/'
def sha(b): return hashlib.sha256(b).hexdigest()
def get(path): return subprocess.check_output(['git','show',BEFORE+':'+NAMESPACE+path])
def line_map(raw): return {json.loads(l)['query_family']:l for l in raw.splitlines(keepends=True)}
def gold_fragments(raw):
 s=raw.decode(); i=1; out={}; dec=json.JSONDecoder()
 while True:
  while s[i].isspace() or s[i]==',': i+=1
  if s[i]==']': break
  start=i; obj,i=dec.raw_decode(s,i); out[obj['family']]=s[start:i].encode()
 return out
review_raw=subprocess.check_output(['git','show',REVIEW+':'+REVIEW_PATH]); review=json.loads(review_raw)
assert review['author_current_sha']==BEFORE and review['counts']['accept']==63 and review['counts']['needschange']==4
native_raw=get('queries.native.dev.jsonl'); compat_raw=get('queries.compat.dev.jsonl'); gold_raw=get('gold/evidence.json')
assert sha(native_raw)==review['native_file_sha256'] and sha(compat_raw)==review['compat_file_sha256']
rows=[json.loads(l) for l in native_raw.splitlines()]
accepted_hashes={r['family_id_sha256'] for r in review['rows'] if r['decision']=='accept'}
changed={q['query_family'] for q in rows if sha(q['query_family'].encode()) not in accepted_hashes}
assert changed=={'v19.gin.f0029','v19.gin.f0045','v19.gin.f0053','v19.gin.f0075'}
# Derive additional guard evidence exclusively from already admitted public source.
b=(BASE/'source/context.go').read_bytes(); start=b.index(b'func bodyAllowedForStatus('); end=b.index(b'\n}',start)+2
guard=dict(source_sha='43fe48e8a0f44af783116cdb010725e6bb50255f',path='context.go',symbol='bodyAllowedForStatus',start_line=b[:start].count(b'\n')+1,end_line=b[:end].count(b'\n')+1,span=dict(start=start,end=end),sha256=sha(b[start:end]))
new=[]; revision_rows=[]
for q in rows:
 f=q['query_family']
 if f not in changed:
  new.append(q); continue
 old=copy.deepcopy(q); a=q['annotations']['v19']; errors=[]
 if f=='v19.gin.f0029':
  for g in q['answers']: g['primary']=g['id']=='facet-2'; g['grade']=3 if g['primary'] else 2
  errors=['R_PRIMARY_NOT_DISTINGUISHING_FACET']
 elif f=='v19.gin.f0075':
  q['query']='For an HTTP 200 response, what happens when Context.HTML is used without a configured HTML renderer?'
  a['intent']=q['query']; a['literal_answer']='For HTTP 200, bodyAllowedForStatus permits a body. Context.HTML calls Context.Render with an empty render.HTML; HTML.Render sets content type then returns errHTMLRendererNotConfigured on nil Template; Context.Render records the error and aborts.'
  a['status_precondition']=dict(status_code=200,body_allowed=True,source_evidence=guard)
  for e in a['graph_constraints']:
   if e['relation']!='calls with empty HTML': e['precondition']='bodyAllowedForStatus(200) == true'
  errors=['R_STATUS_PRECONDITION_MISSING']
 else:
  a['global_family']='v19.gin.f0045'; a['component_link_status']='author_proposed_conservative_merge_pending_designated_global_adjudication'; a['component_members']=['v19.gin.f0045','v19.gin.f0053']; errors=['C_TASK_EQUIVALENCE_REVIEW_NEEDED']
 a['revision']=dict(version=2,review_commit=REVIEW,review_receipt_sha256=sha(review_raw),previous_row_sha256=sha(line_map(native_raw)[f]),resolved_error_codes=errors,re_review_status='pending')
 revision_rows.append(dict(family=f,before_native_row_sha256=sha(line_map(native_raw)[f]),after_native_row_sha256=sha((json.dumps(q,ensure_ascii=False,separators=(',',':'))+'\n').encode()),error_codes=errors,previous_split=old['split'],current_split=q['split'],previous_global_family=old['annotations']['v19']['global_family'],current_global_family=a['global_family']))
 new.append(q)
new_byfamily={q['query_family']:q for q in new}
# Keep every accepted native and compat JSONL line literally byte-identical.
native_out=b''.join(line_map(native_raw)[q['query_family']] if q['query_family'] not in changed else (json.dumps(q,ensure_ascii=False,separators=(',',':'))+'\n').encode() for q in new)
compat_out=b''
for oldline in compat_raw.splitlines(keepends=True):
 c=json.loads(oldline); f=c['query_family']
 if f not in changed: compat_out+=oldline; continue
 c=copy.deepcopy(new_byfamily[f]); c['expected_files']=list(dict.fromkeys(alt['path'] for g in sorted(c['answers'],key=lambda g:not g['primary']) for alt in g['alternatives'])); c['answers']=[]
 compat_out+=(json.dumps(c,ensure_ascii=False,separators=(',',':'))+'\n').encode()
gold=json.loads(gold_raw)
for i,g in enumerate(gold):
 if g['family'] in changed: gold[i]=dict(family=g['family'],**new_byfamily[g['family']]['annotations']['v19'])
gold_out=(json.dumps(gold,ensure_ascii=False,indent=2)+'\n').encode()
old_native=line_map(native_raw); new_native=line_map(native_out); old_compat=line_map(compat_raw); new_compat=line_map(compat_out); old_gold=gold_fragments(gold_raw); new_gold=gold_fragments(gold_out)
unchanged=[]
for f in sorted(set(old_native)-changed):
 assert old_native[f]==new_native[f] and old_gold[f]==new_gold[f]
 if f in old_compat: assert old_compat[f]==new_compat[f]
 unchanged.append(dict(family_sha256=sha(f.encode()),native_row_sha256=sha(old_native[f]),gold_fragment_sha256=sha(old_gold[f]),compat_row_sha256=sha(old_compat[f]) if f in old_compat else None))
assert len(unchanged)==63
(BASE/'queries.native.dev.jsonl').write_bytes(native_out); (BASE/'queries.compat.dev.jsonl').write_bytes(compat_out); (BASE/'gold/evidence.json').write_bytes(gold_out)
relations=dict(protocol_version=1,status='author_proposed_conservative_merge_pending_designated_global_adjudication',review_commit=REVIEW,components=[dict(global_family='v19.gin.f0045',members=['v19.gin.f0045','v19.gin.f0053'])])
(BASE/'relations.json').write_text(json.dumps(relations,indent=2)+'\n'); (BASE/'review/dev-067-independent-review.json').write_bytes(review_raw)
receipt=dict(revision_version=2,author_before_commit=BEFORE,independent_review_commit=REVIEW,independent_review_receipt_sha256=sha(review_raw),independent_reviewer_id=review['reviewer_id'],native_dev_rows=67,compat_dev_rows=55,dev_components_proposed=66,unchanged_independently_accepted_dev_rows=63,revised_rows_pending_re_review=4,proposed_components_pending_re_review_or_adjudication=3,draft_family_ids_preserved=100,global_components_proposed=99,rows=revision_rows,unchanged_accepted_byte_proof=unchanged,before_file_sha256={'native_dev':sha(native_raw),'compat_dev':sha(compat_raw),'gold_dev':sha(gold_raw)},after_file_sha256={'native_dev':sha(native_out),'compat_dev':sha(compat_out),'gold_dev':sha(gold_out)},blocked_holdout_bodies_read=0,blocked_holdout_custody_changed=False,ranking_runs=0,self_review_signed=False)
(BASE/'provenance/dev-revision-v2.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(dict(revised_rows=4,accepted_rows_byte_unchanged=63,public_dev_components_proposed=66,revision_receipt_sha256=sha((BASE/'provenance/dev-revision-v2.json').read_bytes()))))
