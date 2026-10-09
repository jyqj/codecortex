#!/usr/bin/env python3
"""Offline corroboration of retained G275 runtime/backfill originals.

This is a review aid, not a replacement protocol. It never starts codecortex,
an index/build, provider, or workload. The archived original pure Python
validators are reused without calling their run/main functions. Git commands
only read retained fixture objects. Output files are created exclusively.
"""
import argparse
from collections import Counter, defaultdict
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import traceback

SOURCE = '275e8799d4947d297329073eaa3ca675d3fd0777'
QUERY = 'p8_runtime_stable_signal'
ACTIONS = ['bounded_symbol_churn', 'add', 'rename', 'delete',
           'real_git_branch_switch', 'restore_api']
PARSER = argparse.ArgumentParser()
PARSER.add_argument('--evidence-root', type=Path, default=Path(__file__).resolve().parent)
PARSER.add_argument('--output', type=Path)
ARGS = PARSER.parse_args()
ROOT = ARGS.evidence_root.resolve()
OUTPUT = ARGS.output or ROOT / 'raw-observations.json'
sys.dont_write_bytecode = True
OBSERVER = ROOT / 'c1/extracted/p8-build/observer-source/scripts'
sys.path.insert(0, str(OBSERVER))
SPEC = importlib.util.spec_from_file_location('archived_g275_runtime', OBSERVER / 'p8_runtime.py')
ORIGINAL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ORIGINAL)


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def read(path):
    return json.loads(path.read_text())


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(',', ':'), allow_nan=False).encode()


def identity(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x') as stream:
        json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write('\n')


def integer(value, minimum=0):
    return type(value) is int and value >= minimum


def tool_key(name, arguments):
    return name, canonical(arguments).decode()


def status_projection(value):
    diag = value.get('diagnostics', value)
    return dict(server=diag['process_resources'], query_execution=diag['query_execution'],
                search_cache=diag['search_cache'])


def rpc_review(directory, expected, expected_projections, expected_statuses, native_pid):
    """Reconcile every request, send, original wire body and delivered response.

    Operation records do not carry JSON-RPC IDs. Their structured response
    multisets are reconciled exactly; identical responses cannot establish an
    individual operation-to-request ID assignment or backend service timings.
    """
    requests, sends, wires, responses = {}, {}, {}, {}
    events, counts, outcomes, projections, status_bodies = Counter(), Counter(), Counter(), Counter(), Counter()
    terminal = []
    for line_number, line in enumerate((directory / 'rpc.jsonl').open(), 1):
        row = json.loads(line)
        event = row['event']
        events[event] += 1
        require(integer(row['time_ns']), 'invalid RPC event timestamp')
        if event == 'request':
            payload = row['payload']; request_id = payload['id']
            require(integer(request_id, 1) and request_id not in requests, 'duplicate request ID')
            require(payload['jsonrpc'] == '2.0', 'request JSON-RPC version')
            requests[request_id] = row
            if payload['method'] == 'tools/call':
                params = payload['params']
                counts[tool_key(params['name'], params['arguments'])] += 1
            else:
                require(payload['method'] in ('initialize', 'tools/list'), 'unplanned RPC method')
        elif event == 'rpc_send':
            request_id = row['request_id']
            require(request_id in requests and request_id not in sends, 'missing/duplicate RPC send')
            require(row['offered_ns'] <= requests[request_id]['time_ns'] <= row['sent_ns'] <= row['time_ns'],
                    'RPC admission/send timestamps reversed')
            sends[request_id] = row['sent_ns']
        elif event == 'stdout_wire':
            body = row['text'].encode()
            require(row['wire_bytes'] == len(body) and row['wire_sha256'] == hashlib.sha256(body).hexdigest(),
                    'original wire byte length or digest')
            payload = json.loads(row['text']); request_id = payload['id']
            require(request_id in requests and request_id not in wires, 'unrequested/duplicate wire response')
            wires[request_id] = (identity(payload), row['time_ns'])
        elif event == 'response':
            payload = row['payload']; request_id = payload['id']
            require(request_id in sends and request_id in wires and request_id not in responses,
                    'unrequested/duplicate response')
            require(payload.get('jsonrpc') == '2.0' and 'error' not in payload and 'result' in payload,
                    'RPC returned error or malformed result')
            require(wires[request_id][0] == identity(payload), 'wire and delivered body differ')
            require(sends[request_id] <= wires[request_id][1] <= row['time_ns'], 'RPC response before send')
            responses[request_id] = row['time_ns']
            request = requests[request_id]['payload']
            if request['method'] == 'tools/call':
                result = payload['result']
                require(result.get('isError') is not True and isinstance(result.get('structuredContent'), dict),
                        'tool error/missing structured content')
                value = result['structuredContent'].get('result', result['structuredContent'])
                params = request['params']; key = tool_key(params['name'], params['arguments'])
                outcomes[(key, identity(value))] += 1
                if params['name'] == 'status':
                    projected = status_projection(value)
                    require(projected['server']['pid'] == native_pid, 'status belongs to another process')
                    projections[identity(projected)] += 1
                    status_bodies[identity(value)] += 1
        elif event == 'notification':
            require(row['payload'] == {'jsonrpc': '2.0', 'method': 'notifications/initialized'},
                    'unexpected client notification')
        elif event == 'terminal':
            require(row['kind'] in ('stdout_eof', 'process_exit') and not row.get('reason'),
                    'unexpected RPC terminal failure')
            if row['kind'] == 'process_exit':
                require(row['exit_code'] == 0, 'process exit failure')
            terminal.append(row)
        else:
            raise AssertionError('unplanned RPC event: ' + event)
    require(set(requests) == set(sends) == set(wires) == set(responses), 'unanswered/dropped request IDs')
    require(sorted(requests) == list(range(1, len(requests) + 1)), 'missing request sequence IDs')
    require(Counter(r['payload']['method'] for r in requests.values())['initialize'] == 1
            and Counter(r['payload']['method'] for r in requests.values())['tools/list'] == 1,
            'initialize/tool-list protocol differs')
    require(events['notification'] == 1 and Counter(r['kind'] for r in terminal) ==
            {'stdout_eof': 1, 'process_exit': 1}, 'missing native EOF/exit')
    require(all(r['time_ns'] >= max(responses.values()) for r in terminal), 'late response after EOF/exit')
    expected_counts = Counter()
    for (key, digest), n in expected.items():
        expected_counts[key] += n
    status_key = tool_key('status', {'aspect': 'index'})
    if expected_projections:
        expected_counts[status_key] = sum(expected_projections.values())
    require(counts == expected_counts, 'actual tool request denominator/arguments differ')
    actual_nonstatus = Counter({key: n for key, n in outcomes.items() if key[0][0] != 'status'})
    require(actual_nonstatus == expected, 'original operation/endpoint responses differ from wire records')
    require(projections == expected_projections, 'resource/cache/endpoint status projections differ from wire')
    require(not (expected_statuses - status_bodies), 'compound cache status body not found in original RPC')
    return dict(status='passed', original_rpc_file_sha256=sha(directory / 'rpc.jsonl'),
                events=dict(events), requests=len(requests), delivered_responses=len(responses),
                original_wire_hashes_verified=True, every_request_id_closed=True,
                structured_response_and_status_projection_multisets_equal=True,
                tool_requests=[dict(name=k[0], arguments=json.loads(k[1]), n=n)
                               for k, n in sorted(counts.items())],
                terminal_events=terminal,
                attribution_limit='raw operation rows omit RPC IDs; exact response multiset corroboration, '
                    'not an individual ID assignment for identical responses or separated backend queue/service timing')


def interval_summary(rows):
    events = []
    for row in rows:
        events.extend([(row['call_started_ns'], 1, row['operation']), (row['finished_ns'], -1, row['operation'])])
    active = Counter(); maximum = 0; overlap = False
    for at, delta, kind in sorted(events):
        active[kind] += delta
        require(active[kind] >= 0, 'invalid operation interval')
        maximum = max(maximum, sum(active.values()))
        overlap |= active['read'] > 0 and active['build'] > 0
    require(not any(active.values()), 'operation interval remains open')
    return dict(maximum_call_to_finish_intervals=maximum, read_build_interval_overlap=overlap,
                scope='call-start through final validation; upper bound on the narrower driver active-call counter')


def fixture_inventory(project):
    return {p.relative_to(project).as_posix(): sha(p) for p in sorted(project.rglob('*'))
            if p.is_file() and p.relative_to(project).parts[0] not in ('.git', '.codecortex')}


def runtime_review(key):
    base = ROOT / key / 'extracted/p8-runtime'
    plan, report = read(base / 'plan.json'), read(base / 'report.json')
    rows = [json.loads(line) for line in (base / 'raw.jsonl').open()]
    by_kind = defaultdict(list)
    for row in rows:
        by_kind[row['kind']].append(row)
    operations, samples = by_kind['operation'], by_kind['resources']
    require(set(by_kind) == {'operation', 'resources', 'initial_build', 'endpoint_status', 'full_control',
                             'endpoint_public', 'oracle_process'}, 'missing/unexpected raw event kind')
    require(all(len(by_kind[k]) == 1 for k in set(by_kind) - {'operation', 'resources'}), 'raw control cardinality')
    require(plan['source']['source_commit'] == SOURCE and plan['files'] == 1000, 'source/fixture protocol')
    require(plan['local_semantic_state'] == 'disabled' and plan['paid_provider_requests'] == 0,
            'runtime provider profile changed')
    require(plan['queue_capacity'] == 128 and plan['resource_interval_seconds'] == 1
            and plan['request_timeout_seconds'] == 60, 'predeclared runtime bounds changed')
    require(plan['operations'] == (3601 if key == 'soak' else 900), 'fixed offered denominator changed')
    require(plan['concurrency'] == (4 if key == 'soak' else int(key[1:])), 'concurrency cell mismatch')
    require(sorted(r['id'] for r in operations) == list(range(plan['operations'])), 'offered IDs duplicated/missing')
    require(Counter(r['status'] for r in operations) == {'success': plan['operations']}, 'non-success raw outcome')
    start = min(r['scheduled_ns'] for r in operations)
    for row in operations:
        require(row['operation'] == ('build' if row['id'] % 3 == 0 else 'read'), 'operation assignment changed')
        times = [row[k] for k in ('scheduled_ns', 'offered_ns', 'started_ns', 'call_started_ns', 'finished_ns')]
        require(all(integer(v) for v in times) and times == sorted(times), 'invalid original operation timing')
        tick = row['id'] if key == 'soak' else row['id'] // plan['concurrency'] * plan['concurrency']
        require(row['scheduled_ns'] == start + tick * plan['offer_interval_ms'] * 1_000_000,
                'unreported offered schedule change')
        require('error' not in row, 'successful row retains hidden error')
        if row['operation'] == 'read':
            ORIGINAL.require_stable_symbol(row['response'])
        else:
            require(row['response']['parse_errors'] == [] and row['response']['resolution_freshness']['complete'] is True,
                    'incremental parse/closure report not complete')
    builds = sorted((r for r in operations if r['operation'] == 'build'), key=lambda r:r['mutation_ordinal'])
    require([r['mutation_ordinal'] for r in builds] == list(range(len(builds))), 'mutation admission ordinals missing')
    require(all(a['finished_ns'] <= b['call_started_ns'] for a,b in zip(builds,builds[1:])), 'build admission overlap')
    project = base / 'project'
    branch_commits = {}
    for branch, result in [('p8-a', 1), ('p8-b', 2)]:
        command = ['git', '--no-optional-locks', '-C', str(project)]
        commit = subprocess.check_output(command + ['rev-parse', 'refs/heads/' + branch]).decode().strip()
        body = subprocess.check_output(command + ['show', commit + ':branch.py'])
        require(body == f'def branch_signal():\n    return {result}\n'.encode(), 'retained branch object bytes')
        branch_commits[branch] = commit
    for row in builds:
        ordinal, mutation = row['mutation_ordinal'], row['mutation']
        action = ACTIONS[ordinal % 6]
        require(mutation['action'] == action, 'six-action mutation cycle changed')
        if action == 'real_git_branch_switch':
            branch = 'p8-b' if (ordinal // 6) % 2 == 0 else 'p8-a'
            require(mutation['branch'] == branch and mutation['exit_code'] == 0
                    and mutation['commit'] == branch_commits[branch], 'branch switch lacks successful retained Git identity')
            require(mutation['argv'][-3:] == ['switch', '--discard-changes', branch], 'actual branch command differs')
        elif action == 'bounded_symbol_churn':
            require(mutation == dict(action=action, generation=ordinal + 1, symbols=128), 'churn witness differs')
        else:
            wanted = {'add': dict(action=action, path='temporary.py'),
                      'rename': dict(action=action, before='temporary.py', after='renamed.py'),
                      'delete': dict(action=action, path='renamed.py'),
                      'restore_api': dict(action=action, path='churn.py')}[action]
            require(mutation == wanted, 'mutation paths differ')
    fixture = fixture_inventory(project)
    require(fixture == fixture_inventory(base / 'fresh-full'), 'fresh-full source bytes differ from incremental endpoint')
    require(sum(p.endswith('.py') for p in fixture) == 1000, 'terminal fixture file denominator')
    require((project / 'stable.py').read_bytes() == f'def {QUERY}():\n    return 7\n'.encode(), 'stable fixture bytes changed')
    for kind in ['initial_build', 'full_control']:
        witness = by_kind[kind][0]['report']
        require(witness['parse_errors'] == [] and witness['resolution_freshness']['complete'] is True,
                'initial/full parse or closure error')
    endpoint = by_kind['endpoint_public'][0]
    ORIGINAL.require_stable_symbol(endpoint['incremental']); ORIGINAL.require_stable_symbol(endpoint['full'])
    require(endpoint['incremental'] == endpoint['full'], 'public endpoint mismatch')
    require(by_kind['oracle_process'][0]['exit_code'] == report['parity_exit_code'] == 0, 'original endpoint oracle failure')
    require(sha(base / 'parity.json') == report['parity_sha256'], 'parity receipt bytes')
    require(report['outcomes'] == {'success': len(operations)} and report['offered'] == len(operations), 'reported denominators')
    require(ORIGINAL.latency_summary(operations) == report['latency'], 'original descriptive all-outcome latency differs')
    require({kind: ORIGINAL.latency_summary([r for r in operations if r['operation'] == kind]) for kind in ('read','build')}
            == report['latency_by_operation'], 'original operation descriptive latency differs')
    coverage = report['resource_time_coverage']; end = coverage['work_end_ns']
    require(coverage['work_start_ns'] == start and end >= max(r['finished_ns'] for r in operations), 'observed work endpoints')
    require(0 < report['observed_work_ns'] <= end - start, 'observed work duration contradicts original endpoint clocks')
    rss = ORIGINAL.rss_trend(samples)
    require(rss == report['rss'], 'native RSS trend recomputation differs')
    require(ORIGINAL.sample_coverage(samples, start, end) == coverage, 'resource coverage recomputation differs')
    compactions = (base / 'product/product-stderr.log').read_text().count(ORIGINAL.COMPACTION_EVENT)
    switches = sum(r['mutation']['action'] == 'real_git_branch_switch' for r in builds)
    require(compactions == report['observed_catalog_compactions'] and switches == report['real_branch_switches'],
            'catalog/branch report count differs from original logs')
    intervals = interval_summary(operations)
    require(1 <= report['actual_concurrency']['maximum'] <= intervals['maximum_call_to_finish_intervals'] <= plan['concurrency'],
            'active client concurrency exceeds configured or retained intervals')
    if key not in ('c1', 'soak'):
        require(intervals['read_build_interval_overlap'] and report['actual_concurrency']['read_build_overlap'],
                'mixed concurrent cell lacks actual overlap')
    processes = {side: read(base / side / 'process.json') for side in ('product','full-product')}
    for side, record in processes.items():
        require(record['binary_sha256'] == plan['product_sha256'] and record['exit_code'] == record['expected_exit_code'] == 0
                and record['initialized'] is True and record['tool_count'] == 14 and record['cleanup'] == 'completed',
                'original product identity/process lifecycle')
        require(record['environment']['TOKIO_WORKER_THREADS'] == record['environment']['RAYON_NUM_THREADS'] == '2',
                'runtime thread environment differs')
        require(record['command'][1:3] == ['mcp','--project-path'], 'actual product command differs')
    server_pid = processes['product']['pid']
    require(len({r['runner']['pid'] for r in samples}) == 1
            and all(r['server']['pid'] == server_pid != r['runner']['pid'] for r in samples), 'native resource owner identity')
    for metric in ['user_cpu_ns','system_cpu_ns','peak_resident_bytes']:
        values = [r['server'][metric] for r in samples]
        require(all(integer(v) for v in values) and values == sorted(values), 'native process resource counters regressed')
    for k in ['product_stopped','comparison_product_stopped','sampler_stopped']:
        require(report['owned_cleanup'][k] is True, 'owned process/sampler not stopped')
    require(report['owned_cleanup']['unfinished_work'] == 0, 'unfinished offered work')
    expected, projections, statuses = Counter(), Counter(), Counter()
    def expect(name, arguments, value):
        if name == 'status':
            projections[identity(status_projection(value))] += 1
            statuses[identity(value)] += 1
        else:
            expected[(tool_key(name, arguments), identity(value))] += 1
    original_project = processes['product']['command'][-1]
    expect('index', {'path':original_project, 'full':True}, by_kind['initial_build'][0]['report'])
    for row in operations:
        if row['operation'] == 'build':
            expect('index', {'path':original_project, 'full':False}, row['response'])
        elif key == 'soak':
            for call in row['cache_probe']['requests']:
                expect(call['name'], call['arguments'], call['response'])
        else:
            expect('search', {'query':QUERY,'mode':'symbol','top_k':5}, row['response'])
    expect('search', {'query':QUERY,'mode':'symbol','top_k':5}, endpoint['incremental'])
    for row in samples:
        projections[identity({k:row[k] for k in ('server','query_execution','search_cache')})] += 1
    projections[identity(status_projection(by_kind['endpoint_status'][0]['response']))] += 1
    product_rpc = rpc_review(base / 'product', expected, projections, statuses, server_pid)
    expected = Counter()
    full_project = processes['full-product']['command'][-1]
    expected[(tool_key('index', {'path':full_project,'full':True}), identity(by_kind['full_control'][0]['report']))] = 1
    expected[(tool_key('search', {'query':QUERY,'mode':'symbol','top_k':5}), identity(endpoint['full']))] = 1
    full_rpc = rpc_review(base / 'full-product', expected, Counter(), Counter(), processes['full-product']['pid'])
    answer = dict(key=key, status='passed', configured_concurrency=plan['concurrency'],
                  outcomes=dict(Counter(r['status'] for r in operations)), offered=len(operations),
                  operations=dict(Counter(r['operation'] for r in operations)),
                  mutation_counts=dict(Counter(r['mutation']['action'] for r in builds)),
                  reported_active_client_concurrency=report['actual_concurrency'], interval_corroboration=intervals,
                  branch_commits=branch_commits, real_branch_switches=switches, observed_catalog_compactions=compactions,
                  raw_kind_counts={k:len(v) for k,v in by_kind.items()},
                  work_start_ns=start, raw_last_operation_finished_ns=max(r['finished_ns'] for r in operations),
                  raw_last_finished_minus_first_scheduled_ns=max(r['finished_ns'] for r in operations)-start,
                  original_observed_work_ns=report['observed_work_ns'], rss=rss, resource_time_coverage=coverage,
                  latency=report['latency'], latency_by_operation=report['latency_by_operation'],
                  endpoint_fixture_files_equal=True, endpoint_fixture_inventory_sha256=identity(fixture),
                  incremental_product_has_only_initial_full_index_and_planned_incrementals=True,
                  no_late_catchup_index_in_original_rpc=True, processes=processes,
                  product_rpc=product_rpc, full_control_rpc=full_rpc,
                  original_hashes={name:sha(base / name) for name in
                      ('plan.json','raw.jsonl','report.json','statistics.json','parity.json','seal.json')},
                  full_task_complete=False, release_certified=False)
    if key == 'soak':
        cache = ORIGINAL.soak_cache_summary(operations, plan['operations'], start, end, project)
        require(cache == report['cache_reuse'] and cache['passed'] is True, 'original cache predicate/raw recomputation')
        require(report['observed_work_ns'] >= 3_600_000_000_000
                and answer['raw_last_finished_minus_first_scheduled_ns'] >= 3_600_000_000_000
                and rss['passed'] and coverage['passed'] and compactions > 0 and switches >= 2,
                'original soak hard observation predicates fail')
        require(intervals['maximum_call_to_finish_intervals'] == 1, 'soak shared admission lock differs')
        answer['cache_reuse'] = cache
    return answer


def backfill_review():
    base = ROOT / 'backfill/extracted'
    receipt = read(base / 'receipt.json')
    require(receipt['source_before']['source_commit'] == SOURCE
            and receipt['execution_exit_code'] == 0 and receipt['status'] == 'passed_observation',
            'selected original backfill execution/source')
    stdout = (base / 'execution.stdout').read_text()
    require('test real_slow_backfill_preserves_local_progress_and_records_every_request ... ok' in stdout
            and '1 passed; 0 failed; 0 ignored' in stdout, 'selected original backfill test did not pass')
    results = []
    for seed in (7,19,43):
        directory = base / 'raw' / ('seed-' + str(seed))
        protocol, held, summary = [read(directory / (n + '.json')) for n in ('protocol','held-before','summary')]
        require(protocol['seed'] == summary['seed'] == seed and protocol['concurrency_cells'] == [1,4,8,16]
                and protocol['phases'] == ['quiet','held'] and protocol['samples_per_cell'] == 32,
                'fixed backfill seed/cell denominator')
        for name, wanted in [('mutable_files',24),('fake_delay_ms',10),('progress_watchdog_ms',5000),
                             ('query_watchdog_ms',2000),('writer_busy_timeout_ms',100),
                             ('worker_local_attempt_width',4),('worker_claim_round_cap',16)]:
            require(protocol[name] == wanted, 'backfill original bound changed: ' + name)
        source = f'pub fn stable_signal() -> u64 {{ {seed} }}\n'.encode()
        cells, total = [], 0
        for phase in ['quiet','held']:
            rows = read(directory / (phase + '-requests.json'))
            require(len(rows) == 128 and len({(r['concurrency'],r['ordinal']) for r in rows}) == 128,
                    'backfill phase dropped/duplicated request')
            for concurrency in [1,4,8,16]:
                subset = [r for r in rows if r['concurrency'] == concurrency]
                require(sorted(r['ordinal'] for r in subset) == list(range(32)), 'backfill per-cell denominator')
                values = []
                for row in subset:
                    metrics = [row[k] for k in ('caller_schedule_us','capture_admission_us','retrieval_us','offered_to_api_return_us')]
                    require(all(integer(v) for v in metrics) and 0 <= metrics[-1]-sum(metrics[:-1]) <= 2,
                            'backfill elapsed components disagree beyond integer microsecond flooring')
                    require(metrics[-1] < protocol['query_watchdog_ms'] * 1000, 'backfill original query watchdog')
                    require(row['originating_work'] is None and row['source_freshness'] is None,
                            'raw null provenance unexpectedly relabeled')
                    require(row['scope'] == 'real local API; no MCP transport or separated backend queue/service timing',
                            'backfill sample scope differs')
                    hits = [h for h in row['hits'] if h['file_path'] == 'stable.rs' and h['symbol_name'] == 'stable_signal']
                    require(len(hits) == 1, 'real stable source hit missing/duplicated')
                    hit = hits[0]; proof = hit['metadata']['source_evidence']; span = proof['span']
                    fresh = hit['metadata']['source_freshness']
                    require(hit['start_line'] == hit['end_line'] == 1
                            and fresh['status'] == 'current_verified' and fresh['disk_checked'] is True
                            and span['start'] == 0 and span['end'] in (len(source),len(source)-1)
                            and proof['source']['byte_len'] == len(source) and proof['source']['encoding'] == 'utf8'
                            and hit['text'].encode() == source[span['start']:span['end']],
                            'backfill retained hit does not match fixed source/entity/span bytes')
                    values.append(row['offered_to_api_return_us'])
                values.sort()
                quantiles = dict(concurrency=concurrency, n=32, p50_us=values[15], p95_us=values[30],
                                 p99_us=values[31], max_us=values[-1])
                original_cell = next(c for c in summary[phase] if c['concurrency'] == concurrency)
                require(quantiles == {k:original_cell[k] for k in quantiles}, 'backfill all-sample nearest-rank summary differs')
                cells.append(dict(phase=phase, **quantiles)); total += len(subset)
        provider, queue = held['provider'], held['queue']
        require(provider['active'] == provider['waiting'] == queue['claimed'] == 4
                and len(set(provider['held_inputs'])) == 4 and provider['maximum_active'] <= 4
                and queue['pending'] > 0 and queue['published'] > 0 and queue['uncovered'] > 0,
                'actual held provider/claimed/pending/published witness absent')
        db = summary['db_availability']
        require(db['generation_before'] == db['generation_after'] and db['generation_unchanged'] is True
                and db['read_rows'] == 26 and db['writer_busy_timeout_ms'] == 100
                and integer(db['writer_acquire_and_rollback_us']), 'held database transaction control failed')
        require(summary['old_held_input_publications'] == 0 and summary['new_provider_calls'] > 0
                and summary['old_provider']['active'] == summary['old_provider']['waiting'] == 0
                and summary['old_provider']['held_inputs'] == provider['held_inputs']
                and summary['old_provider']['maximum_active'] <= 4, 'retirement/drain witness failed')
        require(summary['queue_final']['pending'] == summary['queue_final']['claimed'] == summary['queue_final']['uncovered'] == 0
                and summary['queue_final']['published'] > 0, 'final replacement publication coverage')
        stages = [summary['quiet_resources'],held['resources'],summary['held_after_resources'],summary['final_resources']]
        owners = [s['shared_process_owner']['usage'] for s in stages]
        require(len({s['pid'] for s in owners}) == 1, 'backfill shared process owner changes')
        for stage in stages:
            require(stage['resource_gate'] == 'attributed_combined_process'
                    and stage['shared_process_owner']['components'] == ['test_runner','CodeIndex','fake_provider']
                    and stage['server']['separate_pid'] is None
                    and stage['server_tree']['child_process_usage'] is None
                    and stage['raw_sampler']['server_rss_bytes'] is None
                    and stage['raw_sampler']['server_tree_rss_bytes'] is None
                    and stage['raw_sampler']['external_service_rss_bytes'] is None,
                    'backfill combined owner/null attribution changed')
        for metric in ['user_cpu_ns','system_cpu_ns','peak_resident_bytes']:
            values = [r[metric] for r in owners]
            require(all(integer(v) for v in values) and values == sorted(values), 'backfill native resource counters regress')
        require(summary['full_p7_015_complete'] is False and protocol['full_task_acceptance'] is False
                and protocol['performance_sla'] is None, 'unsupported whole-task/performance claim')
        results.append(dict(seed=seed, recorded_requests=total, fixed_cells=cells, held_provider=provider,
                            held_queue=queue, db_availability=db,
                            old_held_input_publications=summary['old_held_input_publications'],
                            new_provider_calls=summary['new_provider_calls'], queue_final=summary['queue_final'],
                            native_shared_owner_pid=owners[0]['pid'],
                            resource_scope='test_runner + CodeIndex + in-process fake_provider; no separate server/tree/external RSS',
                            retained_raw_sha256={p.name:sha(p) for p in sorted(directory.iterdir()) if p.is_file()},
                            status='passed'))
    return dict(key='backfill', status='passed', source_commit=SOURCE, seeds=results,
                total_original_requests=sum(r['recorded_requests'] for r in results),
                original_test_exit_code=receipt['execution_exit_code'],
                original_stdout_sha256=sha(base / 'execution.stdout'),
                scope='actual post-index worker with held in-process synthetic provider; original selected test not rerun',
                nonduplicated_evidence_limit='provider/queue unchanged after held queries and stale-symbol deletion are '
                    'assertions in the fixed source executed by the retained passing binary; no separately retained '
                    'held-after queue snapshot or final database is invented',
                performance_scope='descriptive all 32 samples per phase/concurrency/seed; fixed progress watchdogs, '
                    'no new SLA/confidence interval or certified tail stability', full_task_complete=False)


if __name__ == '__main__':
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    results = []
    try:
        for key in ['c1','c4','c8','c16','soak']:
            results.append(runtime_review(key))
            print(json.dumps(dict(key=key, status='passed', offered=results[-1]['offered'])), flush=True)
        results.append(backfill_review())
        print(json.dumps(dict(key='backfill', status='passed', requests=results[-1]['total_original_requests'])), flush=True)
        write(OUTPUT, dict(schema_version=1, status='passed', source_commit=SOURCE,
              started_at=started, finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
              original_validator_sha256=sha(OBSERVER / 'p8_runtime.py'), reviewer_script_sha256=sha(Path(__file__)),
              results=results, scope='offline retained evidence corroboration; no new product/build/index/workload execution',
              full_task_complete=False, release_certified=False, remaining_original_todos=29))
    except Exception:
        write(OUTPUT.with_name(OUTPUT.stem + '-failure.json'), dict(status='review_did_not_complete', completed=results,
              started_at=started, traceback=traceback.format_exc(),
              note='review aid failure is not itself a product-workload failure'))
        raise
