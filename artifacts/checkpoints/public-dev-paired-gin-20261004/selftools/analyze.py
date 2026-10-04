#!/usr/bin/env python3
"""Deterministic paired diagnostics from unchanged scorer outputs; no missing fill."""
import json, hashlib, subprocess, sys
from collections import Counter, defaultdict
from pathlib import Path

FIELDS=['top1','ndcg10','recall5','recall10','mrr10','span_precision','span_recall']
ROOT=Path('/workspace/codecortex')
BASE=ROOT/'artifacts/checkpoints/public-dev-paired-gin-20261004'
mean=lambda xs:sum(xs)/len(xs) if xs else None
def read(p):return json.loads(p.read_bytes())
def rows(p):return [json.loads(l) for l in p.read_bytes().splitlines() if l.strip()] if p.exists() else []
def save(p,x):p.write_text(json.dumps(x,indent=2,sort_keys=True)+'\n')
def flatten(x,prefix=''):
    result={}
    if isinstance(x,dict):
        for k,v in x.items():result.update(flatten(v,prefix+'.'+k if prefix else k))
    elif isinstance(x,list):
        for i,v in enumerate(x):result.update(flatten(v,prefix+'.'+str(i)))
    elif isinstance(x,(int,float)) and not isinstance(x,bool) and not prefix.endswith(('schema_version','scan_cap')):result[prefix]=x
    return result
def analyze(runroot,dest):
    dest.mkdir(parents=True,exist_ok=False)
    registry_entry='fff0931e5b470aa3b9111d6ce55444f9b40f98cf:artifacts/checkpoints/public-dev-current-group-pygo-20261003/protocol/global-dev-review/typescript-extension/global-components.json'
    registry_raw=subprocess.check_output(['git','show',registry_entry],cwd=ROOT)
    registry= json.loads(registry_raw)['components']
    units={m:r['global_component'] for r in registry for m in r['members']}
    summaries=[];cases=[];observations=[];errors=[]
    for arm in ['baseline','candidate']:
        plan=read(BASE/arm/'plan.json')
        for suite in plan['suites']:
            mode=suite['mode']; path=runroot/arm/'runs'/mode
            qs=rows(path/'queries.jsonl'); measured=rows(path/'normalized.jsonl'); scores=rows(path/'scores.jsonl'); costs=rows(path/'costs.jsonl')
            expected={(q['id'],i) for q in qs for i in range(suite['suite']['repetitions'])}
            actual=[(r['case_id'],r['repetition']) for r in measured]
            missing=sorted(expected-set(actual)); duplicate=len(actual)-len(set(actual))
            complete=len(qs)==suite['query_count'] and len(actual)==suite['scheduled_rows'] and not missing and not duplicate and set(actual)==expected and len(scores)==len(actual) and len(costs)==len(actual)
            if not complete:errors.append(arm+'/'+mode+': incomplete schedule/scorer/cost alignment')
            manifest=read(path/'manifest.json'); gate=read(path/'gate.json'); ready=read(path/'readiness.json') if (path/'readiness.json').exists() else None
            integrity=read(BASE/arm/'logs'/(mode+'-integrity.json'))
            stage_ok=manifest['infrastructure_failure'] is None and ready and ready['state']=='ready' and (path/'prepare.json').exists()
            if not stage_ok:errors.append(arm+'/'+mode+': prepare/readiness/infrastructure failed')
            if not integrity['byte_identical'] or integrity['run']['exit_code']!=integrity['replay']['exit_code']:errors.append(arm+'/'+mode+': replay mismatch')
            status=Counter(r['status'] for r in measured); hits=[h for r in measured for h in r['hits']]
            samples=defaultdict(list); cost_values=defaultdict(list)
            for row,score,cost in zip(measured,scores,costs):
                assert (cost['case_id'],cost['repetition'])==(row['case_id'],row['repetition'])
                samples[row['case_id']].append((row,score))
                observations.append(dict(arm=arm,profile=mode,case_id=row['case_id'],repetition=row['repetition'],status=row['status'],
                    scores=score,cost=cost,raw_path=row['raw_path'],raw_digest=row['raw_digest'],diagnostic=row['diagnostic']))
                if cost['work'] is not None:
                    for k,v in flatten(cost['work']).items():cost_values[k].append(v)
            noanswer=[]
            for q in qs:
                observed=samples[q['id']]; strict=[r['status']=='no_match' and not r['hits'] for r,s in observed]
                means={f:mean([s[f] for r,s in observed if s[f] is not None]) for f in FIELDS}
                cases.append(dict(arm=arm,profile=mode,id=q['id'],category=q['category'],family=q['query_family'],global_component=units[q['query_family']],
                    no_answer=q['no_answer'],scheduled_repetitions=suite['suite']['repetitions'],observed_repetitions=len(observed),
                    means=means,metric_applicable_rows={f:sum(s[f] is not None for r,s in observed) for f in FIELDS},status_counts=dict(Counter(r['status'] for r,s in observed)),
                    strict_no_answer_all_repeats=all(strict) if len(observed)==suite['suite']['repetitions'] and q['no_answer'] else None))
                if q['no_answer']:noanswer.extend(strict)
            eligible=[c for c in cases if c['arm']==arm and c['profile']==mode and not c['no_answer']]
            aggregate={}
            for f in FIELDS:
                available=[c for c in eligible if c['means'][f] is not None]
                categories=defaultdict(list); families=defaultdict(list)
                for c in available:categories[c['category']].append(c['means'][f]); families[c['global_component']].append(c['means'][f])
                aggregate[f]=dict(eligible_queries=len(available),query_micro=mean([c['means'][f] for c in available]),
                    category_macro=mean([mean(v) for v in categories.values()]),category_means={k:mean(v) for k,v in sorted(categories.items())},
                    family_balanced=mean([mean(v) for v in families.values()]),applicable_components=len(families)) if complete else None
            summaries.append(dict(arm=arm,profile=mode,source_sha=plan['source_sha'],binary_sha256={n:a['sha256'] for n,a in read(BASE/arm/'build/build-receipt.json')['artifacts'].items()},
                scheduled=suite['scheduled_rows'],executed=len(actual),missing_count=suite['scheduled_rows']-len(set(actual)),missing_case_repetitions=missing,
                duplicates=duplicate,error_rows=sum(v for k,v in status.items() if k in ['protocol_error','tool_error','timeout']),partial=status['partial'],status_counts=dict(status),
                schedule_complete=complete,prepare_index_readiness_succeeded=bool(stage_ok),readiness=ready,infrastructure_failure=manifest['infrastructure_failure'],
                gate=gate,run_exit_code=integrity['run']['exit_code'],replay_exit_code=integrity['replay']['exit_code'],replay_byte_identical=integrity['byte_identical'],
                metrics=aggregate,strict_no_answer=dict(scheduled_queries=sum(q['no_answer'] for q in qs),observed_rows=len(noanswer),correct_rows=sum(noanswer),incorrect_rows=len(noanswer)-sum(noanswer),accuracy=mean(noanswer) if complete else None),
                source_evidence=dict(returned_hits=len(hits),invalid_hits=sum(h['evidence_valid'] is False for h in hits),unverified_hits=sum(h['evidence_valid'] is None for h in hits)),
                retrieval_cost=dict(receipts_available=sum(c['work'] is not None for c in costs),receipts_unavailable=sum(c['work'] is None for c in costs),
                    missing_rows_without_observed_cost=suite['scheduled_rows']-len(costs),originating_work_counters={k:dict(observed=len(v),sum=sum(v),mean=mean(v),min=min(v),max=max(v)) for k,v in sorted(cost_values.items())},
                    interpretation='originating work receipts; never current cache-hit work or complete-retrieval performance causality'),
                walltime=dict(samples=len(actual),rpc_elapsed_us_sum=sum(r['elapsed_us'] for r in measured),rpc_elapsed_us_mean=mean([r['elapsed_us'] for r in measured]),interpretation='all statuses included; cloud walltime not performance causality')))
    joined=defaultdict(dict)
    for c in cases:joined[(c['profile'],c['id'])][c['arm']]=c
    pairs=[]
    for (mode,qid),arms in sorted(joined.items()):
        if set(arms)!={'baseline','candidate'}:errors.append(mode+'/'+qid+': missing arm');continue
        a,b=arms['baseline'],arms['candidate']; valid=a['observed_repetitions']==a['scheduled_repetitions']==b['observed_repetitions']==b['scheduled_repetitions']
        assert all(a[k]==b[k] for k in ['category','family','global_component','no_answer'])
        pairs.append(dict(profile=mode,id=qid,category=a['category'],global_component=a['global_component'],no_answer=a['no_answer'],valid_same_input_pair=valid,
            baseline=a,candidate=b,deltas={f:b['means'][f]-a['means'][f] if valid and a['means'][f] is not None and b['means'][f] is not None else None for f in FIELDS}))
    deltas={}
    for mode in ['native','compat']:
        eligible=[p for p in pairs if p['profile']==mode and not p['no_answer']]
        deltas[mode]={}
        for f in FIELDS:
            present=[p for p in eligible if p['deltas'][f] is not None]; groups=defaultdict(list);cats=defaultdict(list)
            for p in present:groups[p['global_component']].append(p['deltas'][f]);cats[p['category']].append(p['deltas'][f])
            deltas[mode][f]=dict(eligible_pairs=len(present),query_micro=mean([p['deltas'][f] for p in present]),family_balanced=mean([mean(v) for v in groups.values()]),category_macro=mean([mean(v) for v in cats.values()]),category_means={k:mean(v) for k,v in sorted(cats.items())},independent_components=len(groups)) if not errors else None
    result=dict(scope='Gin Go fixed public DEV two-arm same-version paired diagnostic only',measurement_integrity='complete_replayable' if not errors else 'incomplete_or_invalid',errors=errors,
        suites=summaries,candidate_minus_baseline_same_profile=deltas,paired_cases=len(pairs),
        quality='not_certified; gate failures and all Partial statuses preserved',registry_entry=registry_entry,registry_sha256=hashlib.sha256(registry_raw).hexdigest(),
        intervals='not_run: planned-v19-global-paired-cluster-v1 remains unimplemented; existing evaluator 2000-draw diagnostics do not replace the preregistered 10000 cluster bootstrap',
        unsupported={'Recall20':'not_implemented','SymbolAccuracy':'not_implemented','DuplicationRate':'not_implemented','facet_coverage':'not_implemented','graph_correctness':'not_implemented','freshness':'not_run','performance':'not_certified'},
        historical={'four_repo_native':301,'four_repo_compat':256,'correlated_groups':280,'old1671':'allPartial/qualityFAIL preserved; not replaced by this pair'},
        new_independent_samples=0,formal600=0,clean_holdout=0,future_holdout_reads=0,live_provider_calls=0,paid_provider_calls=0,
        interpretation='same-profile paired deltas descriptive under Partial; native/compat scoring formulas are never subtracted against each other')
    save(dest/'summary.json',result)
    for name,values in [('case-means',cases),('paired-cases',pairs),('observed-rows',observations)]:
        (dest/(name+'.jsonl')).write_text(''.join(json.dumps(v,sort_keys=True)+'\n' for v in values))
    print(json.dumps({'measurement_integrity':result['measurement_integrity'],'errors':errors,'paired_cases':len(pairs)}))
if __name__=='__main__':analyze(Path(sys.argv[1]),Path(sys.argv[2]))
