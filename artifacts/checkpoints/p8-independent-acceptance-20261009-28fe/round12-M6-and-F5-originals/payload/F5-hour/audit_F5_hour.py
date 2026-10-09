#!/usr/bin/env python3
"""Independent byte/data audit of the fixed F5 GHA hour; no artifact code execution."""
import datetime, hashlib, json, re, sys, traceback, zipfile
from collections import Counter
from pathlib import Path
import review_soak_logic as a

OUT=Path(__file__).resolve().parent
HEAD='c2ad27b2b189cbc98775f20a550d718dd4913038'
ARTIFACT=11588677565
QUERY='p8_runtime_stable_signal'
ROLES=['before_status','symbol','hybrid','after_status']
COUNTERS=['graph_hits','graph_misses','result_hits','result_misses']
def check(name,condition,details=None): return a.check(name,condition,details)
def identity(status):
    d=status['diagnostics'];g=d['retrieval']['generation'];f=status['resolution_freshness']
    assert type(d['process_resources']['pid']) is int and d['process_resources']['pid']>0
    assert type(g['index_epoch']) is int and type(g['evidence_epoch']) is int
    assert isinstance(g['incarnation'],list) and len(g['incarnation'])==16 and all(type(x) is int and 0<=x<=255 for x in g['incarnation'])
    assert f==d['retrieval']['resolution_freshness'] and f['complete'] is True and f['status']=='ready' and f['index_epoch']==g['index_epoch']
    return {'pid':d['process_resources']['pid'],'generation':g}
def cache_audit(z,report):
    rows=[json.loads(line) for line in z.open('p8-runtime/raw.jsonl')]
    operations=[r for r in rows if r['kind']=='operation']
    reads=sorted((r for r in operations if r['operation']=='read'),key=lambda r:r['call_started_ns'])
    builds=sorted((r for r in operations if r['operation']=='build'),key=lambda r:r['finished_ns'])
    expected=b'def p8_runtime_stable_signal():\n    return 7\n'
    assert z.read('p8-runtime/project/stable.py')==expected
    coverage=report['resource_time_coverage'];start=coverage['work_start_ns'];end=coverage['work_end_ns']
    quarter=lambda t:min(3,max(0,(t-start)*4//(end-start)))
    quarters=[dict(reads=0,hits=0,misses=0,invalidations=0,mutations=Counter()) for _ in range(4)]
    for b in builds:quarters[quarter(b['finished_ns'])]['mutations'][b['mutation']['action']]+=1
    previous=None;states=Counter();request_counts={role:Counter() for role in ROLES};mutation_counts=Counter();owners=Counter();expected_wire=Counter();errors=[];observations=[]
    for row in reads:
        try:
            probe=row['cache_probe'];calls=probe['requests'];assert probe['protocol']=='symbol_plus_local_hybrid_cache_v1'
            assert row['status']=='success' and [c['role'] for c in calls]==ROLES
            wanted=[('status',{'aspect':'index'}),('search',{'query':QUERY,'mode':'symbol','top_k':5}),('search',{'query':QUERY,'mode':'hybrid','top_k':1,'retrieval_strategy':'local'}),('status',{'aspect':'index'})]
            for call,(method,args) in zip(calls,wanted):
                assert call['name']==method and call['arguments']==args and call['status']=='success'
                assert type(call['started_ns']) is int and type(call['finished_ns']) is int and call['started_ns']<=call['finished_ns']
                request_counts[call['role']]['success']+=1
                expected_wire[(method,a.sha(a.canonical(args)),a.sha(a.canonical(call['response'])))]+=1
            assert row['call_started_ns']<=calls[0]['started_ns'] and calls[-1]['finished_ns']<=row['finished_ns']
            assert all(x['finished_ns']<=y['started_ns'] for x,y in zip(calls,calls[1:]))
            before,after=calls[0]['response'],calls[3]['response'];current=identity(before)
            assert current==identity(after) and probe['server_pid']==current['pid']
            if previous:assert previous['identity']['pid']==current['pid']
            preceding=[b for b in builds if b['finished_ns']<=row['call_started_ns']]
            last=preceding[-1] if preceding else None
            mutation=None if last is None else {'action':last['mutation']['action'],'ordinal':last['mutation_ordinal'],'operation_id':last['id'],'index_epoch':last['response']['resolution_freshness']['index_epoch']}
            assert probe['preceding_mutation']==mutation
            if mutation:assert mutation['index_epoch']==current['generation']['index_epoch']
            old,new=before['diagnostics']['search_cache'],after['diagnostics']['search_cache'];delta={}
            for k in COUNTERS:
                assert type(old[k]) is int and type(new[k]) is int and 0<=old[k]<=new[k]
                delta[k]=new[k]-old[k]
            graph=delta['graph_hits'],delta['graph_misses'];result=delta['result_hits'],delta['result_misses']
            if graph==(1,0) and result==(0,0):state,owner='hit','graph_result_cache'
            elif graph==(0,1) and result in ((0,0),(0,1),(1,0)):state,owner='miss','graph_result_cache'
            elif graph==(0,0) and result in ((1,0),(0,1)):state,owner=('hit' if result[0] else 'miss'),'result_cache'
            else:raise AssertionError('does not isolate one hybrid cache lookup')
            same=previous is not None and previous['identity']==current
            assert state==('hit' if same else 'miss')
            symbol=calls[1]['response'];assert row['response']==symbol and any(h.get('name')==QUERY and h.get('file_path')=='stable.py' for h in symbol)
            hybrid=calls[2]['response'];assert hybrid.get('_truncated') is not True and hybrid['evidence_summary']['packing']['partial'] is False
            hits=hybrid['machine_pack']['hits'];assert len(hits)==1;hit=hits[0]
            assert hit['file_path']=='stable.py' and hit['symbol_name']==QUERY and type(hit['start_line']) is int and type(hit['end_line']) is int and (hit['start_line'],hit['end_line'])==(1,2)
            meta=hit['metadata'];proof=meta['source_evidence'];span=proof['span'];fresh=meta['source_freshness']
            assert fresh['status']=='current_verified' and fresh['disk_checked'] is True
            assert type(span['start']) is int and type(span['end']) is int and span['start']==0 and span['end'] in (len(expected),len(expected)-1)
            assert proof['source']['byte_len']==len(expected) and proof['source']['encoding']=='utf8' and expected[span['start']:span['end']]==hit['text'].encode()
            pool0,pool1=before['diagnostics']['query_execution'],after['diagnostics']['query_execution']
            for k in ['completed','rejected','cpu_limit','async_limit','queue_limit']:
                assert type(pool0[k]) is int and type(pool1[k]) is int and 0<=pool0[k]<=pool1[k]
            assert pool1['completed']>pool0['completed'] and pool1['rejected']==pool0['rejected']
            assert all(pool0[k]==pool1[k]>0 for k in ['cpu_limit','async_limit','queue_limit'])
            if previous:assert pool0['completed']>=previous['completed']
            obs={'identity':current,'lookup':{'state':state,'owner':owner,'delta':delta},'expected':state,'invalidated':previous is not None and not same,
                 'previous_identity':previous['identity'] if previous else None,'completed':pool1['completed'],'completed_delta':pool1['completed']-pool0['completed'],
                 'source_witness':{'file_path':'stable.py','symbol_name':QUERY,'source_bytes':len(expected),'source_sha256':a.sha(expected),'span':span,
                     'method':'current owned source bytes and exact entity/span; no digest self-report accepted as byte proof'},
                 'worker_scope':'same native server and shared query pool path/counters; not individual OS-thread identity or semantic worker'}
            assert obs==probe['observation'];previous=obs
            states[state]+=1;states['invalidations']+=obs['invalidated'];owners[owner]+=1
            q=quarters[quarter(calls[2]['finished_ns'])];q['reads']+=1;q['hits' if state=='hit' else 'misses']+=1;q['invalidations']+=obs['invalidated']
            mutation_counts[(mutation or {}).get('action','not_observed')]+=1
            observations.append({'id':row['id'],'hybrid_finished_ns':calls[2]['finished_ns'],'quarter':quarter(calls[2]['finished_ns']),'identity':current,'state':state,'owner':owner,'delta':delta,'invalidated':obs['invalidated'],'preceding_mutation':mutation,'shared_pool_completed':pool1['completed']})
        except Exception as error:errors.append({'id':row['id'],'error':type(error).__name__+': '+str(error)})
    claimed=report['cache_reuse']
    check('independent_cache/all_2400_original_probes',len(reads)==2400 and not errors,{'errors':errors})
    check('independent_cache/9600_rpc_original_denominator',all(dict(v)=={'success':2400} for v in request_counts.values()) and claimed['request_counts']==request_counts and claimed['status_request_counts']=={'success':4800})
    check('independent_cache/all_observations_and_quarters',dict(states)==claimed['observed'] and quarters==claimed['time_quarters'] and dict(mutation_counts)==claimed['preceding_mutation_coverage'] and all(q['hits']>0 and q['misses']>0 and q['invalidations']>0 for q in quarters))
    check('independent_cache/original_summary_complete',claimed['validated_reads']==claimed['expected_offered_reads']==claimed['recorded_offered_reads']==claimed['expected_requests_per_role']==2400 and claimed['errors']==[] and claimed['passed'] is True and claimed['temporal_cache_coverage'] is True)
    # Independently bind every compound method/args/response tuple to distinct original RPCs.
    pending={};wire_pairs=Counter()
    for line in z.open('p8-runtime/product/rpc.jsonl'):
        event=json.loads(line);payload=event.get('payload',{})
        if event['event']=='request':pending[payload['id']]=payload
        elif event['event']=='response':
            request=pending[payload['id']];params=request.get('params',{});method=params.get('name')
            if method in ('status','search'):
                result=payload.get('result',{}).get('structuredContent',{});result=result.get('result',result)
                wire_pairs[(method,a.sha(a.canonical(params['arguments'])),a.sha(a.canonical(result)))]+=1
    missing=expected_wire-wire_pairs
    check('independent_cache/each_original_probe_has_wire_method_arguments_response',not missing,{'missing_tuple_count':sum(missing.values()),'expected_compound_rpcs':sum(expected_wire.values()),'all_status_search_rpc_count':sum(wire_pairs.values())})
    (OUT/'independent-cache-observations.json').write_bytes(a.canonical(observations))
    return {'observed':dict(states),'time_quarters':quarters,'owners':dict(owners),'request_counts':request_counts,'original_probe_errors':errors,
            'preceding_mutation_coverage':dict(mutation_counts),'observations_sha256':a.sha((OUT/'independent-cache-observations.json').read_bytes()),
            'scope':'Independent original response/generation/source-byte and wire-pair audit; shared pool reuse, not individual OS-thread or semantic worker identity.'}

def main():
    a.REPORT.update(started_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),scope='Independent original F5 GHA onehour; fixed source retained; conditional M6 success-path bridge evaluated only after complete audit.',reviewer_product_invocations=0)
    api=json.loads((OUT/'official-api/f5artifacts.json').read_text());meta=next(v for v in api['artifacts'] if v['id']==ARTIFACT)
    job=next(v for v in json.loads((OUT/'official-api/f5jobs.json').read_text())['jobs'] if v['id']==113588467257)
    check('official_source_run_artifact_binding',meta['workflow_run']['id']==37858536446 and meta['workflow_run']['head_sha']==HEAD and meta['name']=='p8-soak-'+HEAD and job['status']=='completed' and job['conclusion']=='success')
    names=['Build an exact release product and complete oracle','One-hour actual stdio session with Git switches and catalog churn','Verify every retained runtime and build artifact','Preserve every execution including failures']
    check('official_original_build_hour_verify_upload_success',all(next(s for s in job['steps'] if s['name']==n)['conclusion']=='success' for n in names))
    a.soak(meta)
    out=a.REPORT['artifacts'][str(ARTIFACT)]
    with zipfile.ZipFile(OUT/'11588677565-F5-soak.zip') as z:
        report=a.js(z,'p8-runtime/report.json');plan=a.js(z,'p8-runtime/plan.json');build=a.js(z,'p8-build/build-receipt.json')
        check('report_and_original_CLI_success',report['status']=='passed_observation' and report['exit_code']==0 and report['failures']==[] and report['artifact_seal_status']=='sealed')
        check('all_owned_writers_stopped',report['owned_cleanup']=={'unfinished_work':0,'sampler_stopped':True,'product_stopped':True,'comparison_product_stopped':True,'product_construction_pending':False,'comparison_product_construction_pending':False})
        check('no_terminal_retention_or_unsealed_failure',not any('terminal-retention-failures' in n or 'report-before-seal-failure' in n for n in z.namelist()) and 'terminal_retention_failures' not in report)
        check('original_cap_and_raw_budget',plan['concurrency']==4 and plan['queue_capacity']==128 and z.getinfo('p8-runtime/raw.jsonl').file_size<=536870912)
        check('original_three_targets_complete',set(build['artifacts'])=={'codecortex','p8-oracle','p8-runtime-statistics'})
        check('original_native_source_files_preserved',a.js(z,'p8-build/source-before.json')==a.js(z,'p8-build/source-after.json')==build['source_before'])
        check('actual_toolchain_195_reported',all('1.95.0' in build['toolchain_before'][n]['version'] for n in ['cargo','rustc']) and build['compiler_environment']=={'RUSTC':build['toolchain_before']['rustc']['invocation'],'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':''})
        check('build_log_terminal_success',any(json.loads(l).get('reason')=='build-finished' and json.loads(l).get('success') is True for l in z.read('p8-build/product-build.jsonl').splitlines()))
        out['independent_cache']=cache_audit(z,report)
        out['owned_cleanup']=report['owned_cleanup'];out['artifact_seal_status']=report['artifact_seal_status'];out['toolchain_original_receipt']=build['toolchain_before']
        # Small evidence materialization only. Native binaries, 323MB RPC log and raw stay in original ZIP.
        small={n:z.read(n) for n in ['p8-runtime/report.json','p8-runtime/plan.json','p8-runtime/seal.json','p8-runtime/parity.json','p8-runtime/statistics.json','p8-runtime/statistics-replay.json','p8-runtime/statistics-execution.json','p8-runtime/product/process.json','p8-runtime/full-product/process.json','p8-build/build-receipt.json','p8-build/seal.json']}
        for name,data in small.items():p=OUT/'small-originals'/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
    a.REPORT['failed_checks']=[v for v in a.REPORT['checks'] if not v['passed']]
    a.REPORT['state']='scoped_F5_original_onehour_verified' if not a.REPORT['failed_checks'] else 'review_findings'
    a.REPORT['finished_at_utc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
    a.REPORT['bridge']={'static_report':'../round12-F5-M6-success-bridge/conditional-success-bridge-review.json','static_report_sha256':'3792ce389920bd5c966cf03c40ab6acfb125f25a73c1e76bc53add68a879678b','eligible':not a.REPORT['failed_checks'],'execution_source_remains':HEAD,
        'scope':'Only unchanged M6 long-duration/cache successful behavior; M6 finalization exceptions retain separate controls. Does not erase local M5/M6 failed/incomplete runs, grant same-job build receipt substitution, complete P8-009 hard dependencies, or close original TODO.'}
    a.REPORT['ledger_unchanged']={'total':192,'done':163,'remaining':29,'original_completed_new':0}
    a.write_report()
    print(json.dumps({'state':a.REPORT['state'],'checks':len(a.REPORT['checks']),'failed_checks':a.REPORT['failed_checks'],'cache':out['independent_cache']['observed'],'quarters':out['independent_cache']['time_quarters'],'gap':out['coverage']['maximum_gap_ns'],'report_sha256':a.sha((OUT/'independent-originals-report.json').read_bytes())},ensure_ascii=False))
    if a.REPORT['failed_checks']:raise SystemExit(1)
if __name__=='__main__':
    try:main()
    except Exception as error:
        a.REPORT.update(state='review_harness_incomplete',error=type(error).__name__+': '+str(error));a.write_report();traceback.print_exc();raise SystemExit(2)
