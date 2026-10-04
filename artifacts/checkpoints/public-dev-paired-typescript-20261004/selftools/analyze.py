#!/usr/bin/env python3
"""Same-input paired descriptive analysis; consume retained rows, never search."""
import hashlib,json,math,random,tarfile
from collections import Counter,defaultdict
from pathlib import Path
from paired import OUT,PROTO,SHAS,dump,sha,canon,inventory
METRICS=['top1','ndcg10','recall5','recall10','mrr10','span_precision','span_recall']
mean=lambda v:sum(v)/len(v) if v else None
def rows(p):return [json.loads(l) for l in p.read_bytes().splitlines() if l.strip()]
def aggregate(cases,profile,metric):
 chosen=[c for c in cases if c['profile']==profile and not c['no_answer'] and c['means'][metric] is not None]
 clusters=defaultdict(list);cats=defaultdict(list)
 for c in chosen:clusters[c['component']].append(c['means'][metric]);cats[c['category']].append(c['means'][metric])
 return {'query_count':len(chosen),'component_count':len(clusters),'query_micro':mean([c['means'][metric] for c in chosen]),'family_balanced':mean([mean(v) for v in clusters.values()]),'category_macro':mean([mean(v) for v in cats.values()]),'category_means':{k:{'queries':len(v),'mean':mean(v)} for k,v in sorted(cats.items())}}
def main():
 plan=json.loads((OUT/'plan.json').read_bytes());commands=json.loads((OUT/'runs/commands.json').read_bytes());by={(c['arm'],c['key']):c for c in commands}
 reg=json.loads((PROTO/'global-dev-review/typescript-extension/global-components.json').read_bytes())['components'];unit={m:c['global_component'] for c in reg for m in c['members']}
 cases_by_arm={};arm_reports={};errors=Counter()
 for arm in SHAS:
  cases=[];suites=[];total=Counter();noanswer=Counter();costs_available=0;hits=Counter();lanes=Counter();reasons=Counter();policies=Counter();raw_missing_lane=0;elapsed=[]
  for entry in plan['suite_entries']:
   key=entry['key'];root=OUT/'runs'/arm/key;cmd=by.get((arm,key));scheduled=entry['scheduled_rows']
   if not cmd or not (root/'manifest.json').exists():errors[arm+'::MISSING_SUITE']+=1;suites.append({'key':key,'scheduled':scheduled,'executed':0,'missing':scheduled,'metrics':None});continue
   qs=rows(root/'queries.jsonl');rs=rows(root/'normalized.jsonl');ss=rows(root/'scores.jsonl');cs=rows(root/'costs.jsonl');manifest=json.loads((root/'manifest.json').read_bytes());query={q['id']:q for q in qs}
   expected={(q['id'],i) for q in qs for i in range(entry['repetitions'])};actual=Counter((r['case_id'],r['repetition']) for r in rs);missing=expected-actual.keys();extra=actual.keys()-expected;duplicates=sum(n-1 for n in actual.values())
   complete=not missing and not extra and not duplicates and len(rs)==scheduled and len(ss)==len(rs)
   if not complete:errors[arm+'::SCHEDULE']+=1
   if manifest['infrastructure_failure']:errors[arm+'::INFRASTRUCTURE']+=1
   if cmd['changed_on_replay'] or cmd['run_exit_code']!=cmd['replay_exit_code']:errors[arm+'::REPLAY']+=1
   counts=Counter(r['status'] for r in rs);total.update(counts);samples=defaultdict(list)
   for r,s in zip(rs,ss):
    samples[r['case_id']].append((r,s));elapsed.append(r['elapsed_us']);q=query[r['case_id']]
    for h in r['hits']:hits['returned']+=1;hits['invalid' if h['evidence_valid'] is False else 'unverified' if h['evidence_valid'] is None else 'verified']+=1
    raw_path=root/r['raw_path'];raw=json.loads(raw_path.read_bytes());retrieval=raw.get('evidence_summary',{}).get('retrieval',{});receipts=retrieval.get('lane_receipts',[])
    if not receipts:raw_missing_lane+=1
    policies[str(retrieval.get('policy',{}).get('semantic_state','unavailable'))]+=1
    for lane in receipts:
     lanes[lane['lane_id']+'::'+lane['status']]+=1
     if lane.get('truncation_reason') is not None:reasons[lane['lane_id']+'::'+lane['truncation_reason']]+=1
    if q['no_answer']:
     correct=r['status']=='no_match' and not r['hits'];existing=s['no_answer_correct'] is True
     noanswer['scheduled_observed']+=1;noanswer['correct' if correct else 'incorrect']+=1;noanswer['existing_correct' if existing else 'existing_incorrect']+=1;noanswer['scorer_strict_discrepancy']+=correct!=existing
   work=sum(c.get('work') is not None for c in cs);costs_available+=work
   for q in qs:
    observed=samples[q['id']];m={f:mean([s[f] for _,s in observed if s[f] is not None]) if len(observed)==entry['repetitions'] else None for f in METRICS}
    cases.append({'id':q['id'],'profile':entry['profile'],'family':q['query_family'],'component':unit[q['query_family']],'category':q['category'],'no_answer':q['no_answer'],'scheduled_repetitions':entry['repetitions'],'observed_repetitions':len(observed),'means':m,'applicable_rows':{f:sum(s[f] is not None for _,s in observed) for f in METRICS},'status_counts':dict(Counter(r['status'] for r,_ in observed))})
   answerable=[c for c in cases if c['profile']==entry['profile'] and c['id'] in query and not c['no_answer']]
   suites.append({'key':key,'profile':entry['profile'],'scheduled':scheduled,'executed':len(rs),'missing':len(missing),'duplicate':duplicates,'extra':len(extra),'error_rows':sum(n for s,n in counts.items() if s not in ['success','partial','no_match']),'row_status_counts':dict(counts),'run_exit_code':cmd['run_exit_code'],'replay_exit_code':cmd['replay_exit_code'],'replay_byte_identical':not cmd['changed_on_replay'],'infrastructure_failure':manifest['infrastructure_failure'],'prepare':json.loads((root/'prepare.json').read_bytes()) if (root/'prepare.json').exists() else None,'readiness':cmd['readiness'],'adapter_version':manifest['adapter_version'],'source_manifest':manifest['input'],'engine_identity':manifest['engine'],'gate':json.loads((root/'gate.json').read_bytes()),'metrics':{f:{'queries':sum(c['means'][f] is not None for c in answerable),'mean':mean([c['means'][f] for c in answerable if c['means'][f] is not None])} for f in METRICS} if complete else None,'cost_availability':{'originating_work':work,'unavailable':scheduled-work,'cost_rows':len(cs)},'costs_sha256':sha((root/'costs.jsonl').read_bytes())})
  if hits['invalid'] or hits['unverified']:errors[arm+'::SOURCE_EVIDENCE']+=hits['invalid']+hits['unverified']
  cases_by_arm[arm]=cases;arm_reports[arm]={'source_sha':SHAS[arm],'scheduled':396,'executed':sum(total.values()),'missing':396-sum(total.values()),'status_counts':dict(total),'suite_results':suites,'strict_no_answer':dict(noanswer),'source_evidence':dict(hits),'cost_availability':{'originating_work':costs_available,'unavailable':396-costs_available,'interpretation':'originating work receipts, not current cache-hit work or inferred zero hidden work; costs.jsonl unchanged','token_money':'not_applicable local default nonsemantic; no provider calls'},'lane_status_counts':dict(lanes),'lane_truncation_reasons':dict(reasons),'semantic_state_counts':dict(policies),'rows_without_lane_receipts':raw_missing_lane,'all_status_elapsed_us':{'samples':len(elapsed),'sum':sum(elapsed),'mean':mean(elapsed),'min':min(elapsed) if elapsed else None,'max':max(elapsed) if elapsed else None,'causal_performance_claim':False},'descriptive_metrics':{p:{f:aggregate(cases,p,f) for f in METRICS} for p in ['native','compat']}}
  (OUT/('case-means-'+arm+'.jsonl')).write_text(''.join(json.dumps(c,ensure_ascii=False)+'\n' for c in cases))
 paired=[];maps={arm:{(c['profile'],c['id']):c for c in cases} for arm,cases in cases_by_arm.items()}
 if maps['baseline'].keys()!=maps['candidate'].keys():errors['PAIR_KEYS']+=1
 for key in sorted(maps['baseline'].keys()&maps['candidate'].keys()):
  b=maps['baseline'][key];c=maps['candidate'][key];assert (b['family'],b['component'],b['category'],b['no_answer'])==(c['family'],c['component'],c['category'],c['no_answer'])
  paired.append({'id':key[1],'profile':key[0],'component':b['component'],'category':b['category'],'no_answer':b['no_answer'],'baseline':b['means'],'candidate':c['means'],'deltas':{f:c['means'][f]-b['means'][f] if b['means'][f] is not None and c['means'][f] is not None else None for f in METRICS}})
 # One observed repository: global component clusters remain intact; no inference about absent repositories.
 intervals={};components=sorted({p['component'] for p in paired});rng=random.Random(20261003);draws=[[rng.choice(components) for _ in components] for _ in range(10000)] if len(components)>1 else []
 for profile in ['native','compat']:
  intervals[profile]={}
  for metric in METRICS:
   chosen=[p for p in paired if p['profile']==profile and not p['no_answer'] and p['deltas'][metric] is not None];clusters=defaultdict(list)
   for p in chosen:clusters[p['component']].append(p['deltas'][metric])
   cluster_means={k:mean(v) for k,v in clusters.items()};vals=[]
   if not errors and len(clusters)>1:
    for draw in draws:
     v=[cluster_means[k] for k in draw if k in cluster_means]
     if v:vals.append(mean(v))
   vals.sort();intervals[profile][metric]={'eligible_queries':len(chosen),'eligible_components':len(clusters),'query_micro_delta':mean([p['deltas'][metric] for p in chosen]),'family_balanced_delta':mean(list(cluster_means.values())),'paired95_percentile':{'lower':vals[math.ceil(.025*len(vals))-1],'upper':vals[math.ceil(.975*len(vals))-1]} if len(vals)==10000 else None,'draws':len(vals),'status':'descriptive_public_dev_conditional_on_typescript' if len(vals)==10000 and len(set(vals))>1 else 'inconclusive_degenerate_or_unavailable','category_deltas':{cat:mean([p['deltas'][metric] for p in chosen if p['category']==cat]) for cat in sorted({p['category'] for p in chosen})},'degradation_review_trigger':mean(list(cluster_means.values())) is not None and mean(list(cluster_means.values()))<-.01 if metric in ['top1','ndcg10'] else None}
 (OUT/'paired-case-deltas.jsonl').write_text(''.join(json.dumps(p,ensure_ascii=False)+'\n' for p in paired))
 report={'scope':'TypeScript admitted public DEV only; 73 native + 59 compat projections, 72 local correlation components; no new independent samples','measurement_integrity':'complete_replayable' if not errors else 'invalid_or_incomplete','errors':dict(errors),'plan_sha256':sha((OUT/'plan.json').read_bytes()),'arms':arm_reports,'paired_queries':len(paired),'observed_components':len(components),'candidate_minus_baseline':intervals if not errors else None,'bootstrap':{'seed':20261003,'replicates':10000,'method':'sorted global-component cluster paired percentile nearest rank; same draws across metrics; one-repository subset conditional inference only','draws_sha256':sha(canon(draws))},'quality_status':'not_certified: all actual status/availability/no-answer failures retained; public DEV exposed','unsupported':{'facet_coverage':'not_implemented','graph_correctness':'not_implemented','Recall20':'not_implemented','SymbolAccuracy':'not_implemented','DuplicationRate':'not_implemented','full_freshness':'not_run','semantic_ablation':'not_run','release_performance':'not_run','formal_six_repo_inference':'not_run'},'old1671':'unchanged all Partial/qualityFAIL','clean_holdout':0,'formal600':0,'CI':'unconfirmed; no new assertion'}
 dump(OUT/'summary.json',report)
 print(json.dumps({'measurement_integrity':report['measurement_integrity'],'errors':dict(errors),'counts':{arm:{k:r[k] for k in ['scheduled','executed','missing','status_counts']} for arm,r in arm_reports.items()},'paired_queries':len(paired),'components':len(components)}))
if __name__=='__main__':main()
