#!/usr/bin/env python3
"""Recompute hierarchical paired quality CIs from factorial per-query deltas.
Repository/authored-corpus and query-family are units; repeats/EN-ZH pairs are not.
No gold or product inputs are rewritten. Missing deltas remain unavailable.
"""
import argparse,collections,json,random,math,pathlib,itertools,hashlib

def interval(clusters,seed):
    if not clusters:return None
    rng=random.Random(seed);repos=sorted(clusters)
    observed=sum(sum(f.values())/len(f) for f in clusters.values())/len(repos)
    values=[]
    for _ in range(10000):
        draws=[]
        for _ in repos:
            families=clusters[rng.choice(repos)];keys=sorted(families)
            draws.append(sum(families[rng.choice(keys)] for _ in keys)/len(keys))
        values.append(sum(draws)/len(draws))
    values.sort()
    return {'mean':observed,'low':values[249],'high':values[9749], 'resamples':10000,'seed':seed,'confidence':0.95,'repository_or_authored_corpus_units':len(repos),'query_family_units':sum(map(len,clusters.values())),'claim':'paired_fixed_development_inputs_not_holdout_or_general_quality'}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--ablation',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args()
    assert not a.output.exists(),'immutable report';data=json.loads(a.ablation.read_text());pairs=collections.defaultdict(list)
    plan=json.loads((a.ablation.parent/'plan.json').read_text())
    query_locks={};gold={}
    for dataset in plan['datasets']:
        suite_path=pathlib.Path(dataset['suite']);suite=json.loads(suite_path.read_text());query_path=(suite_path.parent/suite['queries']).resolve();raw=query_path.read_bytes();queries=[json.loads(line) for line in raw.decode().splitlines() if line.strip()]
        gold[dataset['id']]={q['id']:q for q in queries}
        query_locks[dataset['id']]={'suite_sha256':hashlib.sha256(suite_path.read_bytes()).hexdigest(),'queries_sha256':hashlib.sha256(raw).hexdigest(),'ranking_population':sum(q.get('no_answer') is not True for q in queries),'no_answer_population':sum(q.get('no_answer') is True for q in queries)}
    not_applicable=[]
    expected_edges=len(plan['controls'])*(2**(len(plan['controls'])-1))*len(plan['datasets'])
    factor_ids=[c['id'] for c in plan['controls']]
    expected_keys={(d['id'],factor,tuple(sorted(fixed))) for d in plan['datasets'] for factor in factor_ids for bits in itertools.product([False,True],repeat=len(factor_ids)-1) for fixed in [[f for f,on in zip([x for x in factor_ids if x!=factor],bits) if on]]}
    actual_keys=[(e['dataset'],e['factor'],tuple(sorted(e['fixed_enabled']))) for e in data['one_factor_edges']]
    edge_issues={'missing':sorted(expected_keys-set(actual_keys)),'unexpected':sorted(set(actual_keys)-expected_keys),'duplicate_count':len(actual_keys)-len(set(actual_keys))}
    unavailable_edges=len(edge_issues['missing'])
    for edge in data['one_factor_edges']:
        key=(edge['factor'],tuple(edge['fixed_enabled']))
        pairs[key].append(edge)
    case_coverage_issues=[]
    for edge in data['one_factor_edges']:
        expected_case_ids=set(gold.get(edge['dataset'],{}))
        actual_case_ids=[case['id'] for case in edge['cases']]
        missing_ids=sorted(expected_case_ids-set(actual_case_ids));unknown_ids=sorted(set(actual_case_ids)-expected_case_ids);duplicates=len(actual_case_ids)-len(set(actual_case_ids))
        if missing_ids or unknown_ids or duplicates:
            case_coverage_issues.append({'dataset':edge['dataset'],'factor':edge['factor'],'fixed_enabled':edge['fixed_enabled'],'missing_case_ids':missing_ids,'unknown_case_ids':unknown_ids,'duplicate_count':duplicates})
    reports=[]
    for (factor,fixed),edges in sorted(pairs.items()):
        metrics={};missing=[]
        for metric in ['delta_top1','delta_ndcg10']:
            clusters={}
            for e in edges:
                family_values=collections.defaultdict(list)
                for case in e['cases']:
                    query=gold.get(e['dataset'],{}).get(case['id'])
                    if query is None: missing.append({'dataset':e['dataset'],'case_id':case['id'],'metric':metric,'reason':'not_in_locked_originalqueries'});continue
                    if query.get('no_answer') is True:
                        not_applicable.append({'dataset':e['dataset'],'case_id':case['id'],'metric':metric,'reason':'locked_query_no_answer_ranking_not_applicable','factor':e['factor'],'fixed_enabled':e['fixed_enabled']});continue
                    value=case.get(metric)
                    if type(value) not in (int,float) or not math.isfinite(value):missing.append({'dataset':e['dataset'],'case_id':case['id'],'metric':metric});continue
                    family_values[case['query_family']].append(value)
                clusters[e['dataset']]={family:sum(v)/len(v) for family,v in family_values.items() if v}
                if not clusters[e['dataset']]:del clusters[e['dataset']]
            metrics[metric]={'clusters':clusters,'interval':None if case_coverage_issues else interval(clusters,1905)}
        reports.append({'factor':factor,'fixed_enabled':list(fixed),'metrics':metrics,'missing_deltas':missing})
    a.output.write_text(json.dumps({'schema_version':1,'status':'missing_measurement' if case_coverage_issues or edge_issues['missing'] or edge_issues['unexpected'] or edge_issues['duplicate_count'] or any(r['missing_deltas'] for r in reports) else 'computed_development_CI_not_acceptance', 'expected_edges':expected_edges,'observed_edges':len(data['one_factor_edges']),'unavailable_edges':unavailable_edges,'edge_identity_validation':edge_issues,'case_identity_coverage_issues':case_coverage_issues,'paired_quality_clustered':reports,'locked_query_populations':query_locks,'ranking_total_population':sum(d['ranking_population'] for d in query_locks.values()),'ranking_not_applicable':not_applicable,'no_answer_obligation':'separate everycell executed NoMatch/zero_validbodies/completeabsence proof required;notderivedfrom null or caseID and notrankzero','dataset_units':'source=immutable historical P0 CodeCortex source repository subset; other suites=authored synthetic corpus units, not real-repository sampling','limitations':['51 questions are development data, not holdout','repetitions averaged by existing per-case scorer before clustering','translation pairs share query_family','no interval substitutes scope/source/facet/no-answer hard gates','candidate full-on source byte-span overlay evaluated independently','intentional-off regressions never credited as acceleration']},indent=2)+'\n')
if __name__=='__main__':main()
