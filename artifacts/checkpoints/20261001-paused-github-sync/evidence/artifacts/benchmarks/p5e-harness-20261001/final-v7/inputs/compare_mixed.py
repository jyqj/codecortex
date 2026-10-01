#!/usr/bin/env python3
"""Recompute same-workload paired mixed-read distributions and block-bootstrap CIs.
All errors/Partial/deadline-censored durations remain in the workload. A sampled
read/build cycle is a block, not 300 independent quality questions.
"""
import argparse,json,pathlib,random,math,unicodedata,hashlib
from validate_resources import check as check_native_resources

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

def canonical_file(path):
    # Python mirror of cc_model::repo_path::is_canonical_file (UTF-8 scalar input).
    return isinstance(path,str) and bool(path) and not any(c in path for c in ['\\',':']) and not any(unicodedata.category(c) in ('Cc','Cs') for c in path) and not any(part in ('','.','..') for part in path.split('/'))

def baseline_path_inventory_only(row, declared_scope=None):
    """Pre-registered KNOWN BASELINE RED cost diagnostic, never task/quality PASS.
    Generic structural predicate; no query IDs or phrase special cases.
    """
    check=row.get('check') or {};raw=row.get('raw') or {};evidence=raw.get('evidence_summary') or {};retrieval=evidence.get('retrieval') or {};fresh=evidence.get('source_freshness') or {};packing=evidence.get('packing') or {};resolution=raw.get('resolution_freshness') or {};hard=(retrieval.get('scope') or {}).get('hard') or {}
    lanes=retrieval.get('lane_receipts',retrieval.get('lanes'))
    if not(row.get('kind')=='read' and row.get('error') is None and row.get('response_classification')=='success' and check.get('status')=='Partial' and check.get('passed') is False):return False
    facets=check.get('facets');proofs=check.get('verified_source_spans')
    if not(isinstance(facets,list) and facets and all(f.get('covered') is True for f in facets) and isinstance(proofs,list) and proofs and all(p.get('valid') is True and p.get('verification_error') is None and isinstance(p.get('span'),dict) and p['span'].get('end',0)>p['span'].get('start',0) for p in proofs)):return False
    if not(isinstance(lanes,list) and {l.get('lane_id') for l in lanes}=={'path','exact_symbol','lexical','grep','graph'} and len(lanes)==5):return False
    path=[l for l in lanes if l.get('lane_id')=='path']
    if len(path)!=1:return False
    lane=path[0];coverage=lane.get('coverage') or {}
    if not(type(lane.get('weight')) in (int,float) and math.isfinite(lane['weight']) and lane['weight']>0 and lane.get('status')=='partial' and lane.get('truncation_reason')=='path_token_limit' and coverage.get('complete') is False and type(lane.get('candidate_count')) is int and lane['candidate_count']>0 and type(coverage.get('total_lower_bound')) is int and coverage['total_lower_bound']>=lane['candidate_count']):return False
    for other in lanes:
        if other.get('lane_id')=='path':continue
        if not(other.get('status')=='complete' and (other.get('coverage') or {}).get('complete') is True and other.get('truncation_reason') is None):return False
    # The registered MCP mixed tool calls have no scope overrides. If a future
    # profile supplies scope, it must supply the independently locked request,
    # not trust an attacker-controlled returned scope as authorization.
    expected=declared_scope or {'path_prefix':None,'languages':None,'file_paths':None,'empty':False}
    if expected.get('empty',False) is not False:return False
    prefix=expected.get('path_prefix');languages=expected.get('languages');files=expected.get('file_paths')
    if not isinstance(hard,dict) or not {'path_prefix','languages','explicit_file_count','empty','path_prefix_truncated'}<=set(hard):return False
    if not(hard.get('path_prefix')==prefix and hard.get('languages')==languages and hard.get('explicit_file_count')==(len(files) if files is not None else None) and hard.get('empty') is False):return False
    def permitted(path):
        if not canonical_file(path):return False
        if prefix is not None and prefix.rstrip('/') and not(path==prefix.rstrip('/') or path.startswith(prefix.rstrip('/')+'/')):return False
        if files is not None and path not in files:return False
        return True
    actual_hits=(raw.get('machine_pack') or {}).get('hits')
    if not(isinstance(actual_hits,list) and actual_hits and all(permitted(h.get('file_path')) and (languages is None or h.get('language') in languages) for h in actual_hits)):return False
    if not(all(permitted(p.get('path')) for p in proofs) and all(permitted(f.get('path')) for f in facets)):return False
    return packing.get('partial') is False and packing.get('omitted_hits')==0 and packing.get('omitted_nodes')==0 and fresh.get('partial') is False and fresh.get('budget_exhausted') is False and fresh.get('omitted_files')=={} and resolution.get('complete') is True and resolution.get('status')=='ready' and resolution.get('reason') is None and hard.get('path_prefix_truncated') is False and raw.get('invalidations')==[]

def count_parity(plans,counts,rows, exact_counts=None):
    issues=[];versions=[]
    for n,(plan,lock,jobs) in enumerate(zip(plans,counts,rows)):
        initial=lock.get('initial_status') or {};final=lock.get('final_status') or {};admitted=len(plan['files']);builds=[]
        if exact_counts is not None:
            for key in ['indexed_files','indexed_chunks','indexed_symbols']:
                if initial.get(key)!=exact_counts[key] or final.get(key)!=exact_counts[key]:issues.append({'version':n,'field':'exact_registered_initial_final_count','key':key,'expected':exact_counts[key]})
        if lock.get('initial_final_and_all_fullbuild_counts_identical') is not True:issues.append({'version':n,'field':'claimed_count_lock_false'})
        for key in ['indexed_files','indexed_chunks','indexed_symbols']:
            if type(initial.get(key)) is not int or initial.get(key)!=final.get(key):issues.append({'version':n,'field':'initial_final_actual_count','key':key})
        if initial.get('indexed_files')!=admitted:issues.append({'version':n,'field':'indexed_files_vs_admitted'})
        for row in jobs:
            if row.get('kind')!='full_build':continue
            raw=row.get('raw') or {};document=raw.get('document_changes') or {}
            work={key:raw.get(key) for key in ['files_scanned','files_parsed','files_skipped','chunks_total','symbols_total']};work['files_projected']=document.get('files_projected')
            valid=work['files_scanned']==admitted and work['files_parsed']==admitted and work['files_skipped']==0 and work['files_projected']==admitted and work['chunks_total']==initial.get('indexed_chunks') and work['symbols_total']==initial.get('indexed_symbols') and raw.get('parse_errors')==[] and document.get('render_failed')==0
            if not valid:issues.append({'version':n,'field':'full_build_actual_work','sequence':row.get('sequence'),'actual':work})
            builds.append({'sequence':row.get('sequence'),'actual_work_counts':work,'same_actual_indexed_work':valid})
        versions.append({'initial':initial,'final':final,'admitted_files':admitted,'all_fullbuild_actual_counts':builds})
    for key in ['indexed_files','indexed_chunks','indexed_symbols']:
        if versions[0]['initial'].get(key)!=versions[1]['initial'].get(key):issues.append({'field':'paired_actual_indexed_counts','key':key})
    return {'source_work_counts_equal':not issues,'issues':issues,'versions':versions,'scope':'actual parsed/projected/scanned/initialfinal/fullbuild/chunks/symbolquantity,independent of qualityPartial/lifecycle/provenance gates'}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for flag in ['baseline','candidate','output']:parser.add_argument('--'+flag,type=pathlib.Path,required=True)
    parser.add_argument('--workload-count-lock',type=pathlib.Path,default=pathlib.Path(__file__).parent/'EXACT-MIXED-WORKLOAD-COUNT-LOCK.json')
    args=parser.parse_args();assert not args.output.exists();runs=[args.baseline,args.candidate]
    plans=[json.loads((r/'plan.json').read_text()) for r in runs];assert plans[0]==plans[1],'source/query/budget/seed/offered profile mismatch'
    summary=[json.loads((r/'summary.json').read_text()) for r in runs]
    counts=[json.loads((r/'workload-count-lock.json').read_text()) for r in runs]
    rows=[json.loads((r/'observations.json').read_text()) for r in runs]
    issues=[];quality_issues=[[],[]];known_baseline_red=[];expected=expected_jobs(plans[0]);observed_overlaps=[]
    for n,rundir in enumerate(runs):
        seen=set()
        if len(rows[n])!=len(expected):issues.append({'version':n,'field':'expected_job_count','expected':len(expected),'actual':len(rows[n])})
        for r in rows[n]:
            seq=r['sequence']
            if seq in seen or not 0<=seq<len(expected):issues.append({'version':n,'field':'sequence_unique_range','sequence':seq});continue
            seen.add(seq)
            for key,value in expected[seq].items():
                if r.get(key)!=value:issues.append({'version':n,'field':'expected_offered_query_family_repetition_order','sequence':seq,'key':key,'expected':value,'actual':r.get(key)})
        native=check_native_resources(json.loads((rundir/'resource-samples.json').read_text()))
        if native['exit_code']!=0:issues.append({'version':n,'field':'native_resource_proof','actual':native})
        for row in rows[n]:
            times=[row.get(k) for k in ['offered_us','started_us','finished_us','client_queue_us','service_and_transport_us','end_to_end_us']]
            if not(all(type(t) is int and t>=0 for t in times) and times[2]>=times[1]>=times[0] and times[3]==times[1]-times[0] and times[4]==times[2]-times[1] and times[5]==times[2]-times[0]):issues.append({'version':n,'field':'invalid_clock_or_decomposition','sequence':row.get('sequence')})
            if type(row.get('worker')) is not int or not 0<=row['worker']<plans[n]['concurrency']:issues.append({'version':n,'field':'invalid_worker','sequence':row.get('sequence')})
            if row.get('check',{}).get('passed') is not True:
                failure={'version':n,'field':'raw_request_strict_quality_red','sequence':row.get('sequence'),'actual':row.get('check')};quality_issues[n].append(failure)
                if n==0 and baseline_path_inventory_only(row):known_baseline_red.append(failure)
                else:issues.append(failure)
        allowed_baseline=n==0 and len(quality_issues[0])==len(known_baseline_red) and summary[n].get('failures')==len(known_baseline_red) and summary[n].get('exit_code')==(1 if known_baseline_red else 0)
        if not((allowed_baseline or (summary[n].get('exit_code')==0 and summary[n].get('failures')==0)) and summary[n].get('completed_jobs')==len(expected) and summary[n].get('offered_jobs')==len(expected) and summary[n].get('source_drift')==[] and summary[n].get('worker_failures')==[]):
            issues.append({'version':n,'field':'final_summary_measurement_or_candidate_quality_failure','actual':summary[n]})
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
    count_registry=json.loads(args.workload_count_lock.read_text());registered=count_registry['plans'][str(plans[0]['concurrency'])];source_plan=pathlib.Path(registered['path']);locked_plan=json.loads(source_plan.read_text());
    if hashlib.sha256(source_plan.read_bytes()).hexdigest()!=registered['sha256'] or locked_plan!=plans[0]:issues.append({'field':'registered_exact_source_workload_mismatch'})
    count_proof=count_parity(plans,counts,rows,count_registry['fullbuild_and_status_exact_constraints'])
    if count_proof['issues']:issues.append({'field':'registered_exact_work_counts','actual':count_proof['issues']})
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
    args.output.write_text(json.dumps({'schema_version':1,'status':'invalid_workload_comparison' if issues else 'computed_observations_not_G5_acceptance','concurrency_capacity':plans[0]['concurrency'],'observed_overlap':observed_overlaps,'expected_reads':sum(j['kind']=='read' for j in expected),'expected_fullbuilds':sum(j['kind']=='full_build' for j in expected),'baseline':distribution([v[0] for v in paired]),'candidate':distribution([v[1] for v in paired]),'all_status_counts':statuses,'paired_block_bootstrap':report,'baseline_gate':summary[0]['status'],'candidate_gate':summary[1]['status'],'source_work_counts_equal':count_proof['source_work_counts_equal'],'source_work_count_proof':count_proof,'known_baseline_strict_quality_red':known_baseline_red,'baseline_quality_issues':quality_issues[0],'candidate_quality_issues':quality_issues[1],'measurement_validity_issues':issues,'baseline_cost_diagnostic_eligibility':'only structured path_token_limit inventoryPartial with allotherlanes/source/facets/packing/freshness/clock/resources/lifecycle valid;baseline strictred retained,neverqualityPASS','quality_or_measurement_issues':issues,'issues':issues,'workload_count_locks':counts,'limits':['one fixed mixed workload;cycle dependence retained,no reliable p99 claim','all error/Partial/timeout durations included,not best-of','baseline/candidate compiler/env/build-profile equality requires immutable producer receipts','quality/source/lifecycle/resources hard gates independent;ratio CI never waives failures','counterfactual facet loss cannot buy a passed optimization','C16 is capacity;report observed actual overlap separately']},indent=2)+'\n')
if __name__=='__main__':main()
