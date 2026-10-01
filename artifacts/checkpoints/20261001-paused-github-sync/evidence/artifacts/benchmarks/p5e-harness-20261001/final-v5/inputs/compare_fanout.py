#!/usr/bin/env python3
"""Paired homogeneous wholequery fanout observations. Quality never traded for speed.
Contiguous 10-request block bootstrap is a proposed dependence guard; approval
must bind this exact script before formal execution. All states remain in times.
"""
import argparse,json,pathlib,random
from compare_mixed import percentile,distribution
from validate_resources import check

def main():
    p=argparse.ArgumentParser()
    for arg in ['baseline','candidate','output']:p.add_argument('--'+arg,type=pathlib.Path,required=True)
    a=p.parse_args();assert not a.output.exists()
    dirs=[a.baseline,a.candidate];load=lambda d,f:json.loads((d/f).read_text())
    plans=[load(d,'plan.json') for d in dirs];issues=[];quality=[];samples=[];resources=[]
    if plans[0]!=plans[1]:issues.append('paired_plan_mismatch')
    expected=plans[0]['repetitions']
    for n,d in enumerate(dirs):
        rows=load(d,'whole-query-observations.json');samples.append(rows)
        seq=[r.get('repetition') for r in rows]
        if seq!=list(range(expected)):issues.append({'version':n,'sequence_unique_complete_order':False})
        if any(type(r.get('elapsed_us')) is not int or r['elapsed_us']<0 for r in rows):issues.append({'version':n,'invalid_time':True})
        resource=check(load(d,'resource-samples.json'));resources.append(resource)
        if resource['exit_code']!=0:issues.append({'version':n,'native_resource_proof':False})
        t=load(d,'termination.json')
        if not(t.get('cancel_ok') is True and t.get('reaped') is True and t.get('forced') is False and t.get('exit_code')==0):issues.append({'version':n,'normal_lifecycle':False})
        typed=load(d,'observations.json');ids=[r['id'] for r in typed]
        if len(ids)!=len(set(ids)) or set(ids)!={q['id'] for q in plans[n]['queries']}:issues.append({'version':n,'typed_requests_unique_complete':False})
        quality.append({'summary':load(d,'summary.json'),'failed_whole_query_checks':[r['repetition'] for r in rows if r['check']['passed'] is not True],'typed_failed_checks':[r['id'] for r in typed if r['check']['passed'] is not True],'scope':'all raw retained;baseline known mapping defects remain quality RED;candidate quality RED blocks applicable gate regardless latency'})
    pairs=list(zip(*[[r['elapsed_us'] for r in rows] for rows in samples]));blocks=[pairs[i:i+10] for i in range(0,len(pairs),10)];rng=random.Random(1905)
    draws={key:[] for key in ['p50_ratio','p95_ratio']}
    if not issues and blocks:
        for _ in range(10000):
            batch=[v for _ in blocks for v in rng.choice(blocks)]
            for key,q in [('p50_ratio',.5),('p95_ratio',.95)]:
                b=percentile([v[0] for v in batch],q)
                if b:draws[key].append(percentile([v[1] for v in batch],q)/b)
    ratios={}
    for key,q in [('p50_ratio',.5),('p95_ratio',.95)]:
        b=percentile([v[0] for v in pairs],q);c=percentile([v[1] for v in pairs],q);v=sorted(draws[key]);point=c/b if b else None
        low=v[249] if len(v)==10000 else None;high=v[9749] if len(v)==10000 else None
        decision='inconclusive'
        if not issues and point is not None:
            if point>1.2:decision='block_profile_point_regression'
            elif point>1.1:decision='review_profile_point_regression'
            elif high is not None and high<=1.2 and len(pairs)>=200:decision='within_numeric_guardrail_not_quality_acceptance'
        ratios[key]={'point':point,'low':low,'high':high,'confidence':.95,'seed':1905,'resamples':len(v),'contiguous_block_size':10,'interpretation':decision}
    a.output.write_text(json.dumps({'status':'invalid_measurement' if issues else 'computed_diagnostic_comparison_not_G5_acceptance','issues':issues,'expected_whole_queries_per_variant':expected,'baseline':distribution([v[0] for v in pairs]),'candidate':distribution([v[1] for v in pairs]),'paired_wholequery_ratios':ratios,'independent_quality':quality,'resource_proofs':resources,'limits':['all Partial/error durations retained,no quality filtering','only homogeneous authored highfanout workload,not holdout','same compiler/options/environment/producer locks external hard prerequisites','baseline unmapped defect never optimization credit;candidate hard quality must pass','explicit owner/auditor approval of block policy pending;computed not wholegate']},indent=2)+'\n')
if __name__=='__main__':main()
