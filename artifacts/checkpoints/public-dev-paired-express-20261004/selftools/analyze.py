"""Read retained original outputs; same-profile same-case paired descriptive analysis."""
from pathlib import Path
from collections import Counter,defaultdict
import json,hashlib,random,math,sys
D=Path(__file__).resolve().parent.parent
sha=lambda b:hashlib.sha256(b).hexdigest()
mean=lambda x:sum(x)/len(x) if x else None
load=lambda p:json.loads(p.read_bytes())
lines=lambda p:[json.loads(x) for x in p.read_bytes().splitlines() if x.strip()] if p.exists() else []
metrics=['top1','ndcg10','recall5','recall10','mrr10','span_precision','span_recall']
plan=load(D/'plan.json');commands=load(D/'commands.json')
reg=load(Path('/workspace/express-protocol/crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/typescript-extension/global-components.json'))['components']
assert sha(json.dumps(reg,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode())==plan['registry_sha256']
units={m:c['global_component'] for c in reg for m in c['members']}
analysis=Path(sys.argv[1]) if len(sys.argv)>1 else D/'analysis';analysis.mkdir(exist_ok=False)
suites=[];cases=[];stages=[];issues=[]

def numeric_leaves(v,path=''):
 if isinstance(v,dict):
  for k,x in v.items():yield from numeric_leaves(x,path+'/'+k)
 elif isinstance(v,list):
  # Explicit originating stage name is preserved as a key, not collapsed across work stages.
  for i,x in enumerate(v):yield from numeric_leaves(x,path+'/'+(x.get('stage',str(i)) if isinstance(x,dict) else str(i)))
 elif isinstance(v,(int,float)) and not isinstance(v,bool):yield path,v

def agg(cs,field):
 eligible=[c for c in cs if not c['no_answer'] and c['means'][field] is not None]
 clusters=defaultdict(list);cats=defaultdict(list)
 for c in eligible:clusters[c['global_component']].append(c['means'][field]);cats[c['category']].append(c['means'][field])
 return {'eligible_queries':len(eligible),'metric_unavailable_answerable_queries':sum(not c['no_answer'] for c in cs)-len(eligible),
  'micro':mean([c['means'][field] for c in eligible]),'repository_macro':mean([c['means'][field] for c in eligible]),
  'family_balanced_repository_macro':mean([mean(v) for v in clusters.values()]),'components':len(clusters),
  'category_macro':mean([mean(v) for v in cats.values()]),'category_cells':{k:{'queries':len(v),'mean':mean(v)} for k,v in sorted(cats.items())}}

for arm in ['baseline','candidate']:
 for entry in plan['suite_entries']:
  cmd=next((c for c in commands if c['arm']==arm and c['key']==entry['key']),None)
  root=Path(cmd['output']) if cmd else Path('/workspace/express-runtime')/arm/'full'/entry['key']
  qs=lines(root/'queries.jsonl');rows=lines(root/'normalized.jsonl');scores=lines(root/'scores.jsonl');costs=lines(root/'costs.jsonl')
  expected={(q['id'],i) for q in qs for i in range(entry['repetitions'])}
  actual={(r['case_id'],r['repetition']) for r in rows};complete=(len(qs)==entry['query_count'] and len(rows)==entry['scheduled_rows'] and len(actual)==len(rows) and actual==expected and len(scores)==len(rows))
  manifest=load(root/'manifest.json') if (root/'manifest.json').exists() else None
  readiness=load(root/'readiness.json') if (root/'readiness.json').exists() else None
  prepared=(root/'prepare.json').exists() and readiness is not None and readiness['state']=='ready' and manifest is not None and manifest['infrastructure_failure'] is None
  if not complete or not prepared or not cmd or cmd['changed_on_replay'] or cmd['run_exit_code']!=cmd['replay_exit_code']:issues.append(arm+'/'+entry['key']+' integrity/precondition/schedule/replay failure')
  statuses=Counter(r['status'] for r in rows);samples=defaultdict(list);cost_numeric=defaultdict(list)
  qm={q['id']:q for q in qs};strict=Counter();score_noanswer=Counter();lanes=Counter();trunc=Counter();policies=Counter();sem=Counter();packings=Counter()
  hits=invalid=unknown=0;missing_lanes=0
  for row,score in zip(rows,scores):
   q=qm[row['case_id']];samples[q['id']].append((row,score))
   if q['no_answer']:
    ok=row['status']=='no_match' and not row['hits'];strict['correct' if ok else 'incorrect']+=1;score_noanswer['correct' if score['no_answer_correct'] is True else 'incorrect']+=1
   hits+=len(row['hits']);invalid+=sum(h['evidence_valid'] is False for h in row['hits']);unknown+=sum(h['evidence_valid'] is None for h in row['hits'])
   raw=load(root/row['raw_path']);ev=raw.get('evidence_summary',{});ret=ev.get('retrieval',{})
   missing_lanes+=not bool(ret.get('lane_receipts'));policies[str(ret.get('policy',{}).get('effective','unavailable'))]+=1;sem[str(ret.get('policy',{}).get('semantic_state','unavailable'))]+=1
   packings[str(ev.get('packing',{}).get('spec','unavailable'))]+=1
   for lane in ret.get('lane_receipts',[]):
    lanes[lane['lane_id']+'::'+lane['status']]+=1
    if lane.get('truncation_reason') is not None:trunc[lane['lane_id']+'::'+lane['truncation_reason']]+=1
   stages.append({'arm':arm,'profile':entry['profile'],'case_id':row['case_id'],'repetition':row['repetition'],'status':row['status'],
    'raw_sha256':sha((root/row['raw_path']).read_bytes()),'raw_path':row['raw_path'],'retrieval':ret,'packing':ev.get('packing'),
    'machine_pack_identity':{k:v for k,v in raw.get('machine_pack',{}).items() if k not in ['hits','nodes','edges','references','source_support']}})
  for cost in costs:
   if cost['work'] is not None:
    for k,v in numeric_leaves(cost['work']):cost_numeric[k].append(v)
  cs=[]
  for q in qs:
   obs=samples[q['id']]
   c={'arm':arm,'profile':entry['profile'],'id':q['id'],'family':q['query_family'],'global_component':units[q['query_family']],'category':q['category'],'no_answer':q['no_answer'],
    'scheduled':entry['repetitions'],'executed':len(obs),'missing':entry['repetitions']-len(obs),'statuses':dict(Counter(r['status'] for r,_ in obs)),
    'means':{f:mean([s[f] for _,s in obs if s[f] is not None]) if len(obs)==entry['repetitions'] else None for f in metrics},
    'applicable_rows':{f:sum(s[f] is not None for _,s in obs) for f in metrics}}
   cs.append(c);cases.append(c)
  if invalid:issues.append(arm+'/'+entry['key']+' invalid source hits')
  s={'arm':arm,'key':entry['key'],'profile':entry['profile'],'scheduled':entry['scheduled_rows'],'executed':len(rows),'missing':entry['scheduled_rows']-len(rows),
   'errors':sum(statuses[s] for s in ['timeout','tool_error','protocol_error','cancelled']),'status_counts':dict(statuses),'all_required_cases_present':complete,
   'run_exit':cmd['run_exit_code'] if cmd else None,'replay_exit':cmd['replay_exit_code'] if cmd else None,'replay_changed_files':cmd['changed_on_replay'] if cmd else None,
   'gate':load(root/'gate.json') if (root/'gate.json').exists() else None,'infrastructure_failure':manifest['infrastructure_failure'] if manifest else 'missing manifest',
   'stage_identity':{'source_sha':plan[arm],'binary_sha256':cmd['binary_sha256'] if cmd else None,'adapter':manifest['adapter'] if manifest else None,'adapter_version':manifest['adapter_version'] if manifest else None,
      'fixture_input_manifest':manifest['input'] if manifest else None,'readiness':readiness,'actual_prepare_index_succeeded':prepared,
      'engine_identity':manifest['engine'] if manifest else None,'packing_specs':dict(packings)},
   'metrics':{f:agg(cs,f) for f in metrics} if complete and prepared else None,
   'strict_noanswer':{'queries':entry['no_answer_count'],'scheduled':entry['no_answer_count']*entry['repetitions'],'observed':sum(strict.values()),'correct':strict['correct'],'incorrect':strict['incorrect'],'scorer_counts':dict(score_noanswer)},
   'source_evidence':{'returned_hits':hits,'invalid':invalid,'unverified':unknown},
   'cost':{'originating_work_receipts':sum(c['work'] is not None for c in costs),'unavailable_observed':sum(c['work'] is None for c in costs),'observed_cost_rows':len(costs),
     'numeric_originating_work_fields':{k:{'available_rows':len(v),'sum':sum(v),'mean':mean(v),'min':min(v),'max':max(v)} for k,v in sorted(cost_numeric.items())},
     'interpretation':'originating work, not current cache work; token/money not applicable to local default; exact costs in original output archive'},
   'lane_status_counts':dict(lanes),'lane_truncation_counts':dict(trunc),'missing_lane_receipt_rows':missing_lanes,'effective_policy_counts':dict(policies),'semantic_state_counts':dict(sem),
   'all_status_rpc_elapsed_us':{'samples':len(rows),'sum':sum(r['elapsed_us'] for r in rows),'mean':mean([r['elapsed_us'] for r in rows]),'performance_causality':False}}
  suites.append(s)

paired=[];comparisons={}
for profile in ['native','compat']:
 b={c['id']:c for c in cases if c['arm']=='baseline' and c['profile']==profile};c={x['id']:x for x in cases if x['arm']=='candidate' and x['profile']==profile}
 if set(b)!=set(c):issues.append(profile+' paired cases missing')
 for k in sorted(set(b)&set(c)):
  x,y=b[k],c[k];assert (x['family'],x['category'],x['no_answer'])==(y['family'],y['category'],y['no_answer'])
  paired.append({'profile':profile,'id':k,'family':x['family'],'global_component':x['global_component'],'category':x['category'],'no_answer':x['no_answer'],
   'baseline':x,'candidate':y,'delta':{m:y['means'][m]-x['means'][m] if x['means'][m] is not None and y['means'][m] is not None else None for m in metrics}})
 clusters=sorted({p['global_component'] for p in paired if p['profile']==profile})
 rng=random.Random(20261003);draws=[[rng.choice(clusters) for _ in clusters] for _ in range(10000)] if len(clusters)>=2 else []
 comparison={}
 for metric in metrics:
  eligible=[p for p in paired if p['profile']==profile and not p['no_answer'] and p['delta'][metric] is not None]
  cm=defaultdict(list)
  for p in eligible:cm[p['global_component']].append(p['delta'][metric])
  component_means={k:mean(v) for k,v in cm.items()}
  boots=[mean([component_means[k] for k in draw if k in component_means]) for draw in draws]
  boots=sorted(v for v in boots if v is not None)
  ci=[boots[math.ceil(.025*10000)-1],boots[math.ceil(.975*10000)-1]] if len(boots)==10000 and len(cm)>=2 and not issues else None
  cats=defaultdict(list)
  for p in eligible:cats[p['category']].append(p['delta'][metric])
  comparison[metric]={'paired_eligible_queries':len(eligible),'components':len(cm),'micro_delta':mean([p['delta'][metric] for p in eligible]) if not issues else None,
   'family_balanced_repository_macro_delta':mean(list(component_means.values())) if not issues else None,'paired_component_percentile_95':ci,
   'category_delta':{k:mean(v) for k,v in sorted(cats.items())} if not issues else None,'interval_status':('degenerate_inconclusive; numerical quantiles descriptive only' if ci and ci[0]==ci[1] else 'Express-only descriptive conditional') if ci else 'inconclusive/not_applicable',
   'review_trigger':metric in ['top1','ndcg10'] and mean(list(component_means.values())) is not None and mean(list(component_means.values()))<-.01}
 comparisons[profile]=comparison
noanswer={a:{f:sum(s['strict_noanswer'][f] for s in suites if s['arm']==a) for f in ['scheduled','observed','correct','incorrect']} for a in ['baseline','candidate']}
noanswer_delta=(noanswer['candidate']['correct']/noanswer['candidate']['scheduled']-noanswer['baseline']['correct']/noanswer['baseline']['scheduled']) if not issues and all(v['scheduled']>0 and v['observed']==v['scheduled'] for v in noanswer.values()) else None
report={'scope':'fixed Express same-profile same-input public DEV paired; no clean holdout; no performance causality','measurement_integrity':'complete_replayable' if not issues else 'invalid_or_incomplete',
 'issues':issues,'source_shas':{a:plan[a] for a in ['baseline','candidate']},'plan_sha256':sha((D/'plan.json').read_bytes()),'admission_sha256':plan['admission_sha256'],
 'suites':suites,'paired_comparisons':comparisons,'quality_status':'not_certified; Partial and strict noanswer failure retained','formal_600_accepted':0,'clean_holdout':0,
 'strict_noanswer_paired_delta':{'arms':noanswer,'accuracy_delta':noanswer_delta,'interpretation':'strict status no_match and empty hits; Partial incorrect; missing no inferred values'},
 'statistics':'3 repetitions mean within query; variants mean inside frozen global component; 10000 fixed random.Random(20261003) cluster draws; nearest ranks; one-repo subset not full global inference',
 'unsupported':plan['unsupported'],'old_evidence':'old1671 allPartial qualityFAIL untouched; old JS783 unchanged; default gold preserved','live_provider_calls':0,'new_search_calls_during_analysis':0}
(analysis/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
for name,values in [('case-means',cases),('paired-case-deltas',paired),('stage-identities',stages)]:
 (analysis/(name+'.jsonl')).write_text(''.join(json.dumps(v)+'\n' for v in values))
print(json.dumps({'integrity':report['measurement_integrity'],'issues':issues,'suites':[{k:s[k] for k in ['arm','profile','scheduled','executed','missing','errors','status_counts']} for s in suites],
 'top1_ndcg':{p:{m:comparisons[p][m] for m in ['top1','ndcg10']} for p in comparisons}},indent=2))
