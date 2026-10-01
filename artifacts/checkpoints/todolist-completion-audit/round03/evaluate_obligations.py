#!/usr/bin/env python3
"""Read-only independent byte-span task diagnosis; never replaces existing scorer/gate."""
import json,hashlib,sys,datetime
from pathlib import Path
HERE=Path(__file__).resolve().parent;ROOT=HERE.parents[3]
O=json.load((HERE/'task-obligations-v2.json').open());BY={(x['dataset'],x['id']):x for x in O['tasks']}
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def covered(target,spans):
 s,e=target['start'],target['end'];cursor=s
 for x,y in sorted(spans):
  if x>cursor:break
  cursor=max(cursor,y)
  if cursor>=e:return True
 return False
results=[]
for profile,parent,pattern in [('baseline',Path(sys.argv[1]),'p5c-{}'),('candidate',Path(sys.argv[2]),'{}')]:
 for lock in O['locked_inputs']:
  dataset=lock['dataset'];source=ROOT/lock['source_root'];run=parent/pattern.format(dataset)
  for f in lock['files']:assert sha(source/f['path'])==f['sha256']
  assert sha(ROOT/lock['query_path'])==lock['query_sha256']
  nf=run/'normalized.jsonl';rows=[json.loads(l) for l in nf.read_text().splitlines()]
  for row in rows:
   obligation=BY[(dataset,row['case_id'])];by_path={};invalid=[];scope=[]
   for h in row['hits']:
    p=(source/h['path']).resolve();assert p.is_relative_to(source.resolve())
    b=p.read_bytes();span=h.get('span');text=h.get('text');valid=h.get('evidence_valid') is True
    if h.get('source_evidence'):valid=valid and span is not None and b[span['start']:span['end']].decode()==text
    elif text:valid=valid and text.strip() in b.decode().replace('\r\n','\n')
    if not valid:invalid.append(h['path'])
    prefix=obligation.get('path_prefix')
    if prefix and not (h['path']==prefix.rstrip('/') or h['path'].startswith(prefix.rstrip('/')+'/')):scope.append(h['path'])
    if valid and span:by_path.setdefault(h['path'],[]).append((span['start'],span['end']))
   rawpath=run/row['raw_path'];raw=json.load(rawpath.open());packing=raw.get('evidence_summary',{}).get('packing',{});retrieval=raw.get('evidence_summary',{}).get('retrieval',{})
   lanes=retrieval.get('lanes',retrieval.get('lane_receipts',[]));lane_partial=[{'lane_id':x['lane_id'],'status':x['status'],'reason':x.get('truncation_reason')} for x in lanes if x['status'] not in ['complete','disabled','not_configured']]
   facets=[{'path':f['path'],'span':f['byte_span'],'required_text':f['required_text'],'covered':covered(f['byte_span'],by_path.get(f['path'],[]))} for f in obligation['required_source_evidence']]
   absence=(row['status']=='no_match' and not row['hits'] and not lane_partial and not invalid and not scope) if obligation['no_answer'] else None
   ratio=sum(f['covered'] for f in facets)/len(facets) if facets else None
   results.append({'profile':profile,'dataset':dataset,'case_id':row['case_id'],'repetition':row['repetition'],'status':row['status'],'hit_count':len(row['hits']),'minimum_task_spans':facets,'task_span_coverage':ratio,'all_task_spans_covered':all(f['covered'] for f in facets) if facets else None,'verified_absence':absence,'invalid_hits':invalid,'scope_violations':scope,'lane_partial':lane_partial,'packing_partial':packing.get('partial'),'packing_omitted_hits':packing.get('omitted_hits'),'raw_path':str(rawpath),'raw_sha256':sha(rawpath)})
base={(x['dataset'],x['case_id'],x['repetition']):x for x in results if x['profile']=='baseline'};deltas=[]
for c in results:
 if c['profile']!='candidate':continue
 key=(c['dataset'],c['case_id'],c['repetition']);b=base[key]
 delta=None if c['task_span_coverage'] is None else c['task_span_coverage']-b['task_span_coverage']
 deltas.append({'dataset':key[0],'case_id':key[1],'repetition':key[2],'task_span_coverage_delta':delta,'baseline_coverage':b['task_span_coverage'],'candidate_coverage':c['task_span_coverage'],'baseline_absence':b['verified_absence'],'candidate_absence':c['verified_absence']})
output={'schema_version':1,'observed_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'obligations_sha256':sha(HERE/'task-obligations-v2.json'),'status':'diagnostic_not_stage_gate','rows':results,'paired_deltas':deltas,'limitations':['Independent core span/facet diagnostic is not replacement scoring or all-NL100% gate.','Historical baseline vs development candidate provenance must be matched and re-frozen for certification.','Repeated rows are not independent retrieval question samples; emit per-question aggregates in formal statistical comparison.','Retained lane inventory and packing Partial remain visible.']}
target=HERE/sys.argv[3];assert not target.exists();target.write_text(json.dumps(output,ensure_ascii=False,indent=2)+'\n')
print('rows',len(results),'negative paired span deltas',len([d for d in deltas if d['task_span_coverage_delta'] is not None and d['task_span_coverage_delta']<0]))
for r in results:
 if r['profile']=='candidate' and r['repetition']==0:print(r['dataset'],r['case_id'],'coverage',r['task_span_coverage'],'absence',r['verified_absence'],'inventorypartial',r['lane_partial'])
