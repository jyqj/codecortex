"""Verify all finite cases and summarize, preserving unfavorable ready results."""
from pathlib import Path
import hashlib,json,statistics,subprocess
out=Path(__file__).resolve().parent;repo=out.parents[2]
protocol=json.loads((out/'AB-PROTOCOL.json').read_text());rows=json.loads((out/'AB-RUNS.json').read_text())
assert len(rows)==8
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
for path,expected in protocol['source_files_sha256'].items():
    actual=(repo/path) if path.startswith('crates/') else(out/path)
    assert sha(actual)==expected,path
base='6db4d396e5d994388ada1e97c3d28947ffdb9c81'
assert (out/'driver/src/baseline_status.rs').read_bytes()==subprocess.check_output(['git','show',base+':crates/cc-server/src/capability_status.rs'],cwd=repo)
assert all(r['exit_code']==0 and r['ready'] for r in rows)
data={}
for row in rows:
    case=out/'ab'/row['case'];r=json.loads((case/'receipt.json').read_text());polls=json.loads((case/'polls.json').read_text())
    assert r['poll_interval_ms']==200 and r['timeout_seconds']==120 and r['read_pool_size']==1
    assert r['max_batch_items']==16 and r['dimension']==2 and r['real_provider_calls']==0
    assert r['eligible']==r['published']==r['fake_inputs']==row['files'] and r['uncovered']==0
    assert len(r['steady_128_ms'])==128 and len(polls)==r['polls']
    errors=[p for p in polls if p['status']['retrieval']['index_state']=='error']
    assert len(errors)==r['latest_errors']
    if row['mode']=='candidate':
        assert not errors
        previous=0
        for p in polls:
            value=p['status'];d=value['retrieval'];n=row['files']
            assert d['spec']=='retrieval-capabilities-v2' and d['consistency']=='point_in_time'
            assert d['generation_scope']=='observed_database_snapshot' and d['service_state_scope']=='process_observed_separately'
            assert d['identity_validation']=='checked_at_observation_boundary' and d['semantic_active_space']
            assert value['indexed_files']==value['indexed_symbols']==n
            assert d['resolution_freshness']['index_epoch']==d['generation']['index_epoch']
            assert previous<=d['dense_published']<=n;previous=d['dense_published']
            assert d['dense_desired']==n and 0<=d['semantic_pending']<=n-d['dense_published']
            assert d['semantic_failed']==0
            assert (d['dense_state']=='ready')==(d['dense_published']==n)
    else:
        for p in errors:
            d=p['status']['retrieval']
            assert d['error']['retryable'] and 'after 3 attempts' in d['error']['message']
            assert d['generation'] is None and d['dense_state']!='ready'
    data[row['case']]=(r,polls)
summary=[]
for n in [1000,5000]:
    group={}
    for mode in ['baseline','candidate']:
        cases=[data[r['case']] for r in rows if r['files']==n and r['mode']==mode]
        rs=[r for r,_ in cases];poll_durations=[p['duration_ms'] for _,ps in cases for p in ps]
        steady=[v for r in rs for v in r['steady_128_ms']]
        group[mode]={'polls':sum(r['polls'] for r in rs),'status_errors':sum(r['latest_errors'] for r in rs),
            'status_error_fraction':sum(r['latest_errors'] for r in rs)/sum(r['polls'] for r in rs),
            'poll_mean_ms':statistics.mean(poll_durations),'status_total_ms':sum(poll_durations),
            'steady_256_mean_ms':statistics.mean(steady),'steady_256_median_ms':statistics.median(steady),
            'ready_observation_mean_ms':statistics.mean(r['ready_ms'] for r in rs),
            'all_ready':True}
    pairs=[]
    for pair in [1,2]:
        b=data[f'{n}-pair{pair}-baseline'][0];c=data[f'{n}-pair{pair}-candidate'][0]
        pairs.append({'pair':pair,'baseline_ready_ms':b['ready_ms'],'candidate_ready_ms':c['ready_ms'],
            'candidate_minus_baseline_ready_ms':c['ready_ms']-b['ready_ms'],
            'candidate_steady_mean_ms':statistics.mean(c['steady_128_ms']),
            'baseline_steady_mean_ms':statistics.mean(b['steady_128_ms'])})
    b=group['baseline'];c=group['candidate']
    summary.append({'files':n,'arms':group,'pairs':pairs,
        'poll_mean_reduction_fraction':1-c['poll_mean_ms']/b['poll_mean_ms'],
        'steady_mean_reduction_fraction':1-c['steady_256_mean_ms']/b['steady_256_mean_ms'],
        'ready_mean_reduction_fraction':1-c['ready_observation_mean_ms']/b['ready_observation_mean_ms']})
result={'source_sha':protocol['source_sha'],'binary_sha256':protocol['binary_sha256'],'runs':8,'summary':summary,
    'structural_count_queries':{'stable_wired_eligible_v1':13,'stable_wired_eligible_v2':6,'unwired_v1':8,'unwired_v2':2,
        'basis':'static source-level COUNT/subquery inventory, not measured SQLite VM/page/fullscan steps',
        'removed':'unused chunks/symbol_refs/call_edges/test_edges/routes/literal_index counts; stale correlated COUNT and zero-eligible reason scan; coverage inner retry; ordinary epoch outer retry'},
    'limits':['two pairs per size, no inferential confidence intervals','in-process actual production status/worker, no MCP latency measurement',
      '1k first candidate ready 197.747ms slower and one extra poll retained','200ms serial sleep gap unchanged; start-to-start interval includes call duration',
      '5k ready observation 5.56% lower mean does not isolate worker-throughput causality','warm 128 calls per run occur only after measured backfill',
      'all other current production shared between arms; exact v1 status and equivalent freshness SQL baseline',
      '100k not run; no PR101 scale inference','Linux identity tested; other VFS/platforms untested'],
    'real_provider_calls':0,'heldout_read':False,'fault_kill_gc_wal':'not_run'}
(out/'AB-SUMMARY.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
