#!/usr/bin/env python3
"""Independent locked original51/110 span audit: baseline native vs full-on cell.
Read-only measured artifacts. Does not replace original scorer or inventory gate.
"""
import json, hashlib, datetime, sys
from pathlib import Path
HERE=Path(__file__).resolve().parent; ROOT=HERE.parents[3]
load=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
O=load(HERE/'task-obligations-v2.json'); BY={(t['dataset'],t['id']):t for t in O['tasks']}
ORDER=['smoke','exact','intents','source']
def covers(target, spans):
 cursor=target['start']
 for a,b in sorted(spans):
  if a>cursor:break
  cursor=max(cursor,b)
  if cursor>=target['end']:return True
 return False
rows=[]
for profile,parent in [('baseline',Path(sys.argv[1])),('candidate',Path(sys.argv[2]))]:
 for lock in O['locked_inputs']:
  dataset=lock['dataset']; source=(ROOT/lock['source_root']).resolve()
  assert sha(ROOT/lock['suite_path'])==lock['suite_sha256'] and sha(ROOT/lock['query_path'])==lock['query_sha256']
  for f in lock['files']:assert sha(source/f['path'])==f['sha256']
  run=parent/(f'suite-{ORDER.index(dataset):03}' if profile=='baseline' else f'{dataset}--cell_111')
  normalized=[json.loads(x) for x in (run/'normalized.jsonl').read_text().splitlines()]
  expected={(t['id'],rep) for t in O['tasks'] if t['dataset']==dataset for rep in range(3)}
  actual=[(r['case_id'],r['repetition']) for r in normalized]
  assert len(actual)==len(set(actual)) and set(actual)==expected,(profile,dataset,'missing/duplicate rows')
  for r in normalized:
   t=BY[(dataset,r['case_id'])]; spans={};invalid=[];scope=[]
   for hit in r['hits']:
    p=(source/hit['path']).resolve();assert p.is_relative_to(source) and p.is_file()
    b=p.read_bytes();s=hit.get('span');text=hit.get('text')
    valid=hit.get('evidence_valid') is True and isinstance(s,dict) and isinstance(text,str)
    valid=valid and type(s.get('start')) is int and type(s.get('end')) is int and 0<=s['start']<s['end']<=len(b)
    if valid:valid=b[s['start']:s['end']].decode('utf-8')==text
    if not valid:invalid.append(hit['path'])
    prefix=t['path_prefix']
    if prefix and not(hit['path']==prefix.rstrip('/') or hit['path'].startswith(prefix.rstrip('/')+'/')):scope.append(hit['path'])
    if valid:spans.setdefault(hit['path'],[]).append((s['start'],s['end']))
   rawp=(run/r['raw_path']).resolve();assert rawp.is_relative_to(run.resolve());raw=load(rawp)
   evidence=raw.get('evidence_summary',{});retrieval=evidence.get('retrieval',{});lanes=retrieval.get('lane_receipts',retrieval.get('lanes',[]))
   incomplete=[l for l in lanes if l.get('status') not in ['complete','disabled','not_configured']]
   fresh=evidence.get('source_freshness',{});packing=evidence.get('packing',{})
   core=[{'path':e['path'],'span':e['byte_span'],'covered':covers(e['byte_span'],spans.get(e['path'],[]))} for e in t['required_source_evidence']]
   absence=(r['status']=='no_match' and not r['hits'] and bool(lanes) and not incomplete and fresh.get('partial') is False and fresh.get('budget_exhausted') is False and fresh.get('omitted_files')=={} and not invalid and not scope) if t['no_answer'] else None
   rows.append({'profile':profile,'dataset':dataset,'id':t['id'],'repetition':r['repetition'],'status':r['status'],'core':core,'core_coverage':sum(c['covered'] for c in core)/len(core) if core else None,'verified_absence':absence,'invalid_source_hits':invalid,'scope_violations':scope,'lanes':lanes,'freshness':fresh,'packing':packing,'raw_sha256':sha(rawp),'raw_path':str(rawp)})
base={(r['dataset'],r['id'],r['repetition']):r for r in rows if r['profile']=='baseline'}
deltas=[]
for c in rows:
 if c['profile']!='candidate':continue
 key=(c['dataset'],c['id'],c['repetition']);b=base[key]
 lost=[x for x,y in zip(b['core'],c['core']) if x['covered'] and not y['covered']]
 deltas.append({'dataset':key[0],'id':key[1],'repetition':key[2],'lost_core_spans':lost,'baseline':b['core_coverage'],'candidate':c['core_coverage'],'baseline_absence':b['verified_absence'],'candidate_absence':c['verified_absence']})
issues=[{'dataset':r['dataset'],'id':r['id'],'rep':r['repetition'],'reason':'invalid_source_scope_or_absence'} for r in rows if r['profile']=='candidate' and (r['invalid_source_hits'] or r['scope_violations'] or r['verified_absence'] is False)]
issues += [{'dataset':d['dataset'],'id':d['id'],'rep':d['repetition'],'reason':'new_core_span_regression','spans':d['lost_core_spans']} for d in deltas if d['lost_core_spans']]
output={'observed_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'failed_locked_source_obligation_gate' if issues else 'passed_locked_core_nonregression_not_G5','obligations_sha256':sha(HERE/'task-obligations-v2.json'),'rows':rows,'paired_deltas':deltas,'issues':issues,'accepted_tasks':[],'limits':['51 publicdevelopment questions,not holdout;fixed110minimumspansnotallbodysemantic100%.','Source integrity/newcoreloss/noanswerhardgate only;originalrank/strictinventory/qualityCI/perquerycausereview remain separate.','All actual inventory Partial and omissions retained;never status renaming.']}
p=HERE/sys.argv[3];assert not p.exists();p.write_text(json.dumps(output,ensure_ascii=False,indent=2)+'\n');print('rows',len(rows),'issues',len(issues));raise SystemExit(1 if issues else 0)
