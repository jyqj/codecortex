#!/usr/bin/env python3
"""Descriptive preregistered means from retained unchanged scorer output only."""
from collections import Counter,defaultdict
import argparse
import hashlib
import json
from pathlib import Path

HERE=Path(__file__).resolve().parent
METRICS=['top1','ndcg10','recall5','recall10','mrr10','span_precision','span_recall']
sha=lambda b:hashlib.sha256(b).hexdigest()
mean=lambda values:sum(values)/len(values) if values else None


def jsonl(path):return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def strict_no_answer(row):return row['status']=='no_match' and not row['hits']


def aggregate(cases,profile,field,component_by_family):
    eligible=[c for c in cases if c['profile']==profile and not c['no_answer'] and c['means'][field] is not None]
    repos=defaultdict(list);categories=defaultdict(list);clusters=defaultdict(lambda:defaultdict(list))
    for c in eligible:
        value=c['means'][field];repos[c['repo']].append(value);categories[c['category']].append(value)
        clusters[c['repo']][component_by_family[c['family']]].append(value)
    cells={}
    for c in eligible:cells.setdefault(c['repo']+'::'+c['category'],[]).append(c['means'][field])
    repo_family_means={r:mean([mean(values) for values in units.values()]) for r,units in clusters.items()}
    return {'eligible_query_count':len(eligible),'query_micro':mean([c['means'][field] for c in eligible]),
        'repository_macro':mean([mean(v) for v in repos.values()]),'category_macro':mean([mean(v) for v in categories.values()]),
        'repository_means':{r:{'queries':len(v),'mean':mean(v)} for r,v in sorted(repos.items())},
        'category_means':{r:{'queries':len(v),'mean':mean(v)} for r,v in sorted(categories.items())},
        'repository_category_cells':{r:{'queries':len(v),'mean':mean(v)} for r,v in sorted(cells.items())},
        'family_balanced_repository_macro':mean(list(repo_family_means.values())),
        'family_balanced_repository_means':repo_family_means,
        'global_components_in_applicable_cases':len({component_by_family[c['family']] for c in eligible}),
        'applicability':'scorer-supplied numerical values only; no-answer excluded; None stays unavailable'}


def analyze(plan_path,run_root,output):
    plan=json.loads(plan_path.read_bytes());commands=json.loads((run_root/'commands.json').read_bytes())
    registry=json.loads((HERE.parent/'typescript-extension/global-components.json').read_bytes())['components']
    canonical=json.dumps(registry,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
    if sha(canonical)!=plan['global_correlation_registry_sha256']:raise ValueError('REGISTRY_DRIFT')
    units={m:r['global_component'] for r in registry for m in r['members']}
    by_key={r['key']:r for r in commands};cases=[];errors=Counter();statuses=Counter();strict=Counter();existing=Counter()
    suite_results=[];raw_cost_available=0;hits=invalid=unknown=0;elapsed=[];noanswer_rows=0;noanswer_expected=0;rule_discrepancies=0
    for entry in plan['suite_entries']:
        key=entry['key'];root=run_root/key;cmd=by_key.get(key)
        if cmd is None:errors['MISSING_SUITE']+=1;continue
        if not (root/'manifest.json').exists():errors['MISSING_MANIFEST']+=1;continue
        manifest=json.loads((root/'manifest.json').read_bytes());qs=jsonl(root/'queries.jsonl');rows=jsonl(root/'normalized.jsonl');scores=jsonl(root/'scores.jsonl')
        noanswer_expected+=entry['no_answer_count']*entry['repetitions']
        expected={(q['id'],i) for q in qs for i in range(entry['repetitions'])};actual={(r['case_id'],r['repetition']) for r in rows}
        complete=(actual==expected and len(rows)==len(expected) and len(scores)==len(rows))
        if not complete:errors['MISSING_OR_DUPLICATE_SCHEDULED_ROWS']+=1
        if manifest['infrastructure_failure'] is not None:errors['INFRASTRUCTURE_FAILURE']+=1
        if cmd['changed_on_replay'] or cmd['run_exit_code']!=cmd['replay_exit_code']:errors['REPLAY_MISMATCH']+=1
        if cmd['run_exit_code'] not in [0,1]:errors['RUN_INTEGRITY_EXIT']+=1
        if any(r['status']=='protocol_error' for r in rows):errors['PROTOCOL_ERROR_ROWS']+=1
        statuses.update(r['status'] for r in rows);elapsed.extend(r['elapsed_us'] for r in rows)
        invalid+=sum(h['evidence_valid'] is False for r in rows for h in r['hits'])
        unknown+=sum(h['evidence_valid'] is None for r in rows for h in r['hits']);hits+=sum(len(r['hits']) for r in rows)
        costs=jsonl(root/'costs.jsonl');raw_cost_available+=sum(c['work'] is not None for c in costs)
        samples=defaultdict(list)
        query_map={q['id']:q for q in qs}
        for row,score in zip(rows,scores):
            samples[row['case_id']].append((row,score))
            if query_map[row['case_id']]['no_answer']:
                noanswer_rows+=1;strict['correct' if strict_no_answer(row) else 'incorrect']+=1
                existing['correct' if score['no_answer_correct'] is True else 'incorrect']+=1
                rule_discrepancies+=strict_no_answer(row)!=(score['no_answer_correct'] is True)
        for q in qs:
            observed=samples[q['id']]
            values={field:mean([s[field] for _,s in observed if s[field] is not None]) for field in METRICS}
            cases.append({'id':q['id'],'repo':entry['repo'],'profile':entry['profile'],'family':q['query_family'],
                'global_component':units[q['query_family']],'category':q['category'],'no_answer':q['no_answer'],
                'scheduled_repetitions':entry['repetitions'],'observed_repetitions':len(observed),
                'means':values,'status_counts':dict(Counter(r['status'] for r,_ in observed)),
                'metric_applicable_rows':{f:sum(s[f] is not None for _,s in observed) for f in METRICS}})
        gate=json.loads((root/'gate.json').read_bytes());latency=json.loads((root/'latency-summary.json').read_bytes())
        scorer=json.loads((root/'metrics.json').read_bytes())
        suite_results.append({'key':key,'repo':entry['repo'],'profile':entry['profile'],'scheduled_rows':entry['scheduled_rows'],
            'observed_rows':len(rows),'complete_schedule':complete,'run_exit_code':cmd['run_exit_code'],'replay_exit_code':cmd['replay_exit_code'],
            'gate_status':gate['status'],'gate_reason_count':len(gate['reasons']),'row_status_counts':dict(Counter(r['status'] for r in rows)),
            'unchanged_scorer_descriptive_summary':{k:scorer[k] for k in ['queries','measured_rows','mean_top1','mean_ndcg10','category_means']} if complete else 'not_run; prepare failed and zero-filled missing scorer cases are not observations',
            'existing_success_no_match_latency_summary':latency,'infrastructure_failure':manifest['infrastructure_failure']})
    if invalid:errors['INVALID_SOURCE_HITS']+=invalid
    expected_rows=sum(plan['scheduled_rows'].values())
    if sum(statuses.values())!=expected_rows:errors['GLOBAL_SCHEDULE_INCOMPLETE']+=1
    aggregates={profile:{f:aggregate(cases,profile,f,units) for f in METRICS} for profile in ['native','compat']} if not errors else None
    projections=defaultdict(dict)
    for c in cases:
        if not c['no_answer']:projections[(c['repo'],c['id'])][c['profile']]=c
    paired=[{'repo':repo,'id':qid,'family':v['native']['family'],'global_component':v['native']['global_component'],
             'native':v['native'],'compat':v['compat'],'comparison':'distinct scoring formulas and frozen domains; descriptive projection only'}
            for (repo,qid),v in sorted(projections.items()) if set(v)=={'native','compat'}]
    paired_complete=sum(all(p[profile]['observed_repetitions']==p[profile]['scheduled_repetitions'] for profile in ['native','compat']) for p in paired)
    if len(paired)!=256:errors['PAIRED_PROJECTION_INCOMPLETE']+=1;aggregates=None
    report={'schema_version':1,'scope':'development_only_one_default_product_arm','measurement_integrity':'complete_replayable' if not errors else 'invalid_or_incomplete',
        'errors':dict(errors),'preregistration_receipt_sha256':sha(plan_path.read_bytes()),'source_sha':plan['execution_source_sha'],
        'actual_binary_sha256':plan['actual_binary_sha256'],'admission_sha256':plan['admission_sha256'],
        'query_counts':plan['counts'],'scheduled_rows':plan['scheduled_rows'],'observed_row_count':sum(statuses.values()),
        'row_status_counts':dict(statuses),'suite_results':suite_results,'global_correlation_components':280,
        'paired_answerable_projection_keys':len(paired),'complete_paired_answerable_projections':paired_complete,
        'incomplete_paired_answerable_projections':len(paired)-paired_complete,
        'native_no_answer_queries':sum(c['no_answer'] for c in cases),'native_no_answer_scheduled_rows':noanswer_expected,
        'native_no_answer_observed_rows':noanswer_rows,'native_no_answer_missing_rows':noanswer_expected-noanswer_rows,
        'strict_no_answer_observed_rows':dict(strict),'existing_scorer_no_answer_observed_rows':dict(existing),
        'global_no_answer_accuracy':'invalid/not_computed_when_scheduled_rows_missing' if noanswer_rows!=noanswer_expected else strict['correct']/noanswer_expected,
        'no_answer_rule_discrepancy_count':rule_discrepancies,
        'quality_status':'not_certified; noncomplete rows/no-answer failures and development contamination retained',
        'source_evidence':{'returned_hits':hits,'invalid_hits':invalid,'unverified_hits':unknown,'unknown_never_means_verified':True},
        'cost_availability':{'originating_work_receipts':raw_cost_available,'unavailable_receipts':expected_rows-raw_cost_available,
            'observed_rows_without_work_receipt':sum(statuses.values())-raw_cost_available,'missing_rows_without_observed_cost':expected_rows-sum(statuses.values()),
            'token_money_cost':'not_applicable_to_default_nonsemantic_local_product; no paid provider calls; no inferred zero hidden work',
            'interpretation':'originating work is not current cache-hit work; full costs.jsonl retained'},
        'all_status_rpc_elapsed_us':{'samples':len(elapsed),'min':min(elapsed) if elapsed else None,'max':max(elapsed) if elapsed else None,
            'sum':sum(elapsed),'mean':mean(elapsed),'interpretation':'observed response time including Partial/failures; not complete-retrieval latency or release/tail certification'},
        'descriptive_supported_scorer_means':aggregates,'candidate_minus_baseline_paired_intervals':'not_run; one product arm, native/compat are distinct scoring projections',
        'existing_evaluator_bootstrap':'raw per-suite diagnostics only; 2000 unstratified family draws are not preregistered10000 global paired intervals',
        'unsupported':{'required_facet_coverage':'not_implemented','graph_correctness':'not_implemented','Recall20':'not_implemented',
            'SymbolAccuracy':'not_implemented','DuplicationRate':'not_implemented','full_freshness':'not_run','semantic_ablation':'not_run','release_performance':'not_run'},
        'formal_600_accepted':0,'clean_holdout':0,'protected_body_reads':0,'live_provider_calls':0}
    output.mkdir(parents=True,exist_ok=False);(output/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
    (output/'case-means.jsonl').write_text(''.join(json.dumps(c)+'\n' for c in cases))
    (output/'paired-projections.jsonl').write_text(''.join(json.dumps(c)+'\n' for c in paired))
    print(json.dumps({'measurement_integrity':report['measurement_integrity'],'errors':dict(errors),'row_status_counts':dict(statuses),
        'summary_sha256':sha((output/'summary.json').read_bytes())}))
    return report


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--plan',type=Path,required=True);p.add_argument('--run',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
    analyze(a.plan.resolve(),a.run.resolve(),a.output.resolve())
