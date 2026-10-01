#!/usr/bin/env python3
"""Recompute hierarchical paired quality CIs from factorial per-query deltas.
Repository/authored-corpus and query-family are units; repeats/EN-ZH pairs are not.
No gold or product inputs are rewritten. Missing deltas remain unavailable.
"""
import argparse,collections,json,random,math,pathlib

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
    expected_edges=len(plan['controls'])*(2**(len(plan['controls'])-1))*len(plan['datasets'])
    unavailable_edges=expected_edges-len(data['one_factor_edges'])
    for edge in data['one_factor_edges']:
        key=(edge['factor'],tuple(edge['fixed_enabled']))
        pairs[key].append(edge)
    reports=[]
    for (factor,fixed),edges in sorted(pairs.items()):
        metrics={};missing=[]
        for metric in ['delta_top1','delta_ndcg10']:
            clusters={}
            for e in edges:
                family_values=collections.defaultdict(list)
                for case in e['cases']:
                    value=case.get(metric)
                    if not isinstance(value,(int,float)) or not math.isfinite(value):missing.append({'dataset':e['dataset'],'case_id':case['id'],'metric':metric});continue
                    family_values[case['query_family']].append(value)
                clusters[e['dataset']]={family:sum(v)/len(v) for family,v in family_values.items() if v}
                if not clusters[e['dataset']]:del clusters[e['dataset']]
            metrics[metric]={'clusters':clusters,'interval':interval(clusters,1905)}
        reports.append({'factor':factor,'fixed_enabled':list(fixed),'metrics':metrics,'missing_deltas':missing})
    a.output.write_text(json.dumps({'schema_version':1,'status':'missing_measurement' if unavailable_edges or any(r['missing_deltas'] for r in reports) else 'computed_development_CI_not_acceptance', 'expected_edges':expected_edges,'observed_edges':len(data['one_factor_edges']),'unavailable_edges':unavailable_edges,'paired_quality_clustered':reports,'dataset_units':'source=current CodeCortex source repository subset; other suites=authored synthetic corpus units, not real-repository sampling','limitations':['51 questions are development data, not holdout','repetitions averaged by existing per-case scorer before clustering','translation pairs share query_family','no interval substitutes scope/source/facet/no-answer hard gates','candidate full-on source byte-span overlay evaluated independently','intentional-off regressions never credited as acceleration']},indent=2)+'\n')
if __name__=='__main__':main()
