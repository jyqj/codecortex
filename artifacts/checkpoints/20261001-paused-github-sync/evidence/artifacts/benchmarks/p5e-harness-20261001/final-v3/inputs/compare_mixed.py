#!/usr/bin/env python3
"""Recompute same-workload paired mixed-read distributions and block-bootstrap CIs.
All errors/Partial/deadline-censored durations remain in the workload. A sampled
read/build cycle is a block, not 300 independent quality questions.
"""
import argparse,json,pathlib,random,math

def percentile(values,p):
    if not values:return None
    values=sorted(values);return values[max(0,math.ceil(len(values)*p)-1)]
def distribution(values):
    return {'N':len(values),'p50_us':percentile(values,.5),'p95_us':percentile(values,.95),'max_us':max(values) if values else None,'tail_claim':'fixed_mixed_workload_observed_p95_not_p99' if len(values)>=200 else 'insufficient_for_tail_claim'}
def expected_jobs(plan):
    mask=(1<<64)-1
    def order(n,seed):
        a=list(range(n));state=seed
        for i in range(n-1,0,-1):
            if state==0:state=0x9e3779b97f4a7c15
            state=(state^(state<<13))&mask;state^=state>>7;state=(state^(state<<17))&mask
            j=state%(i+1);a[i],a[j]=a[j],a[i]
        return a
    jobs=[];reads=0
    for rep in range(plan['repetitions']):
        for qi in order(len(plan['queries']),(plan['seed']+rep)&mask):
            jobs.append({'kind':'read','query_id':plan['queries'][qi]['id'],'repetition':rep});reads+=1
            if reads%plan['build_every']==0:jobs.append({'kind':'full_build','query_id':None,'repetition':None})
    for seq,job in enumerate(jobs):job.update(sequence=seq,offered_us=seq*plan['offered_interval_us'])
    return jobs

def overlap(rows):
    def peak(kind=None):
        events=[]
        for r in rows:
            if kind is not None and r['kind']!=kind:continue
            if r['finished_us']<=r['started_us']:continue
            events.extend([(r['started_us'],1),(r['finished_us'],-1)])
        active=maximum=0
        for _,delta in sorted(events):active+=delta;maximum=max(maximum,active)
        return maximum
    return {'observed_peak_requests_in_flight':peak(),'observed_peak_reads_in_flight':peak('read'),'observed_peak_builds_in_flight':peak('full_build'),'definition':'interval started_us<=t<finished_us;C is worker capacity,not claim of observed C parallelism'}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for flag in ['baseline','candidate','output']:parser.add_argument('--'+flag,type=pathlib.Path,required=True)
    args=parser.parse_args();assert not args.output.exists();runs=[args.baseline,args.candidate]
    plans=[json.loads((r/'plan.json').read_text()) for r in runs];assert plans[0]==plans[1],'source/query/budget/seed/offered profile mismatch'
    summary=[json.loads((r/'summary.json').read_text()) for r in runs]
    counts=[json.loads((r/'workload-count-lock.json').read_text()) for r in runs]
    rows=[json.loads((r/'observations.json').read_text()) for r in runs]
    issues=[];expected=expected_jobs(plans[0]);observed_overlaps=[]
    for n,rundir in enumerate(runs):
        seen=set()
        if len(rows[n])!=len(expected):issues.append({'version':n,'field':'expected_job_count','expected':len(expected),'actual':len(rows[n])})
        for r in rows[n]:
            seq=r['sequence']
            if seq in seen or not 0<=seq<len(expected):issues.append({'version':n,'field':'sequence_unique_range','sequence':seq});continue
            seen.add(seq)
            for key,value in expected[seq].items():
                if r.get(key)!=value:issues.append({'version':n,'field':'expected_offered_query_family_repetition_order','sequence':seq,'key':key,'expected':value,'actual':r.get(key)})
        term=json.loads((rundir/'termination.json').read_text())
        if not(term.get('cancel_ok') is True and term.get('reaped') is True and term.get('forced') is False and term.get('exit_code')==0 and term.get('lifecycle_passed') is True):issues.append({'version':n,'field':'normal_owned_lifecycle','actual':term})
        lock=counts[n]
        if lock.get('initial_final_and_all_fullbuild_counts_identical') is not True:issues.append({'version':n,'field':'workload_count_lock_false','actual':lock})
        for k in ['indexed_files','indexed_chunks','indexed_symbols']:
            if lock['initial_status'][k]!=lock['final_status'][k] or lock['initial_status'][k]!=counts[1-n]['initial_status'][k]:issues.append({'version':n,'field':'actual_work_counts','key':k})
        for r in rows[n]:
            if r['kind']!='full_build':continue
            v=r.get('raw') or {};dc=v.get('document_changes') or {}
            if not(v.get('files_scanned')==len(plans[n]['files']) and v.get('files_parsed')==len(plans[n]['files']) and v.get('files_skipped')==0 and dc.get('files_projected')==len(plans[n]['files']) and v.get('symbols_total')==lock['initial_status']['indexed_symbols'] and v.get('chunks_total')==lock['initial_status']['indexed_chunks']):issues.append({'version':n,'field':'fullbuild_work','sequence':r['sequence'],'actual':v})
        o=overlap(rows[n]);observed_overlaps.append(o)
        if o['observed_peak_requests_in_flight']>plans[n]['concurrency']:issues.append({'version':n,'field':'bounded_worker_capacity','actual':o})
    if len(rows[0])!=len(rows[1]):issues.append({'field':'paired_row_count'})
    paired=[];blocks={};statuses=[{},{}];cycle_width=plans[0]['build_every']+1
    for a,b in zip(*rows):
        for key in ['sequence','kind','query_id','repetition','offered_us']:
            if a.get(key)!=b.get(key):issues.append({'field':'paired_offered_work','key':key})
        if a['kind']!='read' or b['kind']!='read':continue
        seq=a['sequence'];pair=(a['end_to_end_us'],b['end_to_end_us']);paired.append(pair);blocks.setdefault(seq//cycle_width,[]).append(pair)
        for n,r in enumerate([a,b]):statuses[n][r['check']['status']]=statuses[n].get(r['check']['status'],0)+1
    rng=random.Random(1905);keys=sorted(blocks);boot={'p50_ratio':[],'p95_ratio':[]}
    if not issues:
        for _ in range(10000):
            sample=[pair for _ in keys for pair in blocks[rng.choice(keys)]]
            for name,p in [('p50_ratio',.5),('p95_ratio',.95)]:
                x=percentile([v[0] for v in sample],p);y=percentile([v[1] for v in sample],p)
                if x:boot[name].append(y/x)
    report={}
    for name,p in [('p50_ratio',.5),('p95_ratio',.95)]:
        x=percentile([v[0] for v in paired],p);y=percentile([v[1] for v in paired],p);draws=sorted(boot[name]);point=y/x if x else None
        low=draws[249] if len(draws)==10000 else None;high=draws[9749] if len(draws)==10000 else None;state='inconclusive'
        if not issues and point is not None and point>1.2:state='block_profile_point_regression'
        elif not issues and point is not None and point>1.1:state='review_profile_point_regression'
        elif high is not None and high<=1.2 and len(paired)>=200:state='within_block_threshold_not_whole_gate'
        report[name]={'point':point,'low':low,'high':high,'confidence':.95,'resamples':len(draws),'seed':1905,'workload_cycle_blocks':len(keys),'interpretation':state}
    args.output.write_text(json.dumps({'schema_version':1,'status':'invalid_workload_comparison' if issues else 'computed_observations_not_G5_acceptance','concurrency_capacity':plans[0]['concurrency'],'observed_overlap':observed_overlaps,'expected_reads':sum(j['kind']=='read' for j in expected),'expected_fullbuilds':sum(j['kind']=='full_build' for j in expected),'baseline':distribution([v[0] for v in paired]),'candidate':distribution([v[1] for v in paired]),'all_status_counts':statuses,'paired_block_bootstrap':report,'baseline_gate':summary[0]['status'],'candidate_gate':summary[1]['status'],'source_work_counts_equal':not issues,'issues':issues,'workload_count_locks':counts,'limits':['one fixed mixed workload;cycle dependence retained,no reliable p99 claim','all error/Partial/timeout durations included,not best-of','baseline/candidate compiler/env/build-profile equality requires immutable producer receipts','quality/source/lifecycle/resources hard gates independent;ratio CI never waives failures','counterfactual facet loss cannot buy a passed optimization','C16 is capacity;report observed actual overlap separately']},indent=2)+'\n')
if __name__=='__main__':main()
