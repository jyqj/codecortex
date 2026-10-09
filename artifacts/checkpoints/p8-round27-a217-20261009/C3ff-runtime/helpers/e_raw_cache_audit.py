"""Read-only, bounded supplementary audit of original fixed C3ff runtime evidence.

This helper never generates product evidence or upgrades historical observations.
The caller must first verify the original ZIP, source, receipt and full seals.
"""
import collections
import hashlib
import json
from pathlib import Path

HEAD = '3ffcefc3b28ee1a4ed80caecebd7208a45c3e302'
RUN = 37908825814
LIFECYCLE_RUN = 37908825715
PRODUCT_MANIFEST = 'be60b3d1bb7b90a8f8023506e9d9570c7b445d67686f047e4ef12fde75f7e835'
RAW_LIMIT = 512 * 1024 * 1024
LINE_LIMIT = 16 * 1024 * 1024  # Auditor safety bound; exceeding is blocked, never truncated/pass.


def file_hash(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'))


def value_hash(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def lines(path):
    with Path(path).open('rb') as stream:
        while True:
            offset = stream.tell()
            line = stream.readline(LINE_LIMIT + 1)
            if not line:
                return
            assert len(line) <= LINE_LIMIT, 'auditor line bound exceeded; retain original and block'
            yield offset, json.loads(line)


def load_runtime_rows(path):
    """Keep small timing/resource projections; read each complete probe on demand."""
    path = Path(path)
    assert path.stat().st_size <= RAW_LIMIT
    kept, offsets = [], {}
    counts = collections.Counter()
    for offset, row in lines(path):
        counts[row['kind']] += 1
        if row['kind'] == 'operation':
            assert row['id'] not in offsets
            offsets[row['id']] = offset
            compact = {k: v for k, v in row.items() if k not in ('cache_probe', 'response')}
            if row['operation'] == 'build':
                compact['response'] = {'resolution_freshness': row['response']['resolution_freshness']}
            kept.append(compact)
        elif row['kind'] in ('resources', 'endpoint_status', 'endpoint_public'):
            kept.append(row)
    return kept, offsets, dict(counts)


def confirmed_cleanup(report):
    cleanup = report['owned_cleanup']
    assert type(cleanup['unfinished_work']) is int and cleanup['unfinished_work'] == 0
    assert all(cleanup[k] is True for k in ('sampler_stopped', 'product_stopped', 'comparison_product_stopped'))
    assert all(cleanup[k] is False for k in ('product_construction_pending', 'comparison_product_construction_pending'))
    assert report['artifact_seal_status'] == 'sealed' and report['artifact_seal'] == 'seal.json'
    assert 'finalization_error' not in report and 'failed_artifact_seal' not in report and 'pre_seal_report' not in report
    if 'work_cleanup' in report:
        assert report['work_cleanup']['still_running'] == 0
        assert report['work_cleanup']['cleanup_bound_seconds'] == 70
    return cleanup


def audit_cache(runtime, measurement, plan, report, raw, offsets):
    """All original probes; independent quarters/counts plus fixed-source validator.

    Product RPC correspondence uses exact public arguments and complete decoded
    response hashes. Status snapshots are checked as a multiset because resource
    sampling may interleave identical status calls; no unique status request-ID
    attribution is claimed. All search calls are checked in original wire order.
    """
    m = Path(measurement)
    assert plan['profile'] == 'soak' and plan['operations'] == 3601
    assert report['status'] == 'passed_observation' and report['exit_code'] == 0
    protocol = plan['read_protocol']
    expected = 2400
    roles = tuple(runtime.SOAK_READ_ROLES)
    assert protocol['name'] == runtime.SOAK_READ_PROTOCOL
    assert protocol['offered_read_operations'] == expected
    assert protocol['planned_request_counts'] == dict.fromkeys(roles, expected)
    assert protocol['request_roles'] == list(roles) and protocol['strategy'] == 'local'
    assert protocol['rpc_timeout_seconds'] == dict(before_status=30, symbol=60, hybrid=60, after_status=30)
    assert protocol['rpc_timeout_sum_seconds'] == 180 and plan['request_timeout_seconds'] == 60
    assert plan['queue_capacity'] == 128 and plan['resource_interval_seconds'] == 1
    assert report['actual_concurrency']['maximum'] == 1 and report['actual_concurrency']['read_build_overlap'] is False
    operations = [r for r in raw if r['kind'] == 'operation']
    reads = sorted((r for r in operations if r['operation'] == 'read'), key=lambda r:r['call_started_ns'])
    builds = sorted((r for r in operations if r['operation'] == 'build'), key=lambda r:r['finished_ns'])
    assert len(reads) == expected and len(builds) == 1201
    actual_order = sorted(operations, key=lambda r:r['call_started_ns'])
    assert all(a['finished_ns'] <= b['call_started_ns'] for a,b in zip(actual_order,actual_order[1:])), 'soak isolation not observed'
    start = report['resource_time_coverage']['work_start_ns']
    end = report['resource_time_coverage']['work_end_ns']
    assert end > start and report['observed_work_ns'] >= 3600_000_000_000
    quarter = lambda t:min(3,max(0,(t-start)*4//(end-start)))
    buckets = [dict(reads=0,hits=0,misses=0,invalidations=0,mutations={}) for _ in range(4)]
    for row in builds:
        assert row['status'] == 'success'
        b = buckets[quarter(row['finished_ns'])]['mutations'];action=row['mutation']['action'];b[action]=b.get(action,0)+1
    states, mutations = collections.Counter(), collections.Counter()
    expected_status = collections.Counter(); expected_search = []
    counts = {role:collections.Counter() for role in roles}
    previous = None; cursor = 0; last = None
    with (m/'raw.jsonl').open('rb') as stream:
        for projected in reads:
            stream.seek(offsets[projected['id']]); line=stream.readline(LINE_LIMIT+1)
            assert len(line)<=LINE_LIMIT
            row=json.loads(line);assert row['id']==projected['id'] and row['status']=='success'
            while cursor < len(builds) and builds[cursor]['finished_ns'] <= row['call_started_ns']:
                last=builds[cursor];cursor+=1
            expected_mutation=None if last is None else dict(action=last['mutation']['action'],ordinal=last['mutation_ordinal'],operation_id=last['id'],index_epoch=last['response']['resolution_freshness']['index_epoch'])
            probe=row['cache_probe'];assert probe['preceding_mutation']==expected_mutation
            observed=runtime.validate_cache_probe(probe,previous,m/'project')
            assert observed==probe['observation'] and row['response']==probe['requests'][1]['response']
            assert row['call_started_ns']<=probe['requests'][0]['started_ns'] and probe['requests'][-1]['finished_ns']<=row['finished_ns']
            for call in probe['requests']:
                assert call['status']=='success';counts[call['role']][call['status']]+=1
                key=(call['name'],canonical(call['arguments']),value_hash(call['response']))
                if call['name']=='search':expected_search.append(key)
                else:expected_status[key]+=1
            previous=observed;states[observed['lookup']['state']]+=1;states['invalidations']+=observed['invalidated']
            bucket=buckets[quarter(probe['requests'][2]['finished_ns'])];bucket['reads']+=1
            bucket['hits' if observed['expected']=='hit' else 'misses']+=1;bucket['invalidations']+=observed['invalidated']
            mutations[(probe['preceding_mutation'] or {}).get('action','not_observed')]+=1
    assert all(q['hits']>0 and q['misses']>0 and q['invalidations']>0 for q in buckets)
    cached=report['cache_reuse']
    assert cached['passed'] is True and cached['temporal_cache_coverage'] is True and cached['errors']==[]
    assert cached['protocol']==runtime.SOAK_READ_PROTOCOL and cached['expected_offered_reads']==cached['recorded_offered_reads']==cached['validated_reads']==expected
    assert cached['expected_requests_per_role']==expected and cached['request_counts']=={r:{'success':expected} for r in roles}
    assert cached['status_request_counts']=={'success':2*expected}
    assert cached['observed']==dict(states) and cached['time_quarters']==buckets and cached['preceding_mutation_coverage']==dict(mutations)
    endpoint=next(r for r in raw if r['kind']=='endpoint_public')
    expected_search.append(('search',canonical({'query':runtime.QUERY,'mode':'symbol','top_k':5}),value_hash(endpoint['incremental'])))
    pending={}; seen_ids=set(); actual_search={}; actual_status=collections.Counter(); requests=collections.Counter(); outcomes=collections.Counter()
    for _,event in lines(m/'product/rpc.jsonl'):
        if event['event']=='request':
            q=event['payload']; ident=q['id'];assert ident not in seen_ids;seen_ids.add(ident)
            if q.get('method')=='tools/call':
                params=q['params'];pending[ident]=(params['name'],canonical(params['arguments']))
                requests[(params['name'],canonical(params['arguments']))]+=1
            else:pending[ident]=None
        elif event['event']=='stdout_wire':
            wire=json.loads(event['text'])
            if 'id' not in wire:continue
            ident=wire['id'];assert ident in pending,'unsolicited or duplicate response'
            identity=pending.pop(ident)
            assert 'error' not in wire,'original product RPC error'
            if identity is None:continue
            value=wire['result']['structuredContent']['result'];key=(*identity,value_hash(value));outcomes[identity[0]]+=1
            if identity[0]=='search':actual_search[ident]=key
            elif identity[0]=='status':actual_status[key]+=1
    assert not pending, 'original RPC requests without responses'
    assert outcomes['search']==2*expected+1 and outcomes['index']==1202
    assert outcomes['status']==2*expected+sum(r['kind']=='resources' for r in raw)+1
    assert [actual_search[k] for k in sorted(actual_search)]==expected_search
    assert all(actual_status[k]>=n for k,n in expected_status.items()), 'cache status body missing from original wire response multiset'
    assert sum(n for (name,args),n in requests.items() if name=='search' and json.loads(args)['mode']=='hybrid')==expected
    assert sum(n for (name,args),n in requests.items() if name=='search' and json.loads(args)['mode']=='symbol')==expected+1
    return dict(protocol=runtime.SOAK_READ_PROTOCOL,expected_offered_reads=expected,validated_reads=expected,protocol_rpc_count=4*expected,observed=dict(states),time_quarters=buckets,preceding_mutation_coverage=dict(mutations),original_search_wire_order_and_full_responses_match=True,original_status_wire_multiset_covers_all_probe_snapshots=True,status_request_id_uniqueness_claim=False,original_product_request_outcome_counts=dict(outcomes),actual_concurrency_scope='soak serialized work, separate resource status sampling; not saturated configured C',worker_scope='same native process and shared query pool counters; not individual OS threads or semantic worker',source_witness_scope='current stable.py bytes under changing project generations; not every mutated entity',helper_sha256=file_hash(__file__))
